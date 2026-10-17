#!/bin/sh
# Actual trusted right-clicks in a private Xvfb, not synthetic browser events.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-workspace-right-paste.XXXXXX)
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$fixture/bin"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
printf '%s\n' "$$" > "$PULSETERM_CLIP_FIXTURE.pid"
printf 'Right clipboard fixed fixture\r\n'
interrupted=0
trap 'interrupted=1; printf "interrupt\n" >> "$PULSETERM_CLIP_FIXTURE.interrupt"' INT
while :; do
  if ! IFS= read -r line; then
    if [ "$interrupted" -eq 1 ]; then interrupted=0; continue; fi
    break
  fi
  printf '%s\n' "$line" >> "$PULSETERM_CLIP_FIXTURE.input"
  printf 'echo:%s\r\n' "$line"
  case "$line" in
    MOUSE_ON) printf '\033[?1000h\033[?1006hVISUAL READY ON\r\n' ;;
    MOUSE_OFF) printf '\033[?1000l\033[?1006lVISUAL READY OFF\r\n' ;;
  esac
done
SHELL
chmod 700 "$fixture/bin/sh"
log=target/probes/workspace-right-click-paste-native.log
PULSETERM_CLIP_FIXTURE="$fixture/native" PATH="$fixture/bin" SHELL="$fixture/bin/sh" target/debug/pulseterm-workspace > "$log" 2>&1 &
app=$!; clip=""
cleanup(){ if [ -n "$clip" ]; then kill "$clip" 2>/dev/null || true; wait "$clip" 2>/dev/null || true; fi; kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true; rm -rf "$fixture"; test ! -e "$fixture"; }
trap cleanup EXIT
await_count(){ label=$1; expected=$2; attempt=0; while [ "$(grep -c "^workspace.$label$" "$log" || true)" -lt "$expected" ]; do [ "$attempt" -lt 80 ] || { if [ -n "${xid:-}" ]; then timeout 3s import -window "$xid" "target/probes/workspace-right-click-paste-timeout-$label.png" || true; fi; echo "workspace.right_paste_timeout.$label" >&2; exit 1; }; kill -0 "$app"; attempt=$((attempt+1)); sleep 0.1; done; }
await_count sessions_listed 1
xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
[ "$(printf '%s\n' "$xid" | awk 'NF {n++} END {print n+0}')" -eq 1 ]; [ "$(xdotool getwindowpid "$xid")" = "$app" ]
timeout 2s xdotool windowactivate --sync "$xid"
paint=0; while :; do timeout 3s import -window "$xid" target/probes/workspace-right-click-paste-initial.png; colors=$(magick target/probes/workspace-right-click-paste-initial.png -format '%k' info:); [ "$colors" -gt 10 ] && break; [ "$paint" -lt 10 ] || exit 1; paint=$((paint+1)); sleep 0.1; done
timeout 2s xdotool mousemove --window "$xid" 80 164 click 1
await_count terminal_input_ready 1
pid=$(cat "$fixture/native.pid")
lines(){ if [ -f "$fixture/native.input" ]; then wc -l < "$fixture/native.input"; else echo 0; fi; }
await_lines(){ expected=$1; attempt=0; while [ "$(lines)" -lt "$expected" ]; do [ "$attempt" -lt 80 ] || { timeout 3s import -window "$xid" "target/probes/workspace-right-click-paste-input-timeout-$expected.png" || true; echo "workspace.right_paste_input_timeout.$expected" >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done; [ "$(lines)" -eq "$expected" ]; }
own_clip(){
  if [ -n "$clip" ]; then kill "$clip"; wait "$clip" 2>/dev/null || true; fi
  if [ "$#" -eq 1 ]; then
    target/probes/x11-clipboard "$fixture/paste.txt" "$1" > "$fixture/clipboard.log" 2>&1 &
  else
    target/probes/x11-clipboard "$fixture/paste.txt" > "$fixture/clipboard.log" 2>&1 &
  fi
  clip=$!
  attempt=0; while ! grep -q '^clipboard.ready$' "$fixture/clipboard.log"; do [ "$attempt" -lt 80 ] || exit 1; kill -0 "$clip"; attempt=$((attempt+1)); sleep 0.1; done
}
right(){ timeout 2s xdotool mousemove --window "$xid" 700 500 click 3; }
served(){ attempt=0; while ! grep -q '^clipboard.served$' "$fixture/clipboard.log"; do [ "$attempt" -lt 80 ] || { timeout 3s import -window "$xid" target/probes/workspace-right-click-paste-read-failure.png; echo workspace.right_paste_clipboard_not_requested >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done; }
requested(){ attempt=0; while ! grep -q '^clipboard.requested$' "$fixture/clipboard.log"; do [ "$attempt" -lt 80 ] || { echo workspace.right_paste_delayed_read_not_requested >&2; exit 1; }; kill -0 "$clip"; attempt=$((attempt+1)); sleep 0.1; done; }
cc -std=c11 -Wall -Wextra -Werror scripts/probe-x11-status.c -o "$fixture/observer" $(pkg-config --cflags --libs atspi-2 gobject-2.0)
observer(){ timeout 2s "$fixture/observer" "$app" "$1" "$2"; }
visible_mode(){
  marker=$1; attempt=0
  # Fixed 13px fixture rows; isolate the marker from the blinking next-row cursor.
  case "$marker" in
    'VISUAL READY ON') crop='220x15+200+284' ;;
    'VISUAL READY OFF') crop='220x15+200+329' ;;
    *) exit 1 ;;
  esac
  while :; do
    timeout 3s import -window "$xid" "$fixture/mode.png"
    magick "$fixture/mode.png" -crop "$crop" +repage -resize 400% "$fixture/mode-terminal.png"
    timeout 3s tesseract "$fixture/mode-terminal.png" "$fixture/mode" -l eng --psm 7 >/dev/null 2>&1
    grep -q "$marker" "$fixture/mode.txt" && break
    [ "$attempt" -lt 80 ] || { cp "$fixture/mode.png" "target/probes/workspace-right-click-paste-mode-timeout.png"; echo workspace.right_paste_mode_not_drawn >&2; exit 1; }
    attempt=$((attempt+1)); sleep 0.1
  done
}
printf DEFAULT_DISABLED > "$fixture/paste.txt"; own_clip; right
timeout 2s xdotool key --clearmodifiers Escape
[ "$(lines)" -eq 0 ]
# Save exactly one known field in the current page; source and ticket checks are native.
observer '设置' activate
await_count settings_read_confirmed 1
attempt=0; until observer '右键粘贴' focus > "$fixture/status.log" 2>&1; do [ "$attempt" -lt 80 ] || { cat "$fixture/status.log" >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
timeout 2s xdotool key --clearmodifiers space Tab Return
await_count settings_save_confirmed 1
grep -q 'right_click_paste = true' "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
attempt=0; until timeout 2s "$fixture/observer" "$app" '已保存 · 重新加载终端后应用；当前会话未关闭' > "$fixture/status.log" 2>&1; do [ "$attempt" -lt 80 ] || exit 1; attempt=$((attempt+1)); sleep 0.1; done
paint_settings(){
  deadline=$(( $(date +%s) + 8 ))
  while :; do
    timeout 3s import -window "$xid" "$1"
    timeout 3s tesseract "$1" "$fixture/settings-frame" --psm 6 > "$fixture/settings-frame.log" 2>&1
    if grep -q monospace "$fixture/settings-frame.txt" && python3 - "$1" <<'PY'
from PIL import Image
import sys
with Image.open(sys.argv[1]) as image:
    rgb = image.convert('RGB')
    data = rgb.get_flattened_data() if hasattr(rgb, 'get_flattened_data') else rgb.getdata()
    # Only right_click_paste is checked; fixture terminal output is monochrome.
    count = sum(1 for r,g,b in data if b > 130 and b-r > 35 and b-g > 15)
sys.exit(0 if count >= 10 else 1)
PY
    then break; fi
    [ "$(date +%s)" -lt "$deadline" ] || { echo workspace.right_paste_settings_not_painted >&2; exit 1; }
    sleep 0.1
  done
}
paint_settings target/probes/workspace-right-click-paste-settings.png
observer '完成' activate
timeout 2s xdotool key --clearmodifiers ctrl+shift+r
await_count terminal_input_ready 2
kill -0 "$pid"; [ "$(grep -c '^workspace.session_opened$' "$log")" -eq 1 ]
printf RIGHT_SINGLE > "$fixture/paste.txt"; own_clip; right; served
timeout 2s xdotool key --clearmodifiers Return
await_lines 1; [ "$(tail -1 "$fixture/native.input")" = RIGHT_SINGLE ]
printf 'RIGHT_ONE\nRIGHT_TWO\n' > "$fixture/paste.txt"; own_clip; right; served
timeout 2s xdotool key --clearmodifiers Return
[ "$(lines)" -eq 1 ]
own_clip; right; served
timeout 2s xdotool key --clearmodifiers Tab Return
await_lines 3
[ "$(sed -n '2p' "$fixture/native.input")" = RIGHT_ONE ]; [ "$(sed -n '3p' "$fixture/native.input")" = RIGHT_TWO ]
printf 'RIGHT_RETIRED_ONE\nRIGHT_RETIRED_TWO\n' > "$fixture/paste.txt"; own_clip; right; served
timeout 2s xdotool key --clearmodifiers ctrl+shift+r
await_count terminal_input_ready 3
[ "$(lines)" -eq 3 ]; kill -0 "$pid"
# Mouse-reporting mode has precedence over the clipboard preference.
timeout 2s xdotool type --clearmodifiers MOUSE_ON
timeout 2s xdotool key --clearmodifiers Return
await_lines 4
visible_mode "VISUAL READY ON"
reads_before=$(grep -c '^workspace.clipboard_read_finished.' "$log" || true)
printf MOUSE_CLIPBOARD_BLOCKED > "$fixture/paste.txt"; own_clip; right
timeout 2s xdotool key --clearmodifiers Escape
timeout 2s xdotool mousemove --window "$xid" 700 500 click 1
timeout 2s xdotool key --clearmodifiers ctrl+u
[ "$(lines)" -eq 4 ]
[ "$(grep -c '^workspace.clipboard_read_finished.' "$log" || true)" -eq "$reads_before" ]
! grep -q '^clipboard.served$' "$fixture/clipboard.log"
timeout 2s xdotool type --clearmodifiers MOUSE_OFF
timeout 2s xdotool key --clearmodifiers Return
await_lines 5
visible_mode "VISUAL READY OFF"
[ "$(tail -1 "$fixture/native.input")" = MOUSE_OFF ]
printf RIGHT_AFTER_MOUSE > "$fixture/paste.txt"; own_clip; right; served
timeout 2s xdotool key --clearmodifiers Return
await_lines 6; [ "$(tail -1 "$fixture/native.input")" = RIGHT_AFTER_MOUSE ]
! grep -q 'RIGHT_RETIRED\|MOUSE_CLIPBOARD_BLOCKED\|DEFAULT_DISABLED' "$fixture/native.input"
! grep -q '^workspace.terminal_failure.' "$log"
timeout 3s import -window "$xid" target/probes/workspace-right-click-paste-final.png
# A real delayed X11 reply must not cross the original page or bypass the exit barrier.
rejected=$(grep -c '^workspace.clipboard_read_finished.rejected$' "$log" || true)
printf LATE_PAGE_REJECTED > "$fixture/paste.txt"; own_clip "$fixture/release-page"; right; requested
timeout 2s xdotool key --clearmodifiers ctrl+shift+r
await_count terminal_input_ready 4
[ "$(lines)" -eq 6 ]; kill -0 "$pid"
[ "$(grep -c '^workspace.clipboard_read_finished.rejected$' "$log" || true)" -eq "$rejected" ]
touch "$fixture/release-page"; served
rejected=$((rejected+1)); await_count clipboard_read_finished.rejected "$rejected"
! grep -q '^clipboard.gate_timeout$' "$fixture/clipboard.log"
timeout 2s xdotool key --clearmodifiers Return
await_lines 7; [ -z "$(tail -1 "$fixture/native.input")" ]
printf LATE_CLOSE_REJECTED > "$fixture/paste.txt"; own_clip "$fixture/release-close"; right; requested
timeout 2s xdotool key --clearmodifiers alt+F4
await_count shutdown_started 1
kill -0 "$app"
[ "$(grep -c '^workspace.exited$' "$log" || true)" -eq 0 ]
[ "$(grep -c '^workspace.clipboard_read_finished.rejected$' "$log" || true)" -eq "$rejected" ]
touch "$fixture/release-close"; served
rejected=$((rejected+1)); await_count clipboard_read_finished.rejected "$rejected"
! grep -q '^clipboard.gate_timeout$' "$fixture/clipboard.log"
await_count exited 1
wait "$app"
! kill -0 "$pid" 2>/dev/null
cp "$fixture/native.input" target/probes/workspace-right-click-paste-input.txt
[ "$(lines)" -eq 7 ]
! grep -q 'LATE_PAGE\|LATE_CLOSE' "$fixture/native.input"
! grep -q '^workspace.terminal_failure.' "$log"
echo workspace.right_click_paste_default_off_settings_single_multiline_approve_cancel_refresh_mouse_mode_late_read_exit_barrier_confirmed
