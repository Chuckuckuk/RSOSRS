//! Appends a recorded cache payload to a copy of a program: bundle_pack <program.exe> <payload> <out.exe>
//! The result runs without the game installed (see `osrs_cache::bundle`). The payload holds game
//! data: keep it, and what this makes, for private use.
use osrs_cache::bundle::{pack_exe, Bundle};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [exe, payload, out] = args.as_slice() else {
        eprintln!("usage: bundle_pack <program.exe> <payload> <out.exe>");
        std::process::exit(2);
    };
    let exe_bytes = std::fs::read(exe).expect("the program is readable");
    let payload_bytes = std::fs::read(payload).expect("the payload is readable");
    let groups = Bundle::parse(payload_bytes.clone()).expect("the payload is well formed").len();
    std::fs::write(out, pack_exe(&exe_bytes, &payload_bytes)).expect("the output is writable");
    println!(
        "{out}: {} MB program + {} MB payload ({groups} cache groups)",
        exe_bytes.len() / 1_000_000,
        payload_bytes.len() / 1_000_000
    );
}
