#!/bin/sh
# Concurrent independent production clients; never synthesize requests or retries.
set -eu
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-cc.XXXXXX)
cleanup() { case "$fixture" in /data/temp/pt-cc.*) rm -rf "$fixture";; *) exit 1;; esac; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
umask 022
for pair in ff pp fp; do
 timeout --kill-after=5s 45s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- \
  sh scripts/probe-window-x11.sh "target/probes/cli-cold-concurrent-$pair-wm.log" \
  python3 scripts/probe-workspace-cold-focus.py "concurrent-$pair"
done
