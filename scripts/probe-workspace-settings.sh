#!/bin/sh
# Real settings UI in isolated directories; no user defaults and no arbitrary commands.
set -eu
umask 077
mkdir -p target/probes
mkdir -p /data/temp
if [ "${PULSE_FONT_PRESET:-0}" = 1 ]; then
  printf '%s\n' '{"version":1,"passed":false,"nativeExitConfirmed":false}' >target/probes/workspace-font-result.json
fi
fixture=$(mktemp -d /data/temp/pt-set.XXXXXX)
export HOME="$fixture/home" XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
app=""; browser_service=""
cleanup(){
  cleanup_status=$?
  trap - EXIT
  if [ "$cleanup_status" -ne 0 ] && [ "${PULSE_FONT_PRESET:-0}" = 1 ] && [ -n "${xid:-}" ] && [ -n "$app" ]; then
    if [ "$(timeout 2s xdotool getwindowpid "$xid" 2>/dev/null || true)" = "$app" ];then
      timeout 3s import -window "$xid" "target/probes/workspace-font-failure-$app.png" || true
      printf 'workspace.font_failure_snapshot.%s\n' "$app" >&2
    fi
  fi
  if [ -n "$app" ]; then kill "$app" 2>/dev/null||true;wait "$app" 2>/dev/null||true;fi
  if [ -n "$browser_service" ]; then
    /usr/bin/python3 "$fixture/link-browser.py" stop "$fixture"
    wait "$browser_service"
  fi
  rm -rf "$fixture"
  exit "$cleanup_status"
}
trap cleanup EXIT
export XDG_RUNTIME_DIR="$fixture/r"
mkdir -m 700 "$XDG_RUNTIME_DIR"
mkdir -m 700 "$fixture/bin"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
: "${PULSETERM_SETTINGS_FIXTURE:?}"
printf '%s\n' "$$" > "$PULSETERM_SETTINGS_FIXTURE/native.pid"
printf '%s\n' "$$" >> "$PULSETERM_SETTINGS_FIXTURE/native-pids"
printf 'Settings terminal fixed fixture\r\n'
while :; do
  if IFS= read -r text; then :; else result=$?; [ "$result" -gt 128 ] && continue; break; fi
  printf '%s\n' "$text" >> "$PULSETERM_SETTINGS_FIXTURE/input"
  /bin/stty size >> "$PULSETERM_SETTINGS_FIXTURE/size"
  printf 'echo:%s\r\n' "$text"
  case "$text" in
    FONT_*) printf "font-sample: -> => != === 中文 😀\r\n";;
    ALPHA_TERM_*) printf '{"term":"%s"}\n' "$TERM" > "$PULSETERM_SETTINGS_FIXTURE/$text-$$.json";;
    ALPHA_KEYS_*|ALPHA_COLORS_*) /usr/bin/python3 "$PULSETERM_SETTINGS_FIXTURE/settings-query.py" "$text" "$$";;
    ALPHA_HISTORY) n=0;while [ "$n" -lt 1100 ];do printf 'alpha-history-%04d\r\n' "$n";n=$((n+1));done;;
    ALPHA_INTERACTION) /usr/bin/python3 "$PULSETERM_SETTINGS_FIXTURE/terminal-interaction.py" worker "$PULSETERM_SETTINGS_FIXTURE";;
    ALPHA_ALT) printf '\033[?1049hALPHA-ALT-needle\r\n';;
    ALPHA_PRIMARY) printf '\033[?1049l';;
    ALPHA_FINISH) printf 'ALPHA-ENDED-needle\r\n';exit 0;;
    LIGATURES*) printf "ligature-sample: 中😀é => !== === -> != <= >= <=> && || :: := ...\r\n";;
  esac
