#!/bin/sh
# Every scenario starts with no GUI and uses the production CLI's cold handoff.
set -eu
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/cold-split-live.XXXXXX)
cleanup() { case "$fixture" in /data/temp/cold-split-live.*) rm -rf -- "$fixture";; *) exit 1;; esac; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
for scenario in cancel partial disconnect refresh approve; do
 timeout --kill-after=5s 70s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- sh scripts/probe-window-x11.sh "target/probes/cli-cold-split-row-$scenario-wm.log" python3 scripts/probe-workspace-cold-focus.py "split-row-$scenario"
done
timeout --kill-after=5s 70s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- sh scripts/probe-window-x11.sh target/probes/cli-cold-split-column-approve-wm.log python3 scripts/probe-workspace-cold-focus.py split-column-approve

# Independent tags preserve explicit-Profile results and earlier failures.
for spec in unset:split-row-approve saved:split-row-approve saved:chain-column-approve; do
 default_kind=${spec%%:*}; action=${spec#*:}
 default_run="default-$(basename "$fixture")-$default_kind-$action"
 PULSE_CLI_PROBE_SPLIT_DEFAULT="$default_kind" PULSE_CLI_PROBE_RUN="$default_run" timeout --kill-after=5s 90s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- sh scripts/probe-window-x11.sh "target/probes/$default_run-wm.log" python3 scripts/probe-workspace-cold-focus.py "$action"
done
