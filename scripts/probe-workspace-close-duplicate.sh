#!/bin/sh
# One fixture PTY, command-palette duplication and confirmation-free close.
set -eu
umask 077
mkdir -p target/probes
fixture=$(mktemp -d "${TMPDIR:-/data/temp}/pt-workspace-close-dup.XXXXXX")
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$fixture/bin" "$fixture/children"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
# Fixed font baseline and an explicitly saved confirmation-off preference.
printf '[terminal]\nfont_size = 13\nconfirm_close = false\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
: "${PULSETERM_TAB_FIXTURE:?missing fixture}"
printf '%s\n' "$$" > "$PULSETERM_TAB_FIXTURE/$$.pid"
printf 'Tab fixture pid=%s\r\n' "$$"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$PULSETERM_TAB_FIXTURE/$$.input"
  printf 'echo:%s\r\n' "$line"
done
SHELL
chmod 700 "$fixture/bin/sh"
log=target/probes/workspace-close-duplicate-native.log
printf 'GDK_BACKEND=%s\nLIBGL_ALWAYS_SOFTWARE=%s\nWEBKIT_DISABLE_DMABUF_RENDERER=%s\n' \
  "${GDK_BACKEND:-unset}" "${LIBGL_ALWAYS_SOFTWARE:-unset}" "${WEBKIT_DISABLE_DMABUF_RENDERER:-unset}" \
  > target/probes/workspace-close-duplicate-environment.log
PULSETERM_TAB_FIXTURE="$fixture/children" PATH="$fixture/bin" SHELL="$fixture/bin/sh" "${PULSE_WORKSPACE_BINARY:-${CARGO_TARGET_DIR:-target}/debug/pulseterm-workspace}" > "$log" 2>&1 &
app=$!
cleanup(){
  status=$?
  if [ "$status" -ne 0 ]; then
    tail -40 "$log" >&2
    timeout 3s import -window root target/probes/workspace-close-duplicate-failure.png || true
  fi
  kill "$app" 2>/dev/null || true
  wait "$app" 2>/dev/null || true
  rm -rf "$fixture"
}
trap cleanup EXIT
count(){ grep -c "^workspace.$1$" "$log" || true; }
await_count(){ step=$1; expected=$2; attempt=0; while [ "$(count "$step")" -lt "$expected" ]; do kill -0 "$app" 2>/dev/null || { echo workspace.close_dup_early_exit >&2; exit 1; }; if grep -q '^workspace.terminal_failure.' "$log"; then echo workspace.close_dup_surface_failed >&2; exit 1; fi; [ "$attempt" -lt 80 ] || { echo "workspace.close_dup_timeout.$step" >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done; }
await_count sessions_listed 1
xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
[ "$(printf '%s\n' "$xid" | awk 'NF {n++} END {print n+0}')" -eq 1 ] || { echo workspace.close_dup_ambiguous_window >&2; exit 1; }
[ "$(xdotool getwindowpid "$xid")" = "$app" ]
timeout 2s xdotool windowactivate --sync "$xid"
paint=0
while :; do
  timeout 3s import -window "$xid" target/probes/workspace-close-duplicate-initial.png
  colors=$(convert target/probes/workspace-close-duplicate-initial.png -format '%k' info:)
  [ "$colors" -gt 10 ] && break
  [ "$paint" -lt 10 ] || { echo workspace.close_dup_initial_paint_timeout >&2; exit 1; }
  paint=$((paint+1)); sleep 0.1
done
timeout 2s xdotool key --clearmodifiers Tab Return
await_count terminal_input_ready 1
[ "$(count profile_selected.sh)" -eq 1 ] || { echo workspace.close_dup_profile_receipt >&2; exit 1; }
set -- "$fixture/children/"*.pid
[ "$#" -eq 1 ] || { echo workspace.close_dup_first_pid_missing >&2; exit 1; }
a=$(cat "$1")
sleep 0.3
#
timeout 2s xdotool type --clearmodifiers --delay 1 'DUP_A'
timeout 2s xdotool key --clearmodifiers Return
attempt=0
while [ ! -f "$fixture/children/$a.input" ] || [ "$(wc -l < "$fixture/children/$a.input")" -lt 1 ]; do
  [ "$attempt" -lt 80 ] || { echo workspace.close_dup_first_input_timeout >&2; exit 1; }
  attempt=$((attempt+1)); sleep 0.1
done
# Palette duplication: the SAME recorded Profile source, a distinct PTY.
timeout 2s xdotool key --clearmodifiers ctrl+shift+p
sleep 0.4
timeout 2s xdotool type --clearmodifiers --delay 20 'duplicate'
sleep 0.4
timeout 2s xdotool key --clearmodifiers Return
await_count session_opened 2
await_count terminal_input_ready 2
set -- "$fixture/children/"*.pid
[ "$#" -eq 2 ] || { echo workspace.close_dup_second_pid_missing >&2; exit 1; }
b=""
for path in "$fixture/children/"*.pid; do candidate=$(cat "$path"); if [ "$candidate" != "$a" ]; then b=$candidate; fi; done
[ -n "$b" ]; kill -0 "$a"; kill -0 "$b"
timeout 3s import -window "$xid" target/probes/workspace-close-duplicate-two.png
# Saved confirm_close=false: close acts immediately, with no dialog keystroke.
timeout 2s xdotool mousemove --window "$xid" 900 80 click 1
await_count session_closed 1
! kill -0 "$b" 2>/dev/null || { echo workspace.close_dup_closed_pid_alive >&2; exit 1; }
kill -0 "$a"
timeout 3s import -window "$xid" target/probes/workspace-close-duplicate-survivor.png
timeout 2s xdotool windowactivate --sync "$xid"
timeout 2s xdotool key --clearmodifiers alt+F4
await_count exited 1
wait "$app"
[ "$(count shutdown_complete)" -eq 1 ]
! kill -0 "$a" 2>/dev/null || { echo workspace.close_dup_shutdown_pid_alive >&2; exit 1; }
echo workspace.close_duplicate_and_confirmation_free_close
