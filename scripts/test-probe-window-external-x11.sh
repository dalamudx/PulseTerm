#!/bin/sh
# Deterministic control-process fixtures; this is not real X11/WM acceptance.
set -eu
umask 077
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/pulse-external-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir "$work/bin"
cat > "$work/app" <<'APP'
#!/bin/sh
set -eu
for request in minimize:1 restore:1 minimize:2 restore:2; do
  printf '%s\n' "$request" > "$FIXTURE/current"
  printf '%s\n' "$request" > "$PULSE_PROBE_WINDOW_CONTROL/request.tmp"
  mv "$PULSE_PROBE_WINDOW_CONTROL/request.tmp" "$PULSE_PROBE_WINDOW_CONTROL/request"
  n=0
  while [ ! -f "$FIXTURE/ack.$request" ]; do
    n=$((n+1)); [ "$n" -lt 300 ] || exit 96
    sleep 0.01
  done
done
APP
cat > "$work/bin/xdotool" <<'XDO'
#!/bin/sh
set -eu
operation=$1; shift
case "$operation" in
  search)
    [ "$#" -eq 6 ] && [ "$1" = --onlyvisible ] && [ "$2" = --all ] && [ "$3" = --pid ] && [ "$5" = --name ] && [ "$6" = '^PulseTerm · IPC validation$' ] || exit 92
    printf '%s\n' "$4" > "$FIXTURE/pid"
    case "$MODE" in
      ambiguous) printf '123\n124\n';;
      missing) :;;
      replaced) if [ "$(cat "$FIXTURE/current")" = minimize:2 ]; then echo 124; else echo 123; fi;;
      *) echo 123;;
    esac
    ;;
  getwindowpid) if [ "$MODE" = foreign ]; then echo 1; else cat "$FIXTURE/pid"; fi;;
  windowminimize|windowactivate)
    request=$(cat "$FIXTURE/current")
    case "$operation:$request" in
      windowminimize:minimize:*|windowactivate:restore:*) :;;
      *) exit 95;;
    esac
    touch "$FIXTURE/ack.$request"
    ;;
  *) exit 93;;
esac
XDO
chmod 700 "$work/bin/xdotool"
for mode in normal ambiguous missing foreign replaced; do
  mkdir "$work/$mode"
  code=0
  PATH="$work/bin:$PATH" FIXTURE="$work/$mode" MODE="$mode" timeout 5s sh "$root/scripts/probe-window-external-x11.sh" "$work/$mode/actions" sh "$work/app" > "$work/$mode/output" 2>&1 || code=$?
  case "$mode" in
    normal)
      [ "$code" -eq 0 ] || { cat "$work/$mode/output"; exit 1; }
      printf 'minimize:1\nrestore:1\nminimize:2\nrestore:2\n' > "$work/expected"
      cmp "$work/expected" "$work/$mode/actions"
      ;;
    replaced)
      [ "$code" -eq 1 ] && [ "$(wc -l < "$work/$mode/actions")" -eq 2 ]
      grep -q '^probe.external_replaced_window$' "$work/$mode/output"
      ;;
    *) [ "$code" -eq 1 ] && [ ! -s "$work/$mode/actions" ];;
  esac
done
printf '%s\n' 'PASS external-controller: exact title + child PID, ambiguity/missing/foreign/replacement denied'
