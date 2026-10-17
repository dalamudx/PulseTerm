#!/bin/sh
# Seven isolated production CLI chains; every creation still needs its own click.
set -eu
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/cli-chain-live.XXXXXX)
cleanup() { case "$fixture" in /data/temp/cli-chain-live.*) rm -rf -- "$fixture";; *) exit 1;; esac; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
for mode in chain-syntax chain-row-cancel chain-row-partial chain-row-disconnect chain-row-refresh chain-row-approve chain-column-approve; do
 timeout --kill-after=5s 80s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- sh scripts/probe-window-x11.sh "target/probes/cli-cold-$mode-wm.log" python3 scripts/probe-workspace-cold-focus.py "$mode"
done
