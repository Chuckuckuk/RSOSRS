#!/usr/bin/env bash
# Builds dist/RSOSRS-Jad.exe: the client with the slice of the game cache the Jad fight reads
# appended to it, so it runs without the game installed and starts straight in the fight.
# Needs the game cache on this machine (RuneLite's). The result contains Jagex's data: share it
# with a few friends if you like, never publish it.
#   ./package_jad.sh
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release -p osrs_core -p osrs_cache
mkdir -p dist
# a recording run: plays the healer phase for ~75 s and notes every cache group read, then adds
# every item picture and sound the data can ask for
OSRS_RECORD_BUNDLE=dist/jad.bundle OSRS_SCREENSHOT=dist/recording.png OSRS_SCREENSHOT_DELAY=70 \
  ./target/release/osrs_core.exe --scenario jad_healers
./target/release/bundle_pack.exe target/release/osrs_core.exe dist/jad.bundle dist/RSOSRS-Jad.exe
rm -f dist/recording.png
