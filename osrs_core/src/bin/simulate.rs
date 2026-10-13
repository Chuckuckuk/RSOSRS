//! Headless runner for data-defined fights:
//!
//! ```text
//! cargo run --bin simulate -- <scenario.json | builtin-name> [--ticks N] [--data DIR] [--quiet]
//! cargo run --bin simulate -- --list
//! ```
//!
//! It loads the item/prayer/spell/monster data (the bundled set, or every `*.json` in `--data`),
//! builds the scenario, prints the expected damage per second of the player's setup against each
//! monster, then runs the ticks and prints what happened.

use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;

use osrs_core::data::{builtin_scenarios, load_scenario, Registry};
use osrs_core::sim::scenarios::build_scenario;

struct Options {
    scenario: Option<String>,
    ticks: i32,
    data_dir: Option<String>,
    quiet: bool,
    list: bool,
}

fn parse_args() -> Result<Options, String> {
    let mut options = Options { scenario: None, ticks: 100, data_dir: None, quiet: false, list: false };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ticks" => {
                options.ticks = args.next().ok_or("--ticks needs a number")?.parse().map_err(|_| "--ticks needs a number")?;
            }
            "--data" => options.data_dir = Some(args.next().ok_or("--data needs a directory")?),
            "--quiet" => options.quiet = true,
            "--list" => options.list = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => options.scenario = Some(other.to_owned()),
        }
    }
    Ok(options)
}

fn run() -> Result<(), String> {
    let options = parse_args()?;
    if options.list {
        for (name, _) in builtin_scenarios() {
            println!("{name}");
        }
        return Ok(());
    }
    let scenario_name = options.scenario.as_deref().ok_or("usage: simulate <scenario.json | builtin-name> [--ticks N] [--data DIR] [--quiet]")?;

    let registry = match &options.data_dir {
        Some(dir) => Registry::load_dir(Path::new(dir)).map_err(|e| e.to_string())?,
        None => Registry::builtin(),
    };
    let scenario = load_scenario(scenario_name)?;
    let mut world = build_scenario(Rc::new(registry), &scenario)?;

    println!("== {} (seed {}) ==", if scenario.name.is_empty() { scenario_name } else { &scenario.name }, scenario.seed);
    println!("player combat level {}", world.player.combat_level());
    let registry = Rc::clone(world.registry());
    for entity in world.entities() {
        let stats = entity.view().stats;
        match world.player.expected_dps(&registry, &stats) {
            Ok(dps) => println!(
                "vs {:<18} hit chance {:>5.1}%  max hit {:>3}  swing every {} ticks  expected {:.2} dmg/s",
                entity.name,
                dps.analysis.hit_chance * 100.0,
                dps.analysis.max_hit,
                dps.swing_ticks,
                dps.per_second
            ),
            Err(reason) => println!("vs {:<18} cannot attack: {reason}", entity.name),
        }
    }

    let mut printed_log = 0;
    for _ in 0..options.ticks {
        world.run_tick();
        let new_lines = &world.log()[printed_log..];
        if !options.quiet {
            for line in new_lines {
                println!("  t={:<4} {line}", world.current_tick());
            }
        }
        printed_log = world.log().len();
        if !options.quiet {
            println!("{}", world.snapshot());
        }
        if world.player.is_dead() {
            println!("the player died on tick {}", world.current_tick());
            break;
        }
    }

    println!("-- after {} ticks --", world.current_tick());
    println!("{}", world.snapshot());
    println!(
        "prayer {}/{}  run energy {}%  special {}%",
        world.player.prayer.current(),
        world.player.prayer.max(),
        world.player.run_energy / 100,
        world.player.special_energy
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
