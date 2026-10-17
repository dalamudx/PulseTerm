#!/bin/sh
# Real original-window SSH authority with a second WebView in the same process.
set -eu
mode=${1:-password}
case "$mode" in password|agent|route|all);; *) exit 2;; esac
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/multiwindow-ssh-live.XXXXXX)
cleanup() { case "$fixture" in /data/temp/multiwindow-ssh-live.*) rm -rf -- "$fixture";; *) exit 1;; esac; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
if [ "$mode" = all ]; then
  pnpm --filter ui build
  cargo build -p pulseterm-desktop --features workspace-cli,workspace-ssh --bin pulseterm-workspace --bin pulse --locked
  cargo build -p xtask --features cli-probe,ssh-private-key-probe --locked
  cc -Wall -Wextra -Werror scripts/probe-x11-status.c -o target/probes/x11-status $(pkg-config --cflags --libs atspi-2 gio-2.0 gobject-2.0 glib-2.0)
  for kind in password agent route; do sh "$0" "$kind"; done
  exit 0
fi
export PULSE_SSH_MULTIWINDOW=1 PULSE_COLD_SSH_AGENT=0 PULSE_COLD_SSH_ROUTE=0 PULSE_SAVED_ALIAS=0 PULSE_COLD_CLOSE_CYCLES=0
case "$mode" in agent) export PULSE_COLD_SSH_AGENT=1;; route) export PULSE_COLD_SSH_ROUTE=1;; esac
timeout --kill-after=5s 170s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- sh scripts/probe-window-x11.sh "target/probes/cli-multiwindow-ssh-$mode-wm.log" python3 scripts/probe-workspace-cold-ssh.py
