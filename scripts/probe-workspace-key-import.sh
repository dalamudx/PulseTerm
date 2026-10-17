#!/bin/sh
# Synthetic encrypted Ed25519 input, two real GUI processes, independent Rust AEAD check.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/workspace-key-import.XXXXXX)
cleanup(){ rm -rf "$fixture"; test ! -e "$fixture"; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture"
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache" XDG_RUNTIME_DIR="$fixture/run"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
export OMP_THREAD_LIMIT=1 PYTHONDONTWRITEBYTECODE=1 PULSE_KEY_IMPORT_FIXTURE_ROOT="$fixture"
cc -Wall -Wextra -Werror scripts/probe-x11-clipboard.c -o "$fixture/clipboard" $(pkg-config --cflags --libs gtk+-3.0)
python3 scripts/probe-workspace-key-import.py
