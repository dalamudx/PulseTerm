#!/bin/sh
# Explicit controlled fixtures; never discovers user configuration.
set -eu
mkdir -p /data/temp target/probes
scratch=$(mktemp -d /data/temp/pulseterm-keymap-probe.XXXXXX)
trap 'rm -rf "$scratch"' EXIT
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch"
cargo run -p pulse-store --example keymap_projection --locked > "$scratch/corpus.json"
pnpm --filter ui exec tsx scripts/keymap-probe.ts "$scratch/corpus.json"
cp "$scratch/corpus.json" target/probes/keymap-projection-corpus.json
cargo build -p pulse-store --example keymap_file_probe --locked
python3 scripts/probe-keymap-file.py
