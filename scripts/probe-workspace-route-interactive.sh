#!/usr/bin/env bash
set -euo pipefail
mkdir -p /data/temp
work=$(mktemp -d /data/temp/route-mfa.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
export TMPDIR="$work" TMP="$work" TEMP="$work"
cc scripts/probe-x11-status.c -o "$work/a11y" $(pkg-config --cflags --libs atspi-2 x11 gobject-2.0)
export PULSE_NATIVE_HELPER="$work/a11y" PULSE_ROUTE_INTERACTIVE=1
python3 scripts/probe-workspace-cold-ssh.py