done
SHELL
chmod 700 "$fixture/bin/sh"
if [ "${PULSE_ALPHA_SETTINGS:-0}" = 1 ]; then
  cp scripts/probe-workspace-terminal-interaction.py "$fixture/terminal-interaction.py"
  cp scripts/probe-workspace-terminal-links.py "$fixture/terminal-links.py"
  cp scripts/probe-workspace-link-browser.py "$fixture/link-browser.py"
  /usr/bin/python3 "$fixture/link-browser.py" serve "$fixture" > target/probes/workspace-link-browser-service.log 2>&1 &
  browser_service=$!
  browser_attempt=0
  while [ ! -f "$fixture/browser-config.json" ]; do
    kill -0 "$browser_service"
    [ "$browser_attempt" -lt 80 ] || { echo "workspace.browser_fixture_start_timeout.$fixture" >&2;exit 1; }
    browser_attempt=$((browser_attempt+1));sleep .1
  done
fi
log=target/probes/workspace-settings-native.log
workspace_bin="${CARGO_TARGET_DIR:-target}/debug/pulseterm-workspace"
PULSETERM_SETTINGS_FIXTURE="$fixture" PATH="$fixture/bin" SHELL="$fixture/bin/sh" "$workspace_bin" > "$log" 2>&1 &
app=$!
count(){ grep -c "^workspace.$1$" "$log"||true; }
await_count(){ wait_key=$1;wait_goal=$2;wait_tries=0;while [ "$(count "$wait_key")" -lt "$wait_goal" ];do kill -0 "$app" 2>/dev/null||exit 1;[ "$wait_tries" -lt 80 ]||{ echo "workspace.settings_timeout.$wait_key" >&2; if [ -n "${xid:-}" ];then timeout 3s import -window "$xid" "target/probes/workspace-settings-failure-$wait_key.png" || true;fi;tail -18 "$log" >&2;exit 1;};wait_tries=$((wait_tries+1));sleep 0.1;done; }
native_helper="${PULSE_NATIVE_HELPER:-target/probes/x11-status}"
settings_open(){
  settings_reads=$(count settings_read_confirmed)
  timeout 3s "$native_helper" "$app" '设置' activate
  await_count settings_read_confirmed "$((settings_reads+1))"
}
settings_filter(){
  printf 'workspace.font_step.filter.%s\n' "$1"
  timeout 3s sh -c 'until timeout 2s "$1" "$2" "搜索设置" focus; do sleep .05; done' sh "$native_helper" "$app"
  timeout 2s xdotool key --clearmodifiers ctrl+a
  timeout 2s xdotool type --clearmodifiers "$1"
}
metric_filter(){
  case "$1" in
    '行高（%）') settings_filter line_height_percent ;;
    '字距（px）') settings_filter letter_spacing ;;
    *) return 1 ;;
  esac
}
await_count profiles_listed 1
xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true)
[ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ]
[ "$(xdotool getwindowpid "$xid")" = "$app" ]
timeout 2s xdotool windowactivate --sync "$xid"
paint=0
while :;do timeout 3s import -window "$xid" target/probes/workspace-settings-start.png;[ "$(magick target/probes/workspace-settings-start.png -format '%k' info:)" -gt 10 ]&&break;[ "$paint" -lt 10 ]||exit 1;paint=$((paint+1));sleep 0.1;done
# No file yet: UI must report compiled 14/default rather than replacing a target.
file="$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
[ ! -e "$file" ]
# Open exactly the discovered fixed sh, then verify kernel grid at compiled 14px.
timeout 2s xdotool key --clearmodifiers Tab Return
await_count terminal_input_ready 1
[ "$(count profile_selected.sh)" -eq 1 ]
input(){ text=$1;lines=$2; timeout 2s xdotool type --clearmodifiers --delay 1 "$text";timeout 2s xdotool key --clearmodifiers Return; attempt=0;while [ ! -f "$fixture/input" ]||[ "$(wc -l < "$fixture/input")" -lt "$lines" ];do [ "$attempt" -lt 80 ]||exit 1;attempt=$((attempt+1));sleep 0.1;done;[ "$(wc -l < "$fixture/input")" -eq "$lines" ];[ "$(tail -1 "$fixture/input")" = "$text" ];}
input DEFAULT_FONT 1
pid=$(cat "$fixture/native.pid")
old_rows=$(head -1 "$fixture/size"|awk '{print $1}')
old_cols=$(head -1 "$fixture/size"|awk '{print $2}')
kill -0 "$pid"
if [ "${PULSE_ALPHA_SETTINGS:-0}" = 1 ]; then
  python3 scripts/probe-workspace-settings-contract.py "$fixture" "$app" "$xid" "$file" "$log"
  timeout 2s xdotool key --clearmodifiers alt+F4
  await_count exited 1;wait "$app"
  python3 scripts/probe-workspace-settings-contract.py finalize "$fixture"
  exit 0
