#!/bin/sh
# One metadata-only GUI chain in two processes; fixture never reads credentials or starts a terminal.
set -eu
umask 077
cargo=$(mise which cargo)
RUSTC=$(mise which rustc)
export RUSTC
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/catalogue-batch.XXXXXX)
cleanup(){ rm -rf -- "$fixture"; test ! -e "$fixture"; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture"
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
export PULSE_CATALOGUE_BATCH_DIRECTORY="$XDG_DATA_HOME/dev.pulseterm.workspace-preview"
"$cargo" test --offline -p pulseterm-desktop --lib --features workspace --locked product_workspace::hosts::batch_tests::catalogue_window_fixture -- --exact --ignored --nocapture > target/probes/catalogue-batch-fixture.log 2>&1
grep -F 'workspace.catalogue_batch_fixture_confirmed' target/probes/catalogue-batch-fixture.log >/dev/null
grep -F '1 passed' target/probes/catalogue-batch-fixture.log >/dev/null
export PULSE_CATALOGUE_BATCH_ROOT="$fixture" PYTHONDONTWRITEBYTECODE=1
if [ "${PULSE_CATALOGUE_ORGANIZATION:-0}" = 1 ]; then
    python3 scripts/probe-workspace-organization.py
else
    python3 scripts/probe-workspace-catalogue-batch.py
fi
