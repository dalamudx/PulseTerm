#!/bin/sh
# CLI and route scenarios share the same cold-start driver.
set -eu
mode=${1:-cli}
[ "$#" -le 1 ] || exit 2
case "$mode" in cli|route|cli-key|route-key|cli-agent|cli-password|route-password) ;; *) echo 'unknown authentication scenario' >&2; exit 2;; esac
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/cli-mfa.XXXXXX)
trap 'rm -rf -- "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
cc -Wall -Wextra -Werror scripts/probe-x11-status.c -o "$fixture/native-helper" $(pkg-config --cflags --libs x11 atspi-2 gobject-2.0 glib-2.0)
flag=$(printf '%s' "$mode" | tr '[:lower:]-' '[:upper:]_')
export "PULSE_${flag}_INTERACTIVE=1" PULSE_NATIVE_HELPER="$fixture/native-helper"
python3 scripts/probe-workspace-cold-ssh.py