fi
settings_open
await_count settings_read_confirmed 2
sleep 0.2
timeout 3s import -window "$xid" target/probes/workspace-settings-default.png
# Focus the current accessible field; adding settings/actions must not select a different row.
settings_filter font_size
timeout 3s "$native_helper" "$app" '字体大小' focus
timeout 2s xdotool key --clearmodifiers ctrl+a
timeout 2s xdotool type --clearmodifiers 18
timeout 2s xdotool key --clearmodifiers Return
await_count settings_save_confirmed 1
grep -Eq 'font_size[[:space:]]*=[[:space:]]*18' "$file"
sleep 0.2
timeout 3s import -window "$xid" target/probes/workspace-settings-saved.png
timeout 2s xdotool key --clearmodifiers Escape ctrl+shift+r
await_count document_confirmed 2
await_count profiles_listed 2
await_count settings_read_confirmed 3
await_count terminal_input_ready 2
settings_open
await_count settings_read_confirmed 4
sleep 0.2
timeout 3s import -window "$xid" target/probes/workspace-settings-refreshed.png
[ "$(count session_opened)" -eq 1 ]
timeout 2s xdotool key --clearmodifiers Escape
await_count terminal_input_ready 2
# Give output explicit focus; selecting the tab never starts another child.
timeout 2s xdotool mousemove --window "$xid" 400 300 click 1
input USER_FONT 2
new_rows=$(tail -1 "$fixture/size"|awk '{print $1}')
new_cols=$(tail -1 "$fixture/size"|awk '{print $2}')
[ "$new_rows" -lt "$old_rows" ] && [ "$new_cols" -lt "$old_cols" ] || { echo workspace.font_grid_unchanged >&2;exit 1; }
[ "$(cat "$fixture/native.pid")" = "$pid" ];kill -0 "$pid"
# Compare actual native ACK grid with the independent kernel stty receipt.
reported=$(grep '^workspace.terminal_resized.' "$log"|tail -1|awk -F. '{print $4 " " $3}')
[ "$reported" = "$new_rows $new_cols" ]
sleep 0.2
timeout 3s import -window "$xid" target/probes/workspace-settings-font-output.png
echo "workspace.font_grid.$old_cols.$old_rows.$new_cols.$new_rows"
# Independently change vertical and horizontal cell metrics without spawning a child.
metric_edit(){
  label=$1;value=$2;saves=$3
  settings_open; metric_filter "$label"
  timeout 3s "$native_helper" "$app" "$label" focus
  timeout 2s xdotool key --clearmodifiers ctrl+a
  timeout 2s xdotool type --clearmodifiers "$value"
  timeout 2s xdotool key --clearmodifiers Return
  await_count settings_save_confirmed "$saves"
  timeout 2s xdotool key --clearmodifiers Escape ctrl+shift+r
  await_count terminal_input_ready "$((saves+1))"
  timeout 2s xdotool mousemove --window "$xid" 400 300 click 1
}
metric_edit '行高（%）' 150 2
input USER_HEIGHT 3
height_rows=$(tail -1 "$fixture/size"|awk '{print $1}')
height_cols=$(tail -1 "$fixture/size"|awk '{print $2}')
[ "$height_rows" -lt "$new_rows" ] && [ "$height_cols" -eq "$new_cols" ]
metric_edit '字距（px）' 2 3
input USER_SPACING 4
spacing_rows=$(tail -1 "$fixture/size"|awk '{print $1}')
spacing_cols=$(tail -1 "$fixture/size"|awk '{print $2}')
[ "$spacing_rows" -eq "$height_rows" ] && [ "$spacing_cols" -lt "$height_cols" ]
[ "$(count session_opened)" -eq 1 ]
[ "$(cat "$fixture/native.pid")" = "$pid" ];kill -0 "$pid"
node -e 'const s=require("node:fs").readFileSync(process.argv[1],"utf8");for(const [k,v] of [["font_size",18],["line_height_percent",150],["letter_spacing",2]])if(!new RegExp("^"+k+"\\s*=\\s*"+v+"\\s*$","m").test(s))process.exit(1)' "$file"
reported=$(awk -F. '/^workspace.terminal_resized./{v=$4 " " $3}END{print v}' "$log")
[ "$reported" = "$spacing_rows $spacing_cols" ]
sleep 0.2
timeout 3s import -window "$xid" target/probes/workspace-settings-metrics-output.png
echo "workspace.metrics_grid.$new_cols.$new_rows.$height_cols.$height_rows.$spacing_cols.$spacing_rows"
timeout 2s xdotool key --clearmodifiers alt+F4
await_count exited 1;wait "$app"
if kill -0 "$pid" 2>/dev/null;then echo workspace.settings_native_still_alive >&2;exit 1;fi
saved_hash=$(sha256sum "$file"|awk '{print $1}')
log=target/probes/workspace-settings-reopened-native.log
PULSETERM_SETTINGS_FIXTURE="$fixture" PATH="$fixture/bin" SHELL="$fixture/bin/sh" "$workspace_bin" > "$log" 2>&1 &
app=$!
await_count profiles_listed 1
xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true)
[ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ]
timeout 2s xdotool windowactivate --sync "$xid"
timeout 2s xdotool key --clearmodifiers Tab Return
await_count terminal_input_ready 1
timeout 2s xdotool mousemove --window "$xid" 400 300 click 1
input REOPEN_METRICS 5
[ "$(tail -1 "$fixture/size")" = "$spacing_rows $spacing_cols" ]
[ "$(sha256sum "$file"|awk '{print $1}')" = "$saved_hash" ]
pid=$(cat "$fixture/native.pid")
kill -0 "$pid"
metric_reset(){
  label=$1;saves=$2
  settings_open; metric_filter "$label"
  timeout 3s "$native_helper" "$app" "${label}恢复继承" activate
  await_count settings_save_confirmed "$saves"
  timeout 2s xdotool key --clearmodifiers Escape ctrl+shift+r
  await_count terminal_input_ready "$((saves+1))"
  timeout 2s xdotool mousemove --window "$xid" 400 300 click 1
}
metric_reset '行高（%）' 1
input RESET_HEIGHT 6
[ "$(tail -1 "$fixture/size")" = "$new_rows $spacing_cols" ]
node -e 'const s=require("node:fs").readFileSync(process.argv[1],"utf8");if(/^line_height_percent\s*=/m.test(s)||!/^letter_spacing\s*=\s*2\s*$/m.test(s))process.exit(1)' "$file"
metric_reset '字距（px）' 2
input RESET_SPACING 7
[ "$(tail -1 "$fixture/size")" = "$new_rows $new_cols" ]
node -e 'const s=require("node:fs").readFileSync(process.argv[1],"utf8");if(/^(line_height_percent|letter_spacing)\s*=/m.test(s)||!/^font_size\s*=\s*18\s*$/m.test(s))process.exit(1)' "$file"
[ "$(count session_opened)" -eq 1 ]
[ "$(cat "$fixture/native.pid")" = "$pid" ];kill -0 "$pid"
if [ "${PULSE_FONT_PRESET:-0}" = 1 ]; then
  font_open(){ settings_open; settings_filter "${1:-font_family}"; }
  font_click(){
    font_action=bounds
    if [ "$1" = '字体预设' ];then font_action=bounds-combo;fi
    printf 'workspace.font_step.control.%s\n' "$1"
    if ! font_bounds=$(timeout 3s sh -c 'until result=$(timeout 2s "$1" "$2" "$3" "$4"); do sleep .05; done; printf "%s\n" "$result"' sh "$native_helper" "$app" "$1" "$font_action");then timeout 3s import -window "$xid" target/probes/workspace-font-control-failure.png || true;return 1;fi
    font_x=$(printf '%s\n' "$font_bounds"|awk '{print int($1+$3/2)}')
    font_y=$(printf '%s\n' "$font_bounds"|awk '{print int($2+$4/2)}')
    timeout 2s xdotool mousemove "$font_x" "$font_y" click 1
  }
  font_pick(){
    expected=$(( $(count settings_save_confirmed) + 1 ))
    font_before=$(sha256sum "$file")
    font_open
    font_click 字体预设
    if [ "$1" = Down ];then timeout 2s xdotool key --clearmodifiers Home Down Return;else timeout 2s xdotool key --clearmodifiers Home Down Down Return;fi
    sleep .2
    [ "$(sha256sum "$file")" = "$font_before" ]
    if [ "$1" = Down ]; then
      font_click 'JetBrains Mono 字体许可'
      sleep .2
      "$native_helper" "$app" 'JetBrains Mono 字体许可' inspect >target/probes/workspace-font-license-state.txt
      awk '/expanded=1/{found=1}END{exit !found}' target/probes/workspace-font-license-state.txt
      timeout 3s import -window "$xid" target/probes/workspace-font-license.png
      [ "$(sha256sum "$file")" = "$font_before" ]
      font_click 'JetBrains Mono 字体许可'
    else
      font_click 字体名称
      timeout 2s xdotool key --clearmodifiers ctrl+a
      timeout 2s xdotool type --clearmodifiers 'PulseTerm Missing Test Font'
    fi
    font_click 保存字体名称
    await_count settings_save_confirmed "$expected"
    font_ready=$(( $(count terminal_input_ready) + 1 ))
    timeout 2s xdotool key --clearmodifiers Escape ctrl+shift+r
    await_count terminal_input_ready "$font_ready"
    timeout 2s xdotool mousemove --window "$xid" 400 300 click 1
  }
  font_pick Down
  input FONT_PRESET 8
  sleep 1
  awk '/^font_family[ ]*=/ {if(index($0,"JetBrains Mono")) ok=1} END{exit !ok}' "$file"
  timeout 3s import -window "$xid" target/probes/workspace-font-preset-output.png
  expected=$(( $(count settings_save_confirmed) + 1 ))
  font_open font_fallback
  "$native_helper" "$app" 后备字体 focus
  timeout 2s xdotool key --clearmodifiers ctrl+a
  timeout 2s xdotool type --clearmodifiers 'JetBrains Mono'
  "$native_helper" "$app" 保存后备字体 focus
  timeout 2s xdotool key --clearmodifiers space
  await_count settings_save_confirmed "$expected"
  timeout 2s xdotool key --clearmodifiers Escape
  font_pick custom
  input FONT_FALLBACK 9
  sleep 1
  awk '/^font_family[ ]*=/ {if(index($0,"PulseTerm Missing Test Font")) primary=1} /^font_fallback[ ]*=/ {if(index($0,"JetBrains Mono")) fallback=1} END{exit !(primary && fallback)}' "$file"
  [ "$(count session_opened)" -eq 1 ]
  [ "$(cat "$fixture/native.pid")" = "$pid" ];kill -0 "$pid"
  timeout 3s import -window "$xid" target/probes/workspace-font-fallback-output.png
  if [ "${PULSE_FONT_LIGATURES:-0}" = 1 ]; then
    if [ "${PULSE_LIGATURE_TRACE:-0}" = 1 ];then set -x;fi
    ligature_before=$(sha256sum "$file")
    expected=$(( $(count settings_save_confirmed) + 1 ))
    font_open font_ligatures
    font_click 运算符连字
    sleep .2
    [ "$(sha256sum "$file")" = "$ligature_before" ]
    font_click 保存运算符连字
    await_count settings_save_confirmed "$expected"
    cp -- "$file" target/probes/workspace-ligatures-saved.toml
    awk '/^[[:space:]]*(terminal[.])?font_ligatures[[:space:]]*=[[:space:]]*true([[:space:]]|$)/ {ok=1} END {exit !ok}' "$file"
    font_ready=$(( $(count terminal_input_ready) + 1 ))
    timeout 2s xdotool key --clearmodifiers Escape ctrl+shift+r
    await_count terminal_input_ready "$font_ready"
    sleep .2
    timeout 3s import -window "$xid" target/probes/workspace-ligatures-renderer-observed.png
    "$native_helper" "$app" "Renderer probe" description >target/probes/workspace-ligatures-renderer.txt
    grep -q "renderer-probe:webgl2" target/probes/workspace-ligatures-renderer.txt
    timeout 2s xdotool mousemove --window "$xid" 400 300 click 1
    input 'LIGATURES == === != !== => -> <= >= <=> && || :: := ...' 10
    [ "$(cat "$fixture/native.pid")" = "$pid" ]; kill -0 "$pid"
    [ "$(count session_opened)" -eq 1 ]
    sleep 1
    timeout 3s import -window "$xid" target/probes/workspace-ligatures-output.png
    echo workspace.ligatures_explicit_save_raw_line_same_pty_webgl_confirmed
  fi
  echo workspace.font_preset_no_write_before_save_local_license_fallback_same_pty_confirmed
