#!/bin/sh
set -eu
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/rpm.XXXXXX)
trap 'rm -rf -- "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
cc -Wall -Wextra -Werror scripts/probe-x11-status.c -o "$fixture/native-helper" $(pkg-config --cflags --libs x11 atspi-2 gobject-2.0 glib-2.0)
export PULSE_ROUTE_PASSWORD_INTERACTIVE=1 PULSE_NATIVE_HELPER="$fixture/native-helper"
python3 scripts/probe-workspace-cold-ssh.py
