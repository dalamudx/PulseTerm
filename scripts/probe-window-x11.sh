#!/bin/sh
# Only used inside a NEW Xvfb display owned by the probe timeout process group.
# Never replace the developer's running window manager. No terminal/user input.
set -eu
log=$1
shift
mkdir -p "$(dirname "$log")"
openbox --sm-disable >"$log" 2>&1 &
wm=$!
cleanup() {
  kill "$wm" 2>/dev/null || true
  wait "$wm" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
attempt=0
while ! xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -Eq 'window id # 0x[1-9a-fA-F][0-9a-fA-F]*'; do
  if ! kill -0 "$wm" 2>/dev/null; then
    echo 'probe.window_manager_failed' >&2
    exit 1
  fi
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 50 ]; then
    echo 'probe.window_manager_ready_timeout' >&2
    exit 1
  fi
  sleep 0.1
done
"$@"
