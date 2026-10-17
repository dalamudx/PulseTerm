#!/bin/sh
# Four isolated real GUI processes; metadata only, no terminal or network startup.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/workspace-identities.XXXXXX)
cleanup(){ rm -rf "$fixture"; test ! -e "$fixture"; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture"
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
export OMP_THREAD_LIMIT=1 PYTHONDONTWRITEBYTECODE=1
# HOME and pinned toolchain directories remain unchanged. The Rust fixture receives only our private XDG directory.
export PULSE_KEY_METADATA_PROBE_ROOT="$fixture"
python3 scripts/probe-workspace-key-identities.py
