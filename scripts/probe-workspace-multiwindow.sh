#!/bin/sh
# Same-process native windows; each creation still needs its original page's click.
set -eu
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/cli-multiwindow-live.XXXXXX)
cleanup() { case "$fixture" in /data/temp/cli-multiwindow-live.*) rm -rf -- "$fixture";; *) exit 1;; esac; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
timeout --kill-after=5s 170s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- sh scripts/probe-window-x11.sh target/probes/cli-multiwindow-wm.log python3 scripts/probe-workspace-cold-focus.py multiwindow
