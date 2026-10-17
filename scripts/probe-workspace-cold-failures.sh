#!/bin/sh
# Trace only process/connect metadata; no tokens, payloads or secrets.
set -eu
command -v strace >/dev/null
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-cf.XXXXXX)
cleanup() { case "$fixture" in /data/temp/pt-cf.*) rm -rf "$fixture";; *) exit 1;; esac; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
umask 022
mode=${1:-failures}
[ "$#" -le 1 ]; case "$mode" in failures|unbound) ;; *) exit 2;; esac
timeout --kill-after=5s 100s xvfb-run -a -s '-screen 0 1280x800x24' python3 scripts/probe-workspace-cold-failures.py "$mode"
