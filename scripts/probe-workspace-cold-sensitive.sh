#!/bin/sh
# Two creation clients cannot share the primary's approval or startup credit.
set -eu
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-cs.XXXXXX)
cleanup() { case "$fixture" in /data/temp/pt-cs.*) rm -rf "$fixture";; *) exit 1;; esac; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
umask 022
for scenario in cancel disconnect refresh approve; do
 timeout --kill-after=5s 55s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- \
  sh scripts/probe-window-x11.sh "target/probes/cli-cold-sensitive-$scenario-wm.log" \
  python3 scripts/probe-workspace-cold-focus.py "sensitive-$scenario"
done