fi
sleep 0.2
timeout 3s import -window "$xid" target/probes/workspace-settings-metrics-reset-output.png
timeout 2s xdotool key --clearmodifiers alt+F4
await_count exited 1;wait "$app"
if kill -0 "$pid" 2>/dev/null;then echo workspace.settings_reopened_native_still_alive >&2;exit 1;fi
if [ "${PULSE_FONT_PRESET:-0}" = 1 ];then
  saved_hash=$(sha256sum "$file"|awk '{print $1}')
  font_grid=$(tail -1 "$fixture/size")
  log=target/probes/workspace-font-reopened-native.log
  PULSETERM_SETTINGS_FIXTURE="$fixture" PATH="$fixture/bin" SHELL="$fixture/bin/sh" "$workspace_bin" > "$log" 2>&1 &
  app=$!
  await_count profiles_listed 1
  xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true)
  [ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ]
  timeout 2s xdotool windowactivate --sync "$xid"
  timeout 2s xdotool key --clearmodifiers Tab Return
  await_count terminal_input_ready 1
  timeout 2s xdotool mousemove --window "$xid" 400 300 click 1
  if [ "${PULSE_FONT_LIGATURES:-0}" = 1 ];then input LIGATURE_REOPEN 11;else input FONT_REOPEN 10;fi
  [ "$(tail -1 "$fixture/size")" = "$font_grid" ]
  [ "$(sha256sum "$file"|awk '{print $1}')" = "$saved_hash" ]
  [ "$(count session_opened)" -eq 1 ]
  pid=$(cat "$fixture/native.pid");kill -0 "$pid"
  sleep 1
  timeout 3s import -window "$xid" target/probes/workspace-font-reopened-output.png
  timeout 2s xdotool key --clearmodifiers alt+F4
  await_count exited 1;wait "$app"
  if kill -0 "$pid" 2>/dev/null;then echo workspace.font_reopened_native_still_alive >&2;exit 1;fi
  echo workspace.font_independent_reopen_same_grid_zero_write_native_exit_confirmed
