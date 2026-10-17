#!/bin/sh
set -eu
# All fixtures own private seed directories. The Python harness reaps the GUI, peers,
# native children and endpoints even when a case fails; the caller owns TMPDIR.
for ALIAS_MODE in password agent route; do
    ALIAS_AGENT=0
    ALIAS_ROUTE=0
    case "$ALIAS_MODE" in
        agent) ALIAS_AGENT=1 ;;
        route) ALIAS_ROUTE=1 ;;
    esac
    for ALIAS_WARM in 0 1; do
        ALIAS_STAGE=cold
        if [ "$ALIAS_WARM" = 1 ]; then ALIAS_STAGE=warm; fi
        PULSE_SAVED_ALIAS=1 PULSE_SAVED_ALIAS_CASE= PULSE_SAVED_ALIAS_WARM="$ALIAS_WARM" \
        PULSE_COLD_SSH_AGENT="$ALIAS_AGENT" PULSE_COLD_SSH_ROUTE="$ALIAS_ROUTE" \
        timeout --kill-after=5s 110s xvfb-run -a -s '-screen 0 1280x800x24' \
        dbus-run-session -- sh scripts/probe-window-x11.sh "target/probes/saved-alias-$ALIAS_MODE-$ALIAS_STAGE-wm.log" \
        python3 scripts/probe-workspace-cold-ssh.py >"target/probes/saved-alias-$ALIAS_MODE-$ALIAS_STAGE-window.log" 2>&1
        tail -n 1 "target/probes/saved-alias-$ALIAS_MODE-$ALIAS_STAGE-window.log"
    done
done
for ALIAS_CASE in ambiguous missing-default; do
    PULSE_SAVED_ALIAS=1 PULSE_SAVED_ALIAS_CASE="$ALIAS_CASE" PULSE_SAVED_ALIAS_WARM=0 \
    PULSE_COLD_SSH_AGENT=1 PULSE_COLD_SSH_ROUTE=0 \
    timeout --kill-after=5s 110s xvfb-run -a -s '-screen 0 1280x800x24' \
    dbus-run-session -- sh scripts/probe-window-x11.sh "target/probes/saved-alias-$ALIAS_CASE-wm.log" \
    python3 scripts/probe-workspace-cold-ssh.py >"target/probes/saved-alias-$ALIAS_CASE-window.log" 2>&1
    tail -n 1 "target/probes/saved-alias-$ALIAS_CASE-window.log"
done
