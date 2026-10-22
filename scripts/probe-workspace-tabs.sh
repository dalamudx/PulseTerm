#!/bin/sh
# Two backend-discovered Profile PTYs, independent X11 actions. No user HOME.
set -eu
umask 077
mode=${1:-tabs}
case "$mode" in tabs|split|split-adjust|labels|profiles) ;; *) exit 2;; esac
mkdir -p target/probes
mkdir -p /data/temp
fixture=$(mktemp -d /data/temp/pt-workspace-tabs.XXXXXX)
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$fixture/bin" "$fixture/children"
# Explicit controlled font baseline; production compiled default remains 14px.
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
: "${PULSETERM_TAB_FIXTURE:?missing fixture}"
printf '%s\n' "$$" > "$PULSETERM_TAB_FIXTURE/$$.pid"
printf 'Tab fixture pid=%s\r\n' "$$"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$PULSETERM_TAB_FIXTURE/$$.input"
  /bin/stty size >> "$PULSETERM_TAB_FIXTURE/$$.size"
  printf 'echo:%s\r\n' "$line"
done
SHELL
chmod 700 "$fixture/bin/sh"
log=target/probes/workspace-tabs-native.log
if [ "$mode" = split ]; then log=target/probes/workspace-split-native.log; fi
if [ "$mode" = split-adjust ]; then log=target/probes/workspace-split-adjust-native.log; fi
if [ "$mode" = labels ]; then log=target/probes/workspace-labels-native.log; fi
if [ "$mode" = profiles ]; then log=target/probes/workspace-profiles-native.log; fi
PULSETERM_TAB_FIXTURE="$fixture/children" PATH="$fixture/bin" SHELL="$fixture/bin/sh" "${PULSE_WORKSPACE_BINARY:-${CARGO_TARGET_DIR:-target}/debug/pulseterm-workspace}" > "$log" 2>&1 &
app=$!
cleanup(){ kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true; rm -rf "$fixture"; }
trap cleanup EXIT
count(){ grep -c "^workspace.$1$" "$log" || true; }
await_count(){ step=$1; expected=$2; attempt=0; while [ "$(count "$step")" -lt "$expected" ]; do kill -0 "$app" 2>/dev/null || { echo workspace.tabs_early_exit >&2; exit 1; }; if grep -q '^workspace.terminal_failure.' "$log"; then echo workspace.tabs_surface_failed >&2; exit 1; fi; [ "$attempt" -lt 80 ] || { echo "workspace.tabs_timeout.$step" >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done; }
await_count sessions_listed 1
xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
[ "$(printf '%s\n' "$xid" | awk 'NF {n++} END {print n+0}')" -eq 1 ] || { echo workspace.tabs_ambiguous_window >&2; exit 1; }
[ "$(xdotool getwindowpid "$xid")" = "$app" ]
timeout 2s xdotool windowactivate --sync "$xid"
# Sidebar content can scroll as controls change; only activate an actually showing control.
sidebar_action(){
  label=$1; action=${2:-activate}; started=$(date +%s)
  timeout 2s xdotool mousemove --window "$xid" 90 420 click --repeat 16 --delay 10 4
  while ! timeout 1s target/probes/x11-status "$app" "$label" "$action"; do
    [ "$(( $(date +%s) - started ))" -lt 8 ] || {
      timeout 3s import -window "$xid" "target/probes/workspace-$mode-control-failure.png"
      echo "workspace.tabs_control_timeout.$mode.$action" >&2; return 1;
    }
    timeout 2s xdotool mousemove --window "$xid" 90 420 click --repeat 2 --delay 25 5
  done
}
# A native sessions-list receipt can precede the first compositor paint.
# Observe actual pixels under a one-second visibility gate, not a guessed delay.
paint=0
while :; do
  timeout 3s import -window "$xid" target/probes/workspace-tabs-initial.png
  colors=$(convert target/probes/workspace-tabs-initial.png -format '%k' info:)
  [ "$colors" -gt 10 ] && break
  [ "$paint" -lt 10 ] || { echo workspace.tabs_initial_paint_timeout >&2; exit 1; }
  paint=$((paint+1)); sleep 0.1
done
# Current painted catalog: focused Bash, next POSIX sh (assert selection receipt).
timeout 2s xdotool key --clearmodifiers Tab Return
await_count terminal_input_ready 1
[ "$(count profile_selected.sh)" -eq 1 ]
set -- "$fixture/children/"*.pid
[ "$#" -eq 1 ] && [ -f "$1" ] || { echo workspace.tabs_first_pid_missing >&2; exit 1; }
a=$(cat "$1")
input(){ pid=$1; text=$2; expected=$3; timeout 2s xdotool type --clearmodifiers --delay 1 "$text"; timeout 2s xdotool key --clearmodifiers Return; attempt=0; while [ ! -f "$fixture/children/$pid.input" ] || [ "$(wc -l < "$fixture/children/$pid.input")" -lt "$expected" ]; do [ "$attempt" -lt 80 ] || { echo "workspace.tabs_input_timeout.$mode.$expected" >&2;timeout 3s import -window "$xid" "target/probes/workspace-$mode-input-failure-$expected.png";exit 1; }; attempt=$((attempt+1)); sleep 0.1; done; [ "$(wc -l < "$fixture/children/$pid.input")" -eq "$expected" ]; [ "$(tail -n 1 "$fixture/children/$pid.input")" = "$text" ]; }
# Shell input receipt is not rendered echo. Verify the fixed ASCII pixels too.
# Full fixed ASCII echo is checked through exact pixels; no terminal content is interpreted as commands.
drawn(){
  text=$1; image=$2; start=$(date +%s)
  while :; do
    timeout 3s import -window "$xid" "$image"
    # Fixed 13px monospace fixture: 8px cells, 15px rows. Row 6 is the
    # independently verified canonical tty echo, row 7 is the app echo.
    # Compare every character after echo:, and reject an all-blank reference.
    width=$(( ${#text} * 8 ))
    line=${3:-3}; x=${4:-200}; base_y=${5:-149}; y=$((base_y+(line*2-1)*15)); echo_y=$((y+15)); echo_x=$((x+40))
    convert "$image" -crop "${width}x15+${x}+${y}" +repage target/probes/workspace-tabs-typed.png
    convert "$image" -crop "${width}x15+${echo_x}+${echo_y}" +repage target/probes/workspace-tabs-echo.png
    colors=$(convert target/probes/workspace-tabs-typed.png -format '%k' info:)
    if [ "$colors" -gt 10 ] && compare -metric AE target/probes/workspace-tabs-typed.png target/probes/workspace-tabs-echo.png null: 2>/dev/null; then return; fi
    [ "$(( $(date +%s) - start ))" -lt 8 ] || { echo workspace.tabs_drawn_echo_timeout >&2; exit 1; }
    sleep 0.1
  done
}
input "$a" TAB_A_FIRST 1
# Same visible Sh Profile, never a generic program/argument command.
timeout 2s xdotool mousemove --window "$xid" 80 164 click 1
await_count terminal_input_ready 2
[ "$(count profile_selected.sh)" -eq 2 ]
set -- "$fixture/children/"*.pid
[ "$#" -eq 2 ] || { echo workspace.tabs_second_pid_missing >&2; exit 1; }
b=""
for path in "$fixture/children/"*.pid; do candidate=$(cat "$path"); if [ "$candidate" != "$a" ]; then b=$candidate; fi; done
[ -n "$b" ]; kill -0 "$a"; kill -0 "$b"
input "$b" TAB_B_FIRST 1
if [ "$mode" = profiles ]; then
  timeout 2s xdotool mousemove --window "$xid" 144 74 click 1
  # The default selector changes Tab order; address the visible checkbox by name.
  timeout 2s target/probes/x11-status "$app" "显示 POSIX sh" activate
  await_count profile_visibility_saved 1
  await_count profiles_listed 2
  timeout 2s xdotool key --clearmodifiers Escape
  kill -0 "$a"; kill -0 "$b"
  [ "$(count session_opened)" -eq 2 ]
  timeout 2s xdotool key --clearmodifiers ctrl+shift+r
  await_count document_confirmed 2
  await_count terminal_input_ready 3
  input "$b" PROFILE_B_STILL_LIVE 2
  [ "$(wc -l < "$fixture/children/$a.input")" -eq 1 ]
  sleep 0.2
  timeout 3s import -window "$xid" target/probes/workspace-profiles-hidden.png
  [ "$(count session_opened)" -eq 2 ]; kill -0 "$a"; kill -0 "$b"
  timeout 2s xdotool key --clearmodifiers alt+F4
  await_count exited 1
  wait "$app"
  ! kill -0 "$a" 2>/dev/null; ! kill -0 "$b" 2>/dev/null
  # Same backend data directory but a new application process. No shell auto-start.
  log=target/probes/workspace-profiles-reopen-native.log
  PULSETERM_TAB_FIXTURE="$fixture/children" PATH="$fixture/bin" SHELL="$fixture/bin/sh" target/debug/pulseterm-workspace > "$log" 2>&1 & app=$!
  await_count sessions_listed 1
  xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
  [ "$(printf '%s\n' "$xid" | awk 'NF {n++} END {print n+0}')" -eq 1 ]; [ "$(xdotool getwindowpid "$xid")" = "$app" ]
  timeout 2s xdotool windowactivate --sync "$xid"
  timeout 2s xdotool mousemove --window "$xid" 144 74 click 1
  timeout 3s import -window "$xid" target/probes/workspace-profiles-reopen.png
  timeout 2s target/probes/x11-status "$app" "显示 POSIX sh" activate
  await_count profile_visibility_saved 1
  await_count profiles_listed 2
  timeout 2s xdotool key --clearmodifiers Escape
  [ "$(count session_opened)" -eq 0 ]
  sleep 0.2
  timeout 3s import -window "$xid" target/probes/workspace-profiles-restored.png
  timeout 2s xdotool key --clearmodifiers alt+F4
  await_count exited 1
  wait "$app"
  echo workspace.profile_hide_refresh_process_reopen_restore_confirmed
  exit 0
fi
if [ "$mode" = labels ]; then
  # Address the header control by accessible name: header button coordinates drift.
  timeout 2s target/probes/x11-status "$app" "标签名称" activate
  timeout 2s xdotool type --clearmodifiers --delay 1 BUILD_TWO
  timeout 2s xdotool key --clearmodifiers Tab Home Down Tab Return
  sleep 0.2
  timeout 3s import -window "$xid" target/probes/workspace-labels-saved.png
  [ "$(convert target/probes/workspace-labels-saved.png -format '%[pixel:p{284,81}]' info:)" = 'srgb(0,162,199)' ] || { echo workspace.label_color_missing >&2;exit 1; }
  [ "$(wc -l < "$fixture/children/$b.input")" -eq 1 ]; [ "$(wc -l < "$fixture/children/$a.input")" -eq 1 ]
  timeout 2s xdotool key --clearmodifiers ctrl+shift+r
  await_count document_confirmed 2
  await_count terminal_input_ready 3
  sleep 0.2
  timeout 3s import -window "$xid" target/probes/workspace-labels-refresh.png
  [ "$(convert target/probes/workspace-labels-refresh.png -format '%[pixel:p{284,81}]' info:)" = 'srgb(0,162,199)' ] || { echo workspace.label_color_not_restored >&2;exit 1; }
  [ "$(count session_opened)" -eq 2 ]; kill -0 "$a"; kill -0 "$b"
  input "$b" LABEL_B_AFTER_REFRESH 2
  [ "$(wc -l < "$fixture/children/$a.input")" -eq 1 ]
  timeout 2s xdotool key --clearmodifiers alt+F4
  await_count exited 1
  wait "$app"
  ! kill -0 "$a" 2>/dev/null; ! kill -0 "$b" 2>/dev/null
  echo workspace.label_editor_no_input_leak_refresh_confirmed
  exit 0
fi
if [ "$mode" = split-adjust ]; then
  sidebar_action "左右分屏"
  await_count terminal_input_ready 4
  timeout 2s xdotool mousemove --window "$xid" 300 350 click 1
  timeout 2s xdotool key --clearmodifiers ctrl+shift+Right
  input "$a" FOCUS_A 2
  timeout 2s xdotool key --clearmodifiers ctrl+shift+Left
  input "$b" FOCUS_B 2
  [ "$(count terminal_input_ready)" -eq 4 ]
  timeout 3s import -window "$xid" target/probes/workspace-split-adjust-before.png
  sidebar_action "第一个窗格比例" focus
  timeout 2s xdotool key --clearmodifiers Right
  await_count terminal_input_ready 6
  # Resize readiness is not focus permission; leave the ratio control explicitly.
  timeout 2s xdotool mousemove --window "$xid" 300 350 click 1
  input "$b" RATIO_B 3
  timeout 2s xdotool key --clearmodifiers ctrl+shift+Right
  input "$a" RATIO_A 3
  bcols=$(tail -1 "$fixture/children/$b.size" | awk '{print $2}')
  acols=$(tail -1 "$fixture/children/$a.size" | awk '{print $2}')
  [ "$bcols" -gt "$acols" ] || { echo workspace.split_ratio_native_mismatch >&2; exit 1; }
  [ "$(tail -1 "$fixture/children/$b.size" | awk '{print $1}')" -eq 31 ]
  [ "$(tail -1 "$fixture/children/$a.size" | awk '{print $1}')" -eq 31 ]
  kill -0 "$a"; kill -0 "$b"; [ "$(count session_opened)" -eq 2 ]
  drawn RATIO_B target/probes/workspace-split-adjust-ratio.png 3 200 149
  sidebar_action "上下分屏"
  await_count terminal_input_ready 8
  timeout 2s xdotool mousemove --window "$xid" 350 220 click 1
  timeout 2s xdotool key --clearmodifiers ctrl+shift+Down
  input "$a" COLUMN_NAV_A 4
  timeout 2s xdotool key --clearmodifiers ctrl+shift+Up
  input "$b" COLUMN_NAV_B 4
  [ "$(count terminal_input_ready)" -eq 8 ]
  drawn COLUMN_NAV_B target/probes/workspace-split-adjust-column.png 4 200 149
  # The 55% ratio persists across direction changes; locate the lower viewport independently.
  brows=$(tail -1 "$fixture/children/$b.size" | awk '{print $1}')
  arows=$(tail -1 "$fixture/children/$a.size" | awk '{print $1}')
  [ "$brows" -gt "$arows" ] || { echo workspace.split_column_ratio_lost >&2; exit 1; }
  [ "$(tail -1 "$fixture/children/$a.size" | awk '{print $2}')" -eq 92 ]
  [ "$(tail -1 "$fixture/children/$b.size" | awk '{print $2}')" -eq 92 ]
  timeout 3s import -window "$xid" target/probes/workspace-split-adjust-column.png
  lower_y=$(convert target/probes/workspace-split-adjust-column.png -crop 1x480+939+140 +repage txt:- |
    awk -F ': ' '/^[0-9]/ { split($1,p,","); black=($2 ~ /#000000 /); if (black && !last) {runs++; if (runs==2) print p[2]+140;} last=black } END {if(runs!=2) exit 1}')
  [ "$lower_y" -gt 350 ] && [ "$lower_y" -lt 450 ]
  drawn COLUMN_NAV_A target/probes/workspace-split-adjust-column.png 4 200 "$lower_y"
  timeout 2s xdotool key --clearmodifiers alt+F4
  await_count exited 1; wait "$app"
  ! kill -0 "$a" 2>/dev/null; ! kill -0 "$b" 2>/dev/null
  for pid in "$a" "$b"; do
    cp -p -- "$fixture/children/$pid.input" "target/probes/workspace-$mode-input-$pid.txt"
    cp -p -- "$fixture/children/$pid.size" "target/probes/workspace-$mode-size-$pid.txt"
  done
  printf '{"version":1,"mode":"%s","status":"passed","gui_exit_code":0,"sessions_opened":2,"native_reaped":true,"pids":[%s,%s]}\n' "$mode" "$a" "$b" > "target/probes/workspace-$mode-result.json"
  echo workspace.split_ratio_direction_native_sizes_confirmed
  exit 0
fi
if [ "$mode" = split ]; then
  timeout 3s import -window "$xid" target/probes/workspace-split-before.png
  # Header/sidebar controls drift; activate the split action by accessible name.
  sidebar_action "左右分屏"
  await_count terminal_input_ready 4
  kill -0 "$a"; kill -0 "$b"
  timeout 2s xdotool mousemove --window "$xid" 300 350 click 1
  input "$b" ROW_B 2
  timeout 2s xdotool mousemove --window "$xid" 750 350 click 1
  input "$a" ROW_A 2
  [ "$(tail -1 "$fixture/children/$a.size")" = '31 45' ] && [ "$(tail -1 "$fixture/children/$b.size")" = '31 45' ] || { echo workspace.split_row_native_size >&2; exit 1; }
  [ "$(wc -l < "$fixture/children/$a.input")" -eq 2 ]; [ "$(wc -l < "$fixture/children/$b.input")" -eq 2 ]
  drawn ROW_B target/probes/workspace-split-two.png 2 200 149
  drawn ROW_A target/probes/workspace-split-two.png 2 573 149
  # Same two sessions, now a column split, not two more shell launches.
  sidebar_action "上下分屏"
  await_count terminal_input_ready 6
  timeout 2s xdotool mousemove --window "$xid" 350 220 click 1
  input "$b" COLUMN_B 3
  timeout 2s xdotool mousemove --window "$xid" 350 460 click 1
  input "$a" COLUMN_A 3
  [ "$(tail -1 "$fixture/children/$a.size")" = '15 92' ] && [ "$(tail -1 "$fixture/children/$b.size")" = '15 92' ] || { echo workspace.split_column_native_size >&2; exit 1; }
  [ "$(count session_opened)" -eq 2 ]; kill -0 "$a"; kill -0 "$b"
  drawn COLUMN_B target/probes/workspace-split-column.png 3 200 149
  drawn COLUMN_A target/probes/workspace-split-column.png 3 200 388
  # Selecting a retained session only focuses it. Explicitly collapse the focused B pane.
  timeout 2s xdotool mousemove --window "$xid" 350 220 click 1
  sidebar_action "仅显示焦点"
  await_count terminal_input_ready 7
  input "$b" SPLIT_B_SINGLE 4
  kill -0 "$a"; kill -0 "$b"
  timeout 2s xdotool key --clearmodifiers ctrl+shift+r
  await_count document_confirmed 2
  await_count terminal_input_ready 8
  input "$b" SPLIT_B_REFRESH 5
  kill -0 "$a"; kill -0 "$b"; [ "$(count session_opened)" -eq 2 ]
  sidebar_action "左右分屏"
  await_count terminal_input_ready 10
  timeout 2s xdotool mousemove --window "$xid" 750 350 click 1
  input "$a" ROW_A_REBOUND 4
  # Close focused A, not the B tab owner; default cancel preserves both.
  timeout 2s xdotool mousemove --window "$xid" 920 80 click 1
  timeout 2s xdotool key --clearmodifiers Return
  [ "$(count session_closed)" -eq 0 ]; kill -0 "$a"; kill -0 "$b"
  timeout 2s xdotool mousemove --window "$xid" 920 80 click 1
  timeout 2s xdotool key --clearmodifiers Tab Return
  await_count session_closed 1
  await_count terminal_input_ready 11
  ! kill -0 "$a" 2>/dev/null || { echo workspace.split_focused_close_mismatch >&2; exit 1; }
  kill -0 "$b"
  input "$b" SPLIT_B_AFTER_CLOSE 6
  drawn SPLIT_B_AFTER_CLOSE target/probes/workspace-split-survivor.png 6
  [ "$(count session_opened)" -eq 2 ]; ! grep -q '^workspace.terminal_failure.' "$log"
  timeout 2s xdotool windowactivate --sync "$xid"
  timeout 2s xdotool key --clearmodifiers alt+F4
  await_count exited 1
  wait "$app"
  ! kill -0 "$b" 2>/dev/null || { echo workspace.split_survivor_not_reaped >&2; exit 1; }
  for pid in "$a" "$b"; do
    cp -p -- "$fixture/children/$pid.input" "target/probes/workspace-$mode-input-$pid.txt"
    cp -p -- "$fixture/children/$pid.size" "target/probes/workspace-$mode-size-$pid.txt"
  done
  printf '{"version":1,"mode":"%s","status":"passed","gui_exit_code":0,"sessions_opened":2,"native_reaped":true,"pids":[%s,%s]}\n' "$mode" "$a" "$b" > "target/probes/workspace-$mode-result.json"
  echo workspace.two_pane_row_column_input_focused_close_confirmed
  exit 0
fi
# Select A by its actual tab, not a backend shortcut. A and B stay alive.
timeout 2s xdotool mousemove --window "$xid" 234 80 click 1
await_count terminal_input_ready 3
input "$a" TAB_A_RESTORED 2
# Re-clicking the already selected tab must focus its output without re-attaching:
# no extra terminal_input_ready, no extra session, both PTYs alive.
timeout 2s xdotool mousemove --window "$xid" 234 80 click 1
sleep 0.5
[ "$(count terminal_input_ready)" -eq 3 ] || { echo workspace.tabs_selected_reactivated >&2; exit 1; }
[ "$(count session_opened)" -eq 2 ] || { echo workspace.tabs_selected_respawned >&2; exit 1; }
kill -0 "$a"; kill -0 "$b"
# Select the other tab by click; a non-selected tab must attach and report input readiness.
timeout 2s xdotool mousemove --window "$xid" 305 80 click 1
await_count terminal_input_ready 4
input "$b" TAB_B_KEYBOARD 2
# New page must restore B from display preference, not select A or spawn again.
timeout 2s xdotool key --clearmodifiers ctrl+shift+r
await_count document_confirmed 2
await_count terminal_input_ready 5
[ "$(count session_opened)" -eq 2 ]; [ "$(count profile_selected.sh)" -eq 2 ]
kill -0 "$a"; kill -0 "$b"
input "$b" TAB_B_AFTER_REFRESH 3
drawn TAB_B_AFTER_REFRESH target/probes/workspace-tabs-two.png
# Current B is reaped, A automatically restored. Close is never a mere detach.
timeout 2s xdotool mousemove --window "$xid" 920 80 click 1
# Default Enter is Keep session; both PTYs and their view survive cancellation.
timeout 2s xdotool key --clearmodifiers Return
[ "$(count session_closed)" -eq 0 ]; kill -0 "$a"; kill -0 "$b"
timeout 2s xdotool mousemove --window "$xid" 920 80 click 1
timeout 2s xdotool key --clearmodifiers Tab Return
await_count session_closed 1
await_count terminal_input_ready 6
! kill -0 "$b" 2>/dev/null || { echo workspace.tabs_closed_pid_alive >&2; exit 1; }
kill -0 "$a"
input "$a" TAB_A_AFTER_CLOSE 3
drawn TAB_A_AFTER_CLOSE target/probes/workspace-tabs-survivor.png
[ "$(count session_opened)" -eq 2 ]
! grep -q '^workspace.terminal_failure.' "$log"
[ "$(xdotool getwindowpid "$xid")" = "$app" ]
timeout 2s xdotool windowactivate --sync "$xid"
timeout 2s xdotool key --clearmodifiers alt+F4
await_count exited 1
wait "$app"
[ "$(count shutdown_complete)" -eq 1 ]
! kill -0 "$a" 2>/dev/null || { echo workspace.tabs_shutdown_pid_alive >&2; exit 1; }
echo workspace.two_tab_keyboard_refresh_close_confirmed