fi
echo workspace.settings_metrics_independent_reopen_reset_sources_grid_native_exit_confirmed
echo workspace.settings_default_edit_reload_font_grid_confirmed
if [ "${PULSE_FONT_PRESET:-0}" = 1 ]; then
  /usr/bin/python3 - "$fixture" <<'FONT_RECEIPT'
import json, os, sys
from pathlib import Path
fixture = Path(sys.argv[1])
pids = [int(value) for value in (fixture / 'native-pids').read_text().split()]
assert len(pids) == len(set(pids)) == 3
for pid in pids:
    assert pid > 0
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        continue
    raise AssertionError('font.native_session_still_alive')
receipt = {'version': 1, 'passed': True, 'nativeExitConfirmed': True,
    'nativePids': pids, 'nativeSessionCount': 3,
    'checks': ['compiled_14px_default', '18px_native_grid', 'line_height_and_spacing',
        'independent_reopen', 'restore_inheritance', 'preset_explicit_save',
        'local_font_license', 'missing_primary_fallback', 'same_pty_after_refresh'],
    'ligatureMode': os.environ.get('PULSE_FONT_LIGATURES') == '1',
    'limitations': ['Linux Xvfb native WebView; system IME, physical DPI and other platforms remain open']}
Path('target/probes/workspace-font-result.json').write_text(json.dumps(receipt, indent=2)+'\n')
FONT_RECEIPT
fi
