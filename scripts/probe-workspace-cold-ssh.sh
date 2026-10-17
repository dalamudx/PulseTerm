#!/bin/sh
set -eu
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/cold-ssh-window.XXXXXX)
trap 'rm -rf -- "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
python3 scripts/probe-workspace-cold-ssh.py
