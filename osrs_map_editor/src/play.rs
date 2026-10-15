//! Starting the game on a compiled cache, from the editor.
//!
//! The game runs as its own process with `OSRS_CACHE_DIR` naming the cache, so playing a project
//! does not change which cache the game reads when started any other way. It is started with
//! `cargo run -p osrs_core --release` from this workspace when the workspace is there (so it is
//! built from the current source), else as the `osrs_core` executable beside the editor's.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace the editor was built from, if it is still there.
fn workspace() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.to_path_buf();
    root.join("Cargo.toml").exists().then_some(root)
}

/// The game executable next to the editor's.
fn sibling_game() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let game = exe.with_file_name(format!("osrs_core{}", std::env::consts::EXE_SUFFIX));
    game.exists().then_some(game)
}

/// Starts the game reading the cache in `cache_dir`, entering the world on `spawn` if given.
/// Returns how it was started.
pub fn launch(cache_dir: &Path, spawn: Option<(i32, i32)>) -> Result<String, String> {
    let mut command = if let Some(root) = workspace() {
        let mut c = Command::new("cargo");
        c.args(["run", "--release", "-p", "osrs_core"]).current_dir(root);
        c
    } else if let Some(game) = sibling_game() {
        Command::new(game)
    } else {
        return Err("cannot find the game: no workspace to build it from and no osrs_core beside the editor".into());
    };
    // (this replaces the editor's own OSRS_CACHE_DIR, which names its base cache)
    command.env(osrs_cache::active::ENV_CACHE_DIR, cache_dir);
    match spawn {
        Some((x, z)) => command.env("OSRS_SPAWN", format!("{x},{z}")),
        None => command.env_remove("OSRS_SPAWN"),
    };
    let how = format!("{:?}", command.get_program());
    command.spawn().map_err(|e| format!("could not start the game ({how}): {e}"))?;
    Ok(how)
}
