#!/bin/sh
# Test-only external WM controller inside the private Xvfb/Openbox process group.
# Window selection is by this direct child's PID; never accept an arbitrary XID.
set -eu
umask 077
log=$1
shift
control=$(mktemp -d "${TMPDIR:-/tmp}/pulseterm-window-control.XXXXXX")
app=
cleanup() {
  if [ -n "$app" ]; then kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true; fi
  rm -f "$control/request" "$control/request.tmp"
  rmdir "$control" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
: > "$log"
PULSE_PROBE_WINDOW_CONTROL="$control" "$@" &
app=$!
last=
xid=
cycle=
attempt=0
while kill -0 "$app" 2>/dev/null; do
  request=$(cat "$control/request" 2>/dev/null || true)
  if [ -n "$request" ] && [ "$request" != "$last" ]; then
    action=${request%%:*}
    generation=${request#*:}
    case "$generation" in ''|*[!0-9]*) echo 'probe.external_invalid_generation' >&2; exit 1;; esac
    case "$action" in
      minimize)
        [ "$generation" != "$cycle" ] || { echo 'probe.external_duplicate_cycle' >&2; exit 1; }
        ids=$(xdotool search --onlyvisible --pid "$app" 2>/dev/null || true)
        count=$(printf '%s\n' "$ids" | awk 'NF {n++} END {print n+0}')
        [ "$count" -eq 1 ] || { echo 'probe.external_ambiguous_window' >&2; exit 1; }
        xid=$ids
        [ "$(xdotool getwindowpid "$xid")" = "$app" ] || exit 1
        timeout 2s xdotool windowminimize "$xid"
        cycle=$generation
        ;;
      restore)
        [ "$cycle" = "$generation" ] && [ -n "$xid" ] || { echo 'probe.external_unrequested_restore' >&2; exit 1; }
        [ "$(xdotool getwindowpid "$xid")" = "$app" ] || exit 1
        timeout 2s xdotool windowactivate --sync "$xid"
        ;;
      *) echo 'probe.external_invalid_action' >&2; exit 1;;
    esac
    printf '%s\n' "$request" >> "$log"
    last=$request
  fi
  attempt=$((attempt + 1))
  [ "$attempt" -lt 3500 ] || { echo 'probe.external_deadline' >&2; exit 1; }
  sleep 0.05
done
code=0
wait "$app" || code=$?
app=
exit "$code"
