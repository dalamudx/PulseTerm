#!/bin/sh
# GTK/D-Bus compatibility regression, not production WebView acceptance.
set -eu
umask 077
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "${1:-}" != --session ]; then
  work=$(mktemp -d /data/temp/pulse-status-cache.XXXXXX)
  trap 'rm -rf -- "$work"' EXIT
  export TMPDIR="$work" TMP="$work" TEMP="$work"
  cc -Wall -Wextra -Werror "$root/scripts/test-probe-x11-status.c" -o "$work/test-status" $(pkg-config --cflags --libs gtk+-3.0 atspi-2 gio-2.0)
  timeout --kill-after=2s 20s xvfb-run -a dbus-run-session -- sh "$0" --session "$work"
  exit
fi
work=$2
export GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=0 GTK_A11Y=1
"$work/test-status" --fixture > "$work/fixture.log" 2>&1 &
fixture_pid=$!
trap 'kill "$fixture_pid" 2>/dev/null || true; wait "$fixture_pid" 2>/dev/null || true' EXIT
# Wait only for the fixture to expose its frame, without querying the stale status.
ready=0
for attempt in $(seq 1 30); do
  if "$work/test-status" "$fixture_pid" 'PulseTerm status cache fixture' > /dev/null 2>&1; then ready=1; break; fi
  sleep 0.1
done
[ "$ready" -eq 1 ]
expect() {
  expected=$1; shift
  code=0
  "$@" > "$work/out" 2> "$work/err" || code=$?
  [ "$code" -eq "$expected" ] || { cat "$work/err"; exit 1; }
  ! grep -E 'Public refusal sentinel|DO_NOT_LOG_SECRET_CONTENT' "$work/out" "$work/err"
}
expect 1 env PULSE_TEST_STALE_CACHE=1 "$work/test-status" "$fixture_pid" 'Public refusal sentinel'
expect 0 "$work/test-status" "$fixture_pid" 'Public refusal sentinel'
expect 0 "$work/test-status" "$fixture_pid" 'Public refusal sentinel' inspect-tree
grep -q 'status_cache_refreshed=1' "$work/err"
grep -q 'text_match=1' "$work/err"
grep -q 'showing=1' "$work/out"
expect 1 "$work/test-status" "$fixture_pid" 'Public refusal'
expect 1 "$work/test-status" 2147483647 'Public refusal sentinel'
printf '%s\n' 'PASS stale cache rejected; refreshed exact text found; partial text and foreign PID rejected; no contents emitted'
