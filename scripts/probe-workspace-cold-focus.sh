#!/bin/sh
# Exact production ELF in an isolated install-like layout; no shell/session startup.
set -eu
[ "$#" -le 1 ]
action=${1:-focus}
case "$action" in focus|profiles) ;; *) exit 2;; esac
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-cold.XXXXXX)
cleanup() {
 case "$fixture" in /data/temp/pt-cold.*) rm -rf "$fixture";; *) exit 1;; esac
}
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
# Cover ordinary user umask: WebKit must not create public app-data first.
umask 022
timeout --kill-after=5s 55s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- \
 sh scripts/probe-window-x11.sh "target/probes/cli-cold-$action-wm.log" \
 python3 scripts/probe-workspace-cold-focus.py "$action"
