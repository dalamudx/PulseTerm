#!/bin/sh
set -eu
# Diagnostic only: repeated explicit cancellation, no target or peer approval.
# Caller owns TMPDIR; the existing subreaper always reclaims fixture children.
ulimit -c 0
NATIVE_CLOSE_KIND=normal
if [ "${PULSE_COLD_CLOSE_GDB:-0}" = 1 ]; then NATIVE_CLOSE_KIND=gdb; fi
for NATIVE_CLOSE_STATE in locked unlocked; do
    PULSE_SAVED_ALIAS=1 PULSE_SAVED_ALIAS_CASE= PULSE_SAVED_ALIAS_WARM=0 \
    PULSE_COLD_SSH_ROUTE=1 PULSE_COLD_SSH_AGENT=1 \
    PULSE_COLD_CLOSE_STATE="$NATIVE_CLOSE_STATE" PULSE_COLD_CLOSE_CYCLES="${PULSE_COLD_CLOSE_CYCLES:-12}" \
    timeout --kill-after=5s 240s xvfb-run -a -s '-screen 0 1280x800x24' \
    dbus-run-session -- sh scripts/probe-window-x11.sh "target/probes/native-close-$NATIVE_CLOSE_STATE-$NATIVE_CLOSE_KIND-wm.log" \
    python3 scripts/probe-workspace-cold-ssh.py >"target/probes/native-close-$NATIVE_CLOSE_STATE-$NATIVE_CLOSE_KIND-window.log" 2>&1
    tail -n 2 "target/probes/native-close-$NATIVE_CLOSE_STATE-$NATIVE_CLOSE_KIND-window.log"
done
