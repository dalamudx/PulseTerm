#!/bin/sh
# Actual user clipboard events via GTK selection owner, in a private Xvfb only.
set -eu
umask 077
mkdir -p target/probes
mkdir -p /data/temp
fixture=$(mktemp -d /data/temp/pt-workspace-clipboard.XXXXXX)
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$fixture/bin"
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture"
clipboard_helper=${PULSE_CLIPBOARD_HELPER:-target/probes/x11-clipboard}
status_helper=${PULSE_X11_STATUS:-target/probes/x11-status}
# Explicit controlled font baseline; production compiled default remains 14px.
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
printf '%s\n' "$$" > "$PULSETERM_CLIP_FIXTURE.pid"
printf 'Clipboard fixed fixture\r\n'
interrupted=0
trap 'interrupted=1; printf "interrupt\n" >> "$PULSETERM_CLIP_FIXTURE.interrupt"' INT
while :; do
  if ! IFS= read -r line; then
    if [ "$interrupted" -eq 1 ]; then interrupted=0; continue; fi
    break
  fi
  printf '%s\n' "$line" >> "$PULSETERM_CLIP_FIXTURE.input"
  printf 'echo:%s\r\n' "$line"
done
SHELL
chmod 700 "$fixture/bin/sh"
log=target/probes/workspace-clipboard-native.log
PULSETERM_CLIP_FIXTURE="$fixture/native" PATH="$fixture/bin" SHELL="$fixture/bin/sh" "${PULSE_WORKSPACE_BINARY:-${CARGO_TARGET_DIR:-target}/debug/pulseterm-workspace}" > "$log" 2>&1 &
app=$!; clip=""
cleanup(){ if [ -n "$clip" ]; then kill "$clip" 2>/dev/null || true; wait "$clip" 2>/dev/null || true; fi; kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true; rm -rf "$fixture"; }
trap cleanup EXIT
await_count(){ label=$1; expected=$2; attempt=0; while [ "$(grep -c "^workspace.$label$" "$log" || true)" -lt "$expected" ]; do [ "$attempt" -lt 80 ] || { echo "workspace.clipboard_timeout.$label" >&2; exit 1; }; kill -0 "$app"; attempt=$((attempt+1)); sleep 0.1; done; }
await_count sessions_listed 1
xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
[ "$(printf '%s\n' "$xid" | awk 'NF {n++} END {print n+0}')" -eq 1 ]; [ "$(xdotool getwindowpid "$xid")" = "$app" ]
timeout 2s xdotool windowactivate --sync "$xid"
paint=0; while :; do timeout 3s import -window "$xid" target/probes/workspace-clipboard-initial.png; colors=$(convert target/probes/workspace-clipboard-initial.png -format '%k' info:); [ "$colors" -gt 10 ] && break; [ "$paint" -lt 10 ] || exit 1; paint=$((paint+1)); sleep 0.1; done
timeout 2s xdotool mousemove --window "$xid" 80 164 click 1
await_count terminal_input_ready 1
[ "$(grep -c '^workspace.profile_selected.sh$' "$log")" -eq 1 ]
pid=$(cat "$fixture/native.pid")
lines(){ if [ -f "$fixture/native.input" ]; then wc -l < "$fixture/native.input"; else echo 0; fi; }
own_clip(){
  if [ -n "$clip" ]; then kill "$clip"; wait "$clip" 2>/dev/null || true; fi
  "$clipboard_helper" "$fixture/paste.txt" > "$fixture/clipboard.log" 2>&1 & clip=$!
  attempt=0; while ! grep -q '^clipboard.ready$' "$fixture/clipboard.log"; do [ "$attempt" -lt 80 ] || exit 1; kill -0 "$clip"; attempt=$((attempt+1)); sleep 0.1; done
}
set_clip(){
  own_clip
  timeout 2s xdotool key --clearmodifiers ctrl+shift+v
  attempt=0; while ! grep -q '^clipboard.served$' "$fixture/clipboard.log"; do [ "$attempt" -lt 80 ] || { echo workspace.clipboard_not_requested >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
}
await_lines(){ expected=$1; attempt=0; while [ "$(lines)" -lt "$expected" ]; do [ "$attempt" -lt 80 ] || { echo workspace.clipboard_write_timeout >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done; [ "$(lines)" -eq "$expected" ]; }
printf PASTE_SINGLE > "$fixture/paste.txt"
set_clip
timeout 2s xdotool key --clearmodifiers Return
await_lines 1
[ "$(tail -1 "$fixture/native.input")" = PASTE_SINGLE ]
printf COPY_SENTINEL_DIFFERENT > "$fixture/paste.txt"
own_clip
# Select the visible canonical echo by a real pointer drag and copy via OS Ctrl+C.
# Row 2, 12 ASCII cells; no programmatic Terminal.select or synthetic ClipboardEvent.
timeout 2s xdotool mousemove --window "$xid" 200 170 mousedown 1 mousemove --window "$xid" 296 170 mouseup 1
if [ "${PULSE_COPY_ON_SELECT_PROBE:-0}" = 1 ]; then
  timeout 3s "$clipboard_helper" read "$fixture/default-copy.txt"
  [ "$(cat "$fixture/default-copy.txt")" = COPY_SENTINEL_DIFFERENT ]
  observer(){ if [ "$2" = status ]; then timeout 3s "$status_helper" "$app" "$1"; else timeout 3s "$status_helper" "$app" "$1" "$2"; fi; }
  paint_settings(){
    expected=0; [ "$2" != enabled ] || expected=1
    settings_frame_deadline=$(( $(date +%s) + 8 ))
    while :; do
      if observer 选中复制 inspect-checkbox > "$fixture/checkbox-state.txt" && grep -q "showing=1 enabled=1 .*checked=$expected" "$fixture/checkbox-state.txt"; then break; fi
      [ "$(date +%s)" -lt "$settings_frame_deadline" ] || { echo workspace.selection_copy_checkbox_mismatch >&2; exit 1; }
      sleep 0.1
    done
    timeout 3s import -window "$xid" "$1"
  }
  close_settings(){
    observer 完成 activate
    close_deadline=$(( $(date +%s) + 8 ))
    while observer 完成 status > "$fixture/closing-state.txt" 2>&1; do
      [ "$(date +%s)" -lt "$close_deadline" ] || { echo workspace.selection_copy_settings_not_closed >&2; exit 1; }
      sleep 0.1
    done
    timeout 2s xdotool mousemove --window "$xid" 700 500 click 1
  }
  attempt=0; until observer 设置 activate > "$fixture/status.log" 2>&1; do [ "$attempt" -lt 80 ] || { cat "$fixture/status.log" >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
  observer 搜索设置 focus
  timeout 2s xdotool key --clearmodifiers ctrl+a
  timeout 2s xdotool type --clearmodifiers --delay 1 copy_on_select
  attempt=0; until observer 选中复制 focus > "$fixture/status.log" 2>&1; do [ "$attempt" -lt 80 ] || { cat "$fixture/status.log" >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
  timeout 2s xdotool key --clearmodifiers space
  paint_settings "$fixture/draft-enabled.png" enabled
  observer 保存选中复制 activate
  attempt=0; until grep -q '^copy_on_select = true$' "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"; do [ "$attempt" -lt 80 ] || { echo workspace.selection_copy_setting_not_saved >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
  attempt=0; until observer '已保存 · 重新加载终端后应用；当前会话未关闭' status > "$fixture/status.log" 2>&1; do [ "$attempt" -lt 80 ] || { cat "$fixture/status.log" >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
  paint_settings target/probes/workspace-selection-copy-settings-enabled.png enabled
  close_settings
  # Saving applies to the next view; this original disabled view must not copy.
  own_clip
  timeout 2s xdotool mousemove --window "$xid" 200 170 mousedown 1 mousemove --window "$xid" 296 170 mouseup 1
  timeout 3s "$clipboard_helper" read "$fixture/old-disabled-copy.txt"
  [ "$(cat "$fixture/old-disabled-copy.txt")" = COPY_SENTINEL_DIFFERENT ]
  timeout 2s xdotool key --clearmodifiers ctrl+shift+r
  await_count document_confirmed 2; await_count terminal_input_ready 2
  own_clip
  timeout 2s xdotool mousemove --window "$xid" 200 170 mousedown 1 mousemove --window "$xid" 296 170 mouseup 1
  attempt=0; while :; do timeout 3s "$clipboard_helper" read "$fixture/auto-copy.txt"; [ "$(cat "$fixture/auto-copy.txt")" = PASTE_SINGLE ] && break; [ "$attempt" -lt 80 ] || { echo workspace.selection_copy_mismatch >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
  kill -0 "$pid"; [ "$(lines)" -eq 1 ]
  timeout 3s import -window "$xid" target/probes/workspace-selection-copy-enabled.png
  observer 设置 activate
  observer 搜索设置 focus
  timeout 2s xdotool key --clearmodifiers ctrl+a
  timeout 2s xdotool type --clearmodifiers --delay 1 copy_on_select
  observer 选中复制 focus
  timeout 2s xdotool key --clearmodifiers space
  paint_settings "$fixture/draft-disabled.png" disabled
  observer 保存选中复制 activate
  attempt=0; until grep -q '^copy_on_select = false$' "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"; do [ "$attempt" -lt 80 ] || exit 1; attempt=$((attempt+1)); sleep 0.1; done
  attempt=0; until observer '已保存 · 重新加载终端后应用；当前会话未关闭' status > "$fixture/status.log" 2>&1; do [ "$attempt" -lt 80 ] || { cat "$fixture/status.log" >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
  paint_settings target/probes/workspace-selection-copy-settings-disabled.png disabled
  observer 选中复制恢复继承 activate
  await_count settings_save_confirmed 3
  ! grep -q '^copy_on_select' "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
  paint_settings target/probes/workspace-selection-copy-settings-inherited.png disabled
  close_settings
  # Explicit false and inheritance reset must not change the already enabled view.
  own_clip
  timeout 2s xdotool mousemove --window "$xid" 200 170 mousedown 1 mousemove --window "$xid" 296 170 mouseup 1
  attempt=0; while :; do
    timeout 3s "$clipboard_helper" read "$fixture/old-enabled-copy.txt"
    [ "$(cat "$fixture/old-enabled-copy.txt")" = PASTE_SINGLE ] && break
    [ "$attempt" -lt 80 ] || {
      cp -p -- "$fixture/old-enabled-copy.txt" target/probes/workspace-selection-copy-old-enabled-failure.txt
      timeout 3s import -window "$xid" target/probes/workspace-selection-copy-old-enabled-failure.png
      echo workspace.selection_copy_old_view_changed >&2; exit 1;
    }
    attempt=$((attempt+1)); sleep 0.1
  done
  timeout 2s xdotool key --clearmodifiers ctrl+shift+r
  await_count document_confirmed 3; await_count terminal_input_ready 3
  own_clip
  timeout 2s xdotool mousemove --window "$xid" 200 170 mousedown 1 mousemove --window "$xid" 296 170 mouseup 1
  timeout 3s "$clipboard_helper" read "$fixture/disabled-copy.txt"
  [ "$(cat "$fixture/disabled-copy.txt")" = COPY_SENTINEL_DIFFERENT ]
  timeout 3s import -window "$xid" target/probes/workspace-selection-copy-disabled.png
  timeout 2s xdotool key --clearmodifiers alt+F4
  await_count exited 1; wait "$app"; ! kill -0 "$pid" 2>/dev/null
  [ "$(lines)" -eq 1 ]
  cp -p -- "$fixture/native.input" target/probes/workspace-selection-copy-input.txt
  cp -p -- "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml" target/probes/workspace-selection-copy-settings.toml
  for state in default auto disabled old-disabled old-enabled; do
    cp -p -- "$fixture/$state-copy.txt" "target/probes/workspace-selection-copy-$state.txt"
  done
  printf '{"version":1,"status":"passed","mode":"selection-copy","gui_exit_code":0,"native_reaped":true,"pty_pid":%s,"session_opened":1,"refreshes":2,"restore_inheritance":true}\n' "$pid" > target/probes/workspace-selection-copy-result.json
  echo workspace.selection_copy_default_off_settings_enable_refresh_actual_mouse_clipboard_disable_no_input_exit_confirmed
  exit 0
fi
timeout 2s xdotool key --clearmodifiers ctrl+c
timeout 3s "$clipboard_helper" read "$fixture/copied.txt"
[ "$(cat "$fixture/copied.txt")" = PASTE_SINGLE ] || { echo workspace.clipboard_copy_mismatch >&2; exit 1; }
kill -0 "$pid" # Selected Ctrl+C did not interrupt the Shell.
timeout 2s xdotool mousemove --window "$xid" 700 500 click 1
printf 'MULTI_ONE\nMULTI_TWO\n' > "$fixture/paste.txt"
set_clip
# Cancel is the modal's default keyboard focus. Enter must NOT execute content.
timeout 2s xdotool key --clearmodifiers Return
[ "$(lines)" -eq 1 ]
set_clip
# Explicit Tab to "Paste once" then Enter, no default/automatic approval.
timeout 2s xdotool key --clearmodifiers Tab Return
await_lines 3
[ "$(sed -n '2p' "$fixture/native.input")" = MULTI_ONE ]; [ "$(sed -n '3p' "$fixture/native.input")" = MULTI_TWO ]
printf 'RETIRED_ONE\nRETIRED_TWO\n' > "$fixture/paste.txt"
set_clip
timeout 2s xdotool key --clearmodifiers ctrl+shift+r
await_count document_confirmed 2
await_count terminal_input_ready 2
[ "$(lines)" -eq 3 ]; kill -0 "$pid"
# No selection: Ctrl+C must remain an actual PTY interrupt, not a copy shortcut.
timeout 2s xdotool key --clearmodifiers ctrl+c
attempt=0; while [ ! -f "$fixture/native.interrupt" ]; do [ "$attempt" -lt 80 ] || { echo workspace.clipboard_interrupt_missing >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
[ "$(wc -l < "$fixture/native.interrupt")" -eq 1 ]; kill -0 "$pid"
printf 'RESIZED_ONE\nRESIZED_TWO\n' > "$fixture/paste.txt"
set_clip
timeout 2s xdotool windowsize --sync "$xid" 840 560
await_count terminal_input_ready 3
[ "$(lines)" -eq 3 ]; kill -0 "$pid"
# Resize retired the decision and removed its modal; a late Return only enters
# an empty line into the new input generation, never the old clipboard command.
timeout 2s xdotool key --clearmodifiers Return
await_lines 4
[ -z "$(tail -1 "$fixture/native.input")" ]
# No second launch on reload; old decision cannot become current input.
[ "$(grep -c '^workspace.session_opened$' "$log")" -eq 1 ]
! grep -q '^workspace.terminal_failure.' "$log"
timeout 3s import -window "$xid" target/probes/workspace-clipboard-final.png
timeout 2s xdotool key --clearmodifiers alt+F4
await_count exited 1
wait "$app"
! kill -0 "$pid" 2>/dev/null
cp -p -- "$fixture/native.input" target/probes/workspace-clipboard-input.txt
printf '{"version":1,"status":"passed","mode":"clipboard","gui_exit_code":0,"native_reaped":true,"pty_pid":%s,"session_opened":1}\n' "$pid" > target/probes/workspace-clipboard-result.json
echo workspace.clipboard_single_cancel_approve_retire_confirmed
