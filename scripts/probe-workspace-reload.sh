#!/bin/sh
# Isolated real WebView/X11 acceptance. Never targets the developer's display.
set -eu
mkdir -p target/probes
mode=${1:-reload}
case "$mode" in reload|terminal|terminal-close|terminal-commands|terminal-cli|terminal-keymap|terminal-keymap-controls|terminal-keymap-conflict|terminal-keymap-reopen|terminal-resize|terminal-storm|terminal-refresh-resize|terminal-paced-resize|terminal-eof|terminal-eof-refresh) ;; *) echo workspace.invalid_mode >&2; exit 1;; esac
log=target/probes/workspace-reload-native.log
: > "$log"
if [ "$mode" = terminal-cli ];then : > target/probes/workspace-cli-focus-result.txt;fi
umask 077
mkdir -p /data/temp
fixture_owned=yes
if [ -n "${PULSETERM_KEYMAP_REOPEN_ROOT:-}" ];then
  fixture="$PULSETERM_KEYMAP_REOPEN_ROOT"
  python3 -c 'import os,stat,sys;from pathlib import Path;p=Path(sys.argv[1]);m=p.lstat();assert p.is_absolute() and p.parent==Path("/data/temp") and p.name.startswith("pulseterm-keymap-reopen.");assert stat.S_ISDIR(m.st_mode) and not p.is_symlink() and m.st_uid==os.geteuid() and stat.S_IMODE(m.st_mode)==0o700;assert (p/"owned-keymap-reopen.fixture").read_text()=="owned-fixture-v1"' "$fixture"
  fixture_owned=no
else
  fixture=$(mktemp -d /data/temp/pulseterm-workspace-home.XXXXXX)
fi
export HOME="$fixture/home" XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -p -m 700 "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
# Explicit controlled font baseline; production compiled default remains 14px.
mkdir -p -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
if [ "$mode" != reload ]; then
  mkdir -p -m 700 "$fixture/bin"
  cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
: "${PULSETERM_FIXTURE_PID:?missing dedicated PID path}"
printf '%s\n' "$$" > "$PULSETERM_FIXTURE_PID"
printf 'PulseTerm fixed terminal fixture\r\n'
printf '\033[31mred\033[0m \033[32mgreen\033[0m\r\n'
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$PULSETERM_FIXTURE_PID.input"
  if [ "${PULSE_CLI_SELECTION_PROBE:-0}" = 1 ];then printf '%s:%s\n' "$$" "$line" >> "$PULSETERM_FIXTURE_PID.selection";fi
  /bin/stty size >> "$PULSETERM_FIXTURE_PID.size"
  printf 'echo:%s\r\n' "$line"
  if [ "$line" = fixture-exit ]; then printf 'final-before-eof\r\n'; exit 7; fi
done
SHELL
  chmod 700 "$fixture/bin/sh"
  PULSETERM_FIXTURE_PID="$fixture/native.pid" PATH="$fixture/bin" SHELL="$fixture/bin/sh" "${CARGO_TARGET_DIR:-target}/debug/pulseterm-workspace" > "$log" 2>&1 &
else
  "${CARGO_TARGET_DIR:-target}/debug/pulseterm-workspace" > "$log" 2>&1 &
fi
app=$!
cleanup() {
  status=$?
  if [ "$status" -ne 0 ] && [ -n "${xid:-}" ] && [ "$(xdotool getwindowpid "$xid" 2>/dev/null || true)" = "$app" ];then
    timeout 3s import -window "$xid" "target/probes/workspace-$mode-failure.png" || true
    [ ! -f "$fixture/native.pid.input" ] || cp "$fixture/native.pid.input" "target/probes/workspace-$mode-failure-input.txt"
  fi
  if [ -n "${cli_request:-}" ];then kill "$cli_request" 2>/dev/null || true;wait "$cli_request" 2>/dev/null || true;fi
  if [ -n "${cli_decoy:-}" ];then kill "$cli_decoy" 2>/dev/null || true;wait "$cli_decoy" 2>/dev/null || true;fi
  kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true; if [ "$fixture_owned" = yes ];then rm -rf "$fixture";fi
}
trap cleanup EXIT
ready_expected=1
round=1
while [ "$round" -le 3 ]; do
  found=0
  attempt=0
  while [ "$attempt" -lt 80 ]; do
    kill -0 "$app" 2>/dev/null || { echo workspace.early_exit >&2; exit 1; }
    confirmed=$(grep -c '^workspace.document_confirmed$' "$log" || true)
    listed=$(grep -c '^workspace.profiles_listed$' "$log" || true)
    [ "$listed" -le "$round" ] || { echo workspace.duplicate_list >&2; exit 1; }
    if [ "$confirmed" -eq "$round" ] && [ "$listed" -eq "$round" ]; then found=1; break; fi
    [ "$confirmed" -le "$round" ] || { echo workspace.duplicate_confirmation >&2; exit 1; }
    attempt=$((attempt + 1))
    sleep 0.1
  done
  if [ "$found" -ne 1 ]; then
    # Preserve the first failed page without increasing the existing 8s allowance.
    failed_ids=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
    if [ "$(printf '%s\n' "$failed_ids" | awk 'NF {n++} END {print n+0}')" -eq 1 ] && [ "$(xdotool getwindowpid "$failed_ids")" = "$app" ]; then
      timeout 3s import -window "$failed_ids" "target/probes/workspace-$mode-startup-failure.png" || true
    fi
    cp "$log" "target/probes/workspace-$mode-startup-failure-native.log"
    if [ "$confirmed" -eq "$round" ]; then echo workspace.profile_list_timeout >&2; else echo workspace.confirm_timeout >&2;fi
    exit 1
  fi
  started=$(grep -c '^workspace.navigation_started$' "$log" || true)
  [ "$started" -eq "$round" ] || { echo workspace.navigation_missing >&2; exit 1; }
  ids=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
  [ "$(printf '%s\n' "$ids" | awk 'NF {n++} END {print n+0}')" -eq 1 ] || { echo workspace.ambiguous_window >&2; exit 1; }
  [ "$(xdotool getwindowpid "$ids")" = "$app" ] || { echo workspace.foreign_window >&2; exit 1; }
  if [ "$round" -eq 1 ]; then xid=$ids; else [ "$ids" = "$xid" ] || { echo workspace.replaced_window >&2; exit 1; }; fi
  if [ "$mode" != reload ]; then
    if [ "$round" -eq 1 ]; then
      attempt=0
      while [ "$(grep -c '^workspace.sessions_listed$' "$log" || true)" -lt 1 ] && [ "$attempt" -lt 80 ]; do attempt=$((attempt+1)); sleep 0.1; done
      timeout 2s xdotool windowactivate --sync "$ids"
      # Discovery also appends /usr/local/bin,/usr/bin,/bin. Count preceding
      # known kinds using metadata only, then keyboard-select the fixture sh.
      # This isolated database starts empty: no hidden/deletion preferences.
      tabs=0
      for kind in bash zsh fish; do
        present=0
        for directory in /usr/local/bin /usr/bin /bin; do
          if [ -f "$directory/$kind" ] && [ -x "$directory/$kind" ]; then present=1; fi
        done
        tabs=$((tabs+present))
      done
      while [ "$tabs" -gt 0 ]; do timeout 2s xdotool key --clearmodifiers Tab; tabs=$((tabs-1)); done
      timeout 2s xdotool key --clearmodifiers Return
    fi
    attempt=0
    while [ "$(grep -c '^workspace.terminal_capture_confirmed$' "$log" || true)" -lt "$round" ] && [ "$attempt" -lt 80 ]; do
      kill -0 "$app" 2>/dev/null || { echo workspace.terminal_early_exit >&2; exit 1; }
      attempt=$((attempt+1)); sleep 0.1
    done
    [ "$(grep -c '^workspace.terminal_capture_confirmed$' "$log" || true)" -ge "$round" ] || { echo workspace.terminal_capture_timeout >&2; exit 1; }
    attempt=0
    while [ ! -f "$fixture/native.pid" ] && [ "$attempt" -lt 80 ]; do attempt=$((attempt+1)); sleep 0.1; done
    [ -f "$fixture/native.pid" ] || { echo workspace.native_pid_missing >&2; exit 1; }
    current_pid=$(cat "$fixture/native.pid")
    if [ "$round" -eq 1 ]; then native_pid=$current_pid; else [ "$current_pid" = "$native_pid" ] || { echo workspace.native_restarted >&2; exit 1; }; fi
    kill -0 "$current_pid" 2>/dev/null || { echo workspace.native_early_eof >&2; exit 1; }
  fi
  if [ "$mode" != reload ]; then
    timeout 2s xdotool windowactivate --sync "$ids"
    attempt=0
    while [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -lt "$ready_expected" ] && [ "$attempt" -lt 80 ]; do attempt=$((attempt+1)); sleep 0.1; done
    [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -eq "$ready_expected" ] || { echo workspace.input_ready_timeout >&2; exit 1; }
    if [ "$mode" = terminal-keymap-controls ] && [ "$round" -gt 1 ];then
      # Completion from a prefix armed before the real reload must not become a new-page command.
      # The reload gesture itself can retire the prefix; this is not proof of navigation-only isolation.
      page_settings=$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)
      timeout 2s xdotool mousemove --window "$ids" 916 20 click 1
      attempt=0;while [ "$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)" -lt "$((page_settings+1))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_new_page_focus_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
      sleep 0.2;timeout 2s xdotool key --clearmodifiers Escape
      timeout 3s import -window "$ids" "target/probes/keymap-nav-before-$round.png"
      before_nav_pixel=$(convert "target/probes/keymap-nav-before-$round.png" -format '%[pixel:p{10,10}]' info:)
      timeout 2s xdotool key --clearmodifiers ctrl+shift+y
      sleep 0.1;timeout 3s import -window "$ids" "target/probes/keymap-nav-after-$round.png"
      after_nav_pixel=$(convert "target/probes/keymap-nav-after-$round.png" -format '%[pixel:p{10,10}]' info:)
      [ "$before_nav_pixel" = "$after_nav_pixel" ] || { echo workspace.keymap_old_page_completion_opened_modal >&2;exit 1; }
      timeout 2s xdotool mousemove --window "$ids" 430 240 click 1
    fi
    if [ "$mode" = terminal-keymap ] || [ "$mode" = terminal-keymap-controls ] || [ "$mode" = terminal-keymap-conflict ] || [ "$mode" = terminal-keymap-reopen ];then
      before_settings=$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)
      if [ "$round" -eq 1 ] && [ "$mode" != terminal-keymap-reopen ];then
        before_keymap=$(grep -c '^workspace.keymap_confirm_returned$' "$log" || true)
        timeout 2s xdotool key --clearmodifiers ctrl+shift+p;sleep 0.2
        timeout 2s xdotool type --clearmodifiers --delay 1 settings
        timeout 2s xdotool key --clearmodifiers Return
        attempt=0;while [ "$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)" -lt "$((before_settings+1))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_settings_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
        sleep 0.2;timeout 3s import -window "$ids" target/probes/keymap-gui-settings.png
        # Find the production entry; search -> category -> clear -> keymap.
        timeout 2s xdotool type --clearmodifiers --delay 1 keymap
        timeout 2s xdotool key --clearmodifiers Tab Tab Tab Return
        attempt=0;while [ "$(grep -c '^workspace.keymap_confirm_returned$' "$log" || true)" -lt "$((before_keymap+1))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_editor_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
        # Require an actual painted form; native confirmation precedes frontend rendering.
        timeout 8s sh -c 'while :; do import -window "$1" target/probes/keymap-gui-form.png; tesseract target/probes/keymap-gui-form.png stdout --psm 6 2>/dev/null | grep -E "workspaceFocus.*editableFocus.*modal" >/dev/null && exit 0; sleep 0.1; done' sh "$ids" || { echo workspace.keymap_form_paint_timeout >&2;exit 1; }
        # Native dialog focuses Done while the asynchronous form is populated.
        # The independently inspected form shows the palette textarea at this fixed client point.
        # Choose its alias by actual mouse focus, not a guessed asynchronous Tab order.
        timeout 2s xdotool mousemove --window "$ids" 450 280 click 1
        timeout 2s xdotool key --clearmodifiers ctrl+a
        timeout 2s xdotool type --clearmodifiers --delay 1 ctrl+shift+y
        timeout 2s xdotool key --clearmodifiers Tab Tab space Tab Return
        attempt=0;while [ "$(grep -c '^workspace.keymap_save_returned$' "$log" || true)" -lt 1 ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_gui_save_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
        attempt=0;while [ "$(grep -c '^workspace.keymap_confirm_returned$' "$log" || true)" -lt "$((before_keymap+2))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_saved_confirm_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
        sleep 0.2;timeout 3s import -window "$ids" target/probes/keymap-gui-saved.png
        grep -F 'ctrl+shift+y' "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/keymap.toml" > /dev/null
        cp "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/keymap.toml" target/probes/keymap-gui-saved.toml
        if [ "$mode" = terminal-keymap-conflict ];then
          keymap_file="$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/keymap.toml"
          # Independent atomic writer changes the complete image while the confirmed GUI ticket is held.
          python3 -c 'import os,sys; from pathlib import Path;p=Path(sys.argv[1]);q=p.with_name("keymap.external-fixture");q.write_text(p.read_text().replace("ctrl+shift+y","ctrl+shift+z"));q.chmod(0o600);os.replace(q,p)' "$keymap_file"
          cp "$keymap_file" target/probes/keymap-conflict-newer.toml
          timeout 2s xdotool mousemove --window "$ids" 450 430 click --repeat 6 --delay 50 5
          timeout 2s xdotool mousemove --window "$ids" 233 426 click 1
          timeout 2s xdotool mousemove --window "$ids" 264 461 click 1
          attempt=0;while ! grep -q '^workspace.keymap_save_failed.workspace.keymap_stale$' "$log";do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_conflict_not_rejected >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
          cmp "$keymap_file" target/probes/keymap-conflict-newer.toml || { echo workspace.keymap_conflict_overwrote_newer >&2;exit 1; }
          timeout 3s import -window "$ids" target/probes/keymap-conflict-rejected.png
          timeout 2s xdotool key --clearmodifiers Escape
          reread=$(grep -c '^workspace.keymap_confirm_returned$' "$log" || true)
          # Escape returns to the same filtered settings and its entry button.
          timeout 8s sh -c 'while :;do import -window "$1" target/probes/keymap-conflict-settings.png;tesseract target/probes/keymap-conflict-settings.png stdout --psm 6 2>/dev/null|grep -F "keymap" >/dev/null&&exit 0;sleep 0.1;done' sh "$ids"
          timeout 2s xdotool key --clearmodifiers Return
          attempt=0;while [ "$(grep -c '^workspace.keymap_confirm_returned$' "$log" || true)" -lt "$((reread+1))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_conflict_reread_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
          timeout 8s sh -c 'while :;do import -window "$1" target/probes/keymap-conflict-reloaded.png;tesseract target/probes/keymap-conflict-reloaded.png stdout --psm 6 2>/dev/null|grep -E "workspaceFocus.*editableFocus.*modal" >/dev/null&&exit 0;sleep 0.1;done' sh "$ids"
        fi
        if [ "$mode" = terminal-keymap-controls ];then
          keymap_file="$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/keymap.toml"
          # Save receipt restores Done focus and form scroll; next consent is always fresh.
          timeout 2s xdotool mousemove --window "$ids" 450 430 click --repeat 6 --delay 50 5
          timeout 3s import -window "$ids" target/probes/keymap-controls-consent-before.png
          timeout 2s xdotool mousemove --window "$ids" 233 426 click 1
          timeout 3s import -window "$ids" target/probes/keymap-controls-consent-after.png
          timeout 2s xdotool mousemove --window "$ids" 345 461 click 1
          attempt=0;while [ "$(grep -c '^workspace.keymap_save_returned$' "$log" || true)" -lt 2 ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_disable_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
          timeout 8s sh -c 'while :;do import -window "$1" target/probes/keymap-gui-disabled.png;tesseract target/probes/keymap-gui-disabled.png stdout --psm 6 2>/dev/null|grep -E "workspaceFocus.*editableFocus.*modal" >/dev/null&&exit 0;sleep 0.1;done' sh "$ids"
          grep -F 'keys = []' "$keymap_file" >/dev/null
          cp "$keymap_file" target/probes/keymap-gui-disabled.toml
          timeout 2s xdotool mousemove --window "$ids" 450 430 click --repeat 6 --delay 50 5
          timeout 2s xdotool mousemove --window "$ids" 233 426 click 1
          timeout 2s xdotool mousemove --window "$ids" 419 461 click 1
          attempt=0;while [ "$(grep -c '^workspace.keymap_save_returned$' "$log" || true)" -lt 3 ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_restore_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
          timeout 8s sh -c 'while :;do import -window "$1" target/probes/keymap-gui-restored.png;tesseract target/probes/keymap-gui-restored.png stdout --psm 6 2>/dev/null|grep -E "workspaceFocus.*editableFocus.*modal" >/dev/null&&exit 0;sleep 0.1;done' sh "$ids"
          ! grep -F 'action = "palette"' "$keymap_file" >/dev/null || { echo workspace.keymap_restore_did_not_remove_override >&2;exit 1; }
          cp "$keymap_file" target/probes/keymap-gui-restored.toml
          timeout 2s xdotool mousemove --window "$ids" 450 280 click 1
          timeout 2s xdotool key --clearmodifiers ctrl+a
          timeout 2s xdotool type --clearmodifiers --delay 1 'ctrl+shift+k ctrl+shift+y'
          timeout 2s xdotool key --clearmodifiers Tab Tab space Tab Return
          attempt=0;while [ "$(grep -c '^workspace.keymap_save_returned$' "$log" || true)" -lt 4 ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_sequence_save_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
          attempt=0;while [ "$(grep -c '^workspace.keymap_confirm_returned$' "$log" || true)" -lt "$((before_keymap+5))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_sequence_confirm_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
          cp "$keymap_file" target/probes/keymap-gui-sequence.toml
        fi
        timeout 2s xdotool key --clearmodifiers Escape
        # First Escape returns to settings; the second returns to the terminal.
        sleep 0.2
        timeout 2s xdotool key --clearmodifiers Escape
        before_settings=$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)
      fi
      if [ "$mode" = terminal-keymap-controls ];then
        # Real A→B→A header focus transitions must retire a prefix even when context is equal again.
        focus_settings=$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)
        timeout 2s xdotool mousemove --window "$ids" 916 20 click 1
        attempt=0;while [ "$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)" -lt "$((focus_settings+1))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_focus_settings_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
        sleep 0.2;timeout 2s xdotool key --clearmodifiers Escape
        timeout 3s import -window "$ids" "target/probes/keymap-focus-before-$round.png"
        before_pixel=$(convert "target/probes/keymap-focus-before-$round.png" -format '%[pixel:p{10,10}]' info:)
        timeout 2s xdotool key --clearmodifiers ctrl+shift+k shift+Tab Tab ctrl+shift+y
        sleep 0.1;timeout 3s import -window "$ids" "target/probes/keymap-focus-after-$round.png"
        after_pixel=$(convert "target/probes/keymap-focus-after-$round.png" -format '%[pixel:p{10,10}]' info:)
        [ "$before_pixel" = "$after_pixel" ] || { echo workspace.keymap_retired_focus_opened_modal >&2;exit 1; }
        before_settings=$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)
        timeout 2s xdotool mousemove --window "$ids" 430 240 click 1
        timeout 2s xdotool key --clearmodifiers ctrl+shift+k
      fi
      if [ "$mode" = terminal-keymap-conflict ];then alias=ctrl+shift+z;else alias=ctrl+shift+y;fi
      timeout 2s xdotool key --clearmodifiers "$alias";sleep 0.2
      timeout 2s xdotool type --clearmodifiers --delay 1 settings
      timeout 2s xdotool key --clearmodifiers Return
      attempt=0;while [ "$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)" -lt "$((before_settings+1))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.keymap_custom_action_timeout >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
      timeout 2s xdotool key --clearmodifiers Escape
      timeout 2s xdotool mousemove --window "$ids" 430 240 click 1
    fi
    if [ "$mode" = terminal-commands ];then
      before_settings=$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)
      timeout 2s xdotool key --clearmodifiers ctrl+shift+p;sleep 0.2
      if [ "$round" -eq 1 ];then timeout 3s import -window "$ids" target/probes/workspace-terminal-commands-menu.png;fi
      if [ "$round" -eq 2 ];then command_query=keyboard;else command_query=settings;fi
      before_keymap=$(grep -c '^workspace.keymap_confirm_returned$' "$log" || true)
      timeout 2s xdotool type --clearmodifiers --delay 1 "$command_query"
      timeout 2s xdotool key --clearmodifiers Return
      attempt=0
      while [ "$(grep -c '^workspace.settings_read_confirmed$' "$log" || true)" -lt "$((before_settings+1))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.command_settings_not_admitted >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
      sleep 0.2
      timeout 2s xdotool key --clearmodifiers ctrl+shift+p
      timeout 2s xdotool key --clearmodifiers Escape
      if [ "$round" -eq 2 ];then
        attempt=0
        while [ "$(grep -c '^workspace.keymap_confirm_returned$' "$log" || true)" -lt "$((before_keymap+1))" ];do [ "$attempt" -lt 80 ]||{ echo workspace.command_keymap_not_admitted >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
        timeout 2s xdotool key --clearmodifiers Escape
      fi
      timeout 2s xdotool key --clearmodifiers ctrl+shift+p;sleep 0.2
      timeout 2s xdotool type --clearmodifiers --delay 1 unmatched-command
      timeout 2s xdotool key --clearmodifiers Return
      timeout 2s xdotool key --clearmodifiers Escape
      # The following exact real-PTY line check detects shortcut/search bytes leaking.
    fi
    if { [ "$mode" = terminal-resize ] || [ "$mode" = terminal-storm ] || [ "$mode" = terminal-paced-resize ]; } && [ "$round" -gt 1 ]; then
      ready_expected=$((ready_expected+1))
      if [ "$round" -eq 2 ]; then width=840; height=560; else width=1000; height=720; fi
      if [ "$mode" = terminal-paced-resize ]; then
        # Preserve the original separate-controller bootstrap counterexample.
        for shape in 900x620 880x600 960x640 820x520 940x660 860x580 910x630 830x550; do
          w=${shape%x*}; h=${shape#*x}
          timeout 2s xdotool windowsize --sync "$ids" "$w" "$h"
        done
        timeout 2s xdotool windowsize --sync "$ids" "$width" "$height"
      elif [ "$mode" = terminal-storm ]; then
        # Same nine actions in one controller process: a finite event batch,
        # not a scheduler-paced continuous drag or throughput benchmark.
        timeout 3s xdotool windowsize --sync "$ids" 900 620 windowsize --sync "$ids" 880 600 windowsize --sync "$ids" 960 640 windowsize --sync "$ids" 820 520 windowsize --sync "$ids" 940 660 windowsize --sync "$ids" 860 580 windowsize --sync "$ids" 910 630 windowsize --sync "$ids" 830 550 windowsize --sync "$ids" "$width" "$height"
      else
        timeout 2s xdotool windowsize --sync "$ids" "$width" "$height"
      fi

      attempt=0
      while [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -lt "$ready_expected" ] && [ "$attempt" -lt 80 ]; do attempt=$((attempt+1)); sleep 0.1; done
      [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -eq "$ready_expected" ] || { echo workspace.resize_ready_timeout >&2; exit 1; }
      kill -0 "$current_pid" 2>/dev/null || { echo workspace.resize_killed_native >&2; exit 1; }
    fi
    if [ "$mode" = terminal-cli ]; then
      opened_before=$(grep -c '^workspace.session_opened$' "$log" || true)
      timeout 8s "${CARGO_TARGET_DIR:-target}/debug/pulse" profiles > "target/probes/workspace-cli-profiles-$round.json" || { echo workspace.cli_profiles_failed >&2;exit 1; }
      python3 - "$round" <<'PY'
import json,sys
from pathlib import Path
v=json.loads(Path(f'target/probes/workspace-cli-profiles-{sys.argv[1]}.json').read_text())
assert set(v)=={'version','profiles'} and v['version']==1
assert isinstance(v['profiles'],list) and 'sh' in v['profiles']
assert len(v['profiles'])==len(set(v['profiles']))<=5
assert set(v['profiles'])<= {'bash','zsh','fish','sh','pwsh'}
PY
      [ "$(grep -c '^workspace.session_opened$' "$log" || true)" = "$opened_before" ] || { echo workspace.cli_discovery_started_shell >&2;exit 1; }
      xmessage -name PulseCliDecoy -buttons OK:0 "Controlled focus counterexample" >/dev/null 2>&1 &
      cli_decoy=$!
      attempt=0;decoy_id=
      while [ -z "$decoy_id" ] && [ "$attempt" -lt 40 ];do
        decoy_id=$(xdotool search --onlyvisible --name "^PulseCliDecoy$" 2>/dev/null | head -1 || true)
        attempt=$((attempt+1));sleep 0.05
      done
      [ -n "$decoy_id" ] || { echo workspace.cli_decoy_missing >&2;exit 1; }
      timeout 2s xdotool windowactivate --sync "$decoy_id"
      [ "$(xdotool getactivewindow)" = "$decoy_id" ] || { echo workspace.cli_not_defocused >&2;exit 1; }
      if [ "${PULSE_CLI_SINGLE_INSTANCE_PROBE:-0}" = 1 ];then
        . scripts/probe-workspace-single-instance.sh
      fi
      case ${PULSE_CLI_WINDOW_PROBE:-0} in
        0) result=$(timeout 8s "${CARGO_TARGET_DIR:-target}/debug/pulse" focus) ;;
        1) result=$(timeout 8s "${CARGO_TARGET_DIR:-target}/debug/pulse" -w workspace focus) ;;
        *) echo workspace.cli_invalid_window_probe >&2;exit 1 ;;
      esac || { echo workspace.cli_focus_failed >&2;exit 1; }
      [ "$result" = "focus requested" ] || { echo workspace.cli_wrong_receipt >&2;exit 1; }
      attempt=0
      while [ "$(xdotool getactivewindow)" != "$ids" ] && [ "$attempt" -lt 40 ];do attempt=$((attempt+1));sleep 0.05;done
      [ "$(xdotool getactivewindow)" = "$ids" ] || { echo workspace.cli_compositor_focus_missing >&2;exit 1; }
      printf "round=%s old_focus=%s restored_focus=%s native_pid=%s\n" "$round" "$decoy_id" "$ids" "$current_pid" >> target/probes/workspace-cli-focus-result.txt
      kill "$cli_decoy";wait "$cli_decoy" 2>/dev/null || true;unset cli_decoy
    fi
    if [ "$mode" != terminal-refresh-resize ] || [ "$round" -eq 3 ]; then
      input_rounds="$round"
      if [ "$mode" = terminal-refresh-resize ]; then input_rounds="1 2 3"; fi
      for input_round in $input_rounds; do
    # Explicit frontend focus/queue-installed receipt, not a guessed delay.
    timeout 2s xdotool type --clearmodifiers --delay 1 "keyboard-round-$input_round"
    timeout 2s xdotool key --clearmodifiers Return
    attempt=0
    while [ ! -f "$fixture/native.pid.input" ] || [ "$(wc -l < "$fixture/native.pid.input")" -lt "$input_round" ]; do
      [ "$attempt" -lt 80 ] || { echo workspace.keyboard_timeout >&2; exit 1; }
      attempt=$((attempt+1)); sleep 0.1
    done
    [ "$(tail -n 1 "$fixture/native.pid.input")" = "keyboard-round-$input_round" ] || { echo workspace.keyboard_mismatch >&2; exit 1; }
    [ "$(wc -l < "$fixture/native.pid.input")" -eq "$input_round" ] || { echo workspace.keyboard_duplicate >&2; exit 1; }
    if [ "$mode" = terminal-resize ] || [ "$mode" = terminal-storm ] || [ "$mode" = terminal-refresh-resize ] || [ "$mode" = terminal-paced-resize ]; then
      attempt=0
      while [ ! -f "$fixture/native.pid.size" ] || [ "$(wc -l < "$fixture/native.pid.size")" -lt "$input_round" ]; do [ "$attempt" -lt 80 ] || { echo workspace.size_receipt_missing >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
      physical=$(sed -n "${input_round}p" "$fixture/native.pid.size")
      committed=$(grep '^workspace.terminal_resized.' "$log" | tail -n 1 | awk -F . '{print $4 " " $3}')
      [ "$physical" = "$committed" ] || { echo workspace.size_actor_kernel_mismatch >&2; exit 1; }
    fi
      done
    fi

  fi
  if [ "$mode" = terminal-refresh-resize ] && [ "$round" -lt 3 ]; then
    pending=$(grep -c '^workspace.terminal_confirm_waiting$' "$log" || true)
    previous_ready=$(grep -c '^workspace.terminal_input_ready$' "$log" || true)
    if [ "$round" -eq 1 ]; then width=840; height=560; else width=1000; height=720; fi
    timeout 2s xdotool windowsize --sync "$ids" "$width" "$height"
    attempt=0
    while [ "$(grep -c '^workspace.terminal_confirm_waiting$' "$log" || true)" -le "$pending" ] && [ "$attempt" -lt 80 ]; do attempt=$((attempt+1)); sleep 0.01; done
    [ "$(grep -c '^workspace.terminal_confirm_waiting$' "$log" || true)" -gt "$pending" ] || { echo workspace.capture_pending_missing >&2; exit 1; }
    [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -eq "$previous_ready" ] || { echo workspace.refresh_not_inflight >&2; exit 1; }
    # Refresh while the confirmation is actually pending; no photography delay.
    timeout 2s xdotool windowactivate --sync "$ids"
    timeout 2s xdotool key --clearmodifiers ctrl+shift+r
  fi
  if grep -q '^workspace.terminal_failure.' "$log"; then echo workspace.failed_surface >&2; exit 1; fi
  # Independent pixels, not a native return-value claim of DOM drawing.
  if [ "$mode" != terminal-refresh-resize ]; then
    sleep 0.2
    timeout 3s import -window "$ids" "target/probes/workspace-$mode-round-$round.png"
  fi
  # Refresh mode verifies the complete final fourth-page photograph instead;
  # per-round photography would consume the actual pending-comparison interval.
  if [ "$mode" != reload ]; then ready_expected=$((ready_expected+1)); fi
  if [ "$round" -lt 3 ] && [ "$mode" != terminal-refresh-resize ]; then
    timeout 2s xdotool windowactivate --sync "$ids"
    # Explicit standard reload shortcut; Profile buttons must not be activated.
    if [ "$mode" = terminal-keymap-controls ];then timeout 2s xdotool key --clearmodifiers ctrl+shift+k;fi
    timeout 2s xdotool key --clearmodifiers ctrl+shift+r
  fi
  round=$((round + 1))
done
kill -0 "$app" 2>/dev/null || { echo workspace.early_exit >&2; exit 1; }
[ "$(grep -c '^workspace.document_confirmed$' "$log")" -eq 3 ]
[ "$(grep -c '^workspace.navigation_started$' "$log")" -eq 3 ]
[ "$(grep -c '^workspace.profiles_listed$' "$log")" -eq 3 ]
[ -f "$XDG_DATA_HOME/dev.pulseterm.workspace-preview/state.db" ] || { echo workspace.database_missing >&2; exit 1; }
if [ "$mode" = terminal-resize ] || [ "$mode" = terminal-storm ] || [ "$mode" = terminal-refresh-resize ] || [ "$mode" = terminal-paced-resize ]; then
  attempt=0
  while [ ! -f "$fixture/native.pid.size" ] || [ "$(wc -l < "$fixture/native.pid.size")" -lt 3 ]; do [ "$attempt" -lt 80 ] || { echo workspace.size_receipt_missing >&2; exit 1; }; attempt=$((attempt+1)); sleep 0.1; done
  [ "$(wc -l < "$fixture/native.pid.size")" -eq 3 ]
  if [ "$mode" != terminal-refresh-resize ]; then
  [ "$(sort -u "$fixture/native.pid.size" | wc -l)" -eq 3 ] || { echo workspace.size_unchanged >&2; exit 1; }
  fi
  [ "$(grep -c '^workspace.terminal_resized.' "$log" || true)" -ge 3 ] || { echo workspace.resize_native_receipt_missing >&2; exit 1; }
  cp "$fixture/native.pid.size" "target/probes/workspace-$mode-pty-size.txt"
  echo workspace.native_resize_three_confirmed
fi
if [ "$mode" = terminal-refresh-resize ]; then
  [ "$(grep -c '^workspace.terminal_confirm_retired$' "$log" || true)" -eq 2 ] || { echo workspace.pending_capture_not_retired >&2; exit 1; }
  echo workspace.resize_pending_refresh_two_confirmed
fi
echo workspace.reload_three_confirmed
if [ "$mode" != reload ]; then
  # The keyboard echo pipeline may still be painting at the per-round photograph.
  # Re-capture after all three fixed lines reached the PTY: inspect the authoritative
  # complete image instead of claiming the third partial live screenshot is final.
  timeout 2s xdotool windowactivate --sync "$xid"
  timeout 2s xdotool key --clearmodifiers ctrl+shift+r
  attempt=0
  while [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -lt "$ready_expected" ] && [ "$attempt" -lt 80 ]; do attempt=$((attempt+1)); sleep 0.1; done
  [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -eq "$ready_expected" ] || { echo workspace.final_capture_timeout >&2; exit 1; }
  ! grep -q '^workspace.terminal_failure.' "$log" || { echo workspace.failed_final_capture >&2; exit 1; }
  [ "$(wc -l < "$fixture/native.pid.input")" -eq 3 ] || { echo workspace.extra_keyboard >&2; exit 1; }
  kill -0 "$native_pid" 2>/dev/null || { echo workspace.native_lost_final_capture >&2; exit 1; }
  timeout 3s import -window "$xid" "target/probes/workspace-$mode-final.png"
fi
if [ "$mode" = terminal-eof ] || [ "$mode" = terminal-eof-refresh ]; then
  timeout 2s xdotool type --clearmodifiers --delay 1 'fixture-exit'
  timeout 2s xdotool key --clearmodifiers Return
  attempt=0
  while [ "$(grep -c '^workspace.terminal_eof_acked$' "$log" || true)" -lt 1 ] && [ "$attempt" -lt 80 ]; do
    ! grep -q '^workspace.terminal_failure.' "$log" || { echo workspace.failed_eof >&2; exit 1; }
    attempt=$((attempt+1));sleep 0.1
  done
  [ "$(grep -c '^workspace.terminal_eof_acked$' "$log" || true)" -eq 1 ] || { echo workspace.eof_ack_missing >&2;exit 1; }
  [ "$(wc -l < "$fixture/native.pid.input")" -eq 4 ] || { echo workspace.eof_input_count >&2;exit 1; }
  ! kill -0 "$native_pid" 2>/dev/null || { echo workspace.native_eof_not_reaped >&2;exit 1; }
  if [ "$mode" = terminal-eof-refresh ]; then
    for expected in 2 3; do
      timeout 2s xdotool key --clearmodifiers ctrl+shift+r
      attempt=0
      while [ "$(grep -c '^workspace.terminal_eof_acked$' "$log" || true)" -lt "$expected" ] && [ "$attempt" -lt 80 ]; do
        ! grep -q '^workspace.terminal_failure.' "$log" || { echo workspace.failed_eof_refresh >&2;exit 1; }
        attempt=$((attempt+1));sleep 0.1
      done
      [ "$(grep -c '^workspace.terminal_eof_acked$' "$log" || true)" -eq "$expected" ] || { echo workspace.eof_refresh_ack_missing >&2;exit 1; }
      [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -eq "$ready_expected" ] || { echo workspace.input_reenabled_after_eof >&2;exit 1; }
      [ "$(grep -c '^workspace.session_opened$' "$log" || true)" -eq 1 ] || { echo workspace.eof_restarted_shell >&2;exit 1; }
    done
    echo workspace.ended_refresh_two_confirmed
  fi
  timeout 2s xdotool type --clearmodifiers --delay 1 'after-eof'
  timeout 2s xdotool key --clearmodifiers Return
  [ "$(wc -l < "$fixture/native.pid.input")" -eq 4 ] || { echo workspace.input_after_eof >&2;exit 1; }
  timeout 3s import -window "$xid" "target/probes/workspace-$mode-final.png"
  echo workspace.natural_eof_rendered_confirmed
fi
if [ "$mode" = terminal-close ] || [ "$mode" = terminal-commands ] || [ "$mode" = terminal-eof ] || [ "$mode" = terminal-eof-refresh ]; then
  # Fixed 960x640 Xvfb client: the actual visible Close Session button. Require
  # backend native cleanup receipt, never infer success from the mouse command.
  [ "$(xdotool getwindowpid "$xid")" = "$app" ] || { echo workspace.foreign_session_close >&2; exit 1; }
  timeout 2s xdotool windowactivate --sync "$xid"
  if [ "$mode" = terminal-commands ];then
    timeout 2s xdotool key --clearmodifiers ctrl+shift+p;sleep 0.2
    timeout 2s xdotool type --clearmodifiers --delay 1 close
    timeout 2s xdotool key --clearmodifiers Return;sleep 0.2
    timeout 2s xdotool key --clearmodifiers space;sleep 0.2
    kill -0 "$native_pid";[ "$(grep -c '^workspace.session_closed$' "$log" || true)" -eq 0 ]
    timeout 2s xdotool key --clearmodifiers ctrl+shift+p;sleep 0.2
    timeout 2s xdotool type --clearmodifiers --delay 1 close
    timeout 2s xdotool key --clearmodifiers Return;sleep 0.2
  else
    timeout 2s xdotool mousemove --window "$xid" 900 80 click 1
  fi
  timeout 2s xdotool key --clearmodifiers Tab Return
  attempt=0
  while [ "$(grep -c '^workspace.session_closed$' "$log" || true)" -lt 1 ] && [ "$attempt" -lt 80 ]; do attempt=$((attempt+1)); sleep 0.1; done
  [ "$(grep -c '^workspace.session_closed$' "$log" || true)" -eq 1 ] || { echo workspace.session_close_timeout >&2; exit 1; }
  ! kill -0 "$native_pid" 2>/dev/null || { echo workspace.native_session_close_not_reaped >&2; exit 1; }
  if [ "$mode" = terminal-eof ] || [ "$mode" = terminal-eof-refresh ]; then
    [ "$(grep -c '^workspace.session_exit.7.false$' "$log" || true)" -eq 1 ] || { echo workspace.eof_native_exit_mismatch >&2;exit 1; }
  fi
  echo workspace.explicit_session_close_confirmed
fi
expected_sessions=1
if [ "$mode" = terminal-cli ] && [ "${PULSE_CLI_NEW_TAB_PROBE:-0}" = 1 ];then
  . scripts/probe-workspace-cli-new-tab.sh
  expected_sessions=2
fi
# WM_DELETE_WINDOW via the independent WM, not XDestroyWindow or SIGTERM.
[ "$(xdotool getwindowpid "$xid")" = "$app" ] || { echo workspace.foreign_close >&2; exit 1; }
timeout 2s xdotool windowactivate --sync "$xid"
timeout 2s xdotool key --clearmodifiers alt+F4
timeout 8s tail --pid="$app" --sleep-interval=0.1 -f /dev/null || { echo workspace.drain_timeout >&2; exit 1; }
wait "$app" || { echo workspace.exit_failed >&2; exit 1; }
for phase in shutdown_started native_drained profiles_drained shutdown_complete exited; do
  [ "$(grep -c "^workspace.$phase$" "$log" || true)" -eq 1 ] || { echo "workspace.missing_or_duplicate_$phase" >&2; exit 1; }
done
awk '/^workspace.shutdown_started$/ {s=NR} /^workspace.native_drained$/ {n=NR} /^workspace.profiles_drained$/ {p=NR} /^workspace.shutdown_complete$/ {c=NR} /^workspace.exited$/ {e=NR} END {exit !(s<n && s<p && n<c && p<c && c<e)}' "$log" || { echo workspace.exit_order >&2; exit 1; }
if [ "$mode" = terminal-cli ];then
  [ ! -e "$XDG_RUNTIME_DIR/pulseterm-workspace-cli/control.sock" ] || { echo workspace.cli_socket_survived_shutdown >&2;exit 1; }
  [ ! -e "$XDG_RUNTIME_DIR/pulseterm-workspace-cli/control.token" ] || { echo workspace.cli_token_survived_shutdown >&2;exit 1; }
  if timeout 7s "${CARGO_TARGET_DIR:-target}/debug/pulse" focus >/dev/null 2>&1;then echo workspace.cli_reconnected_after_exit >&2;exit 1;fi
  echo workspace.cli_three_process_focus_and_native_drain_confirmed
fi
if [ "$mode" != reload ]; then
  [ "$(grep -c '^workspace.session_opened$' "$log" || true)" -eq "$expected_sessions" ] || { echo workspace.duplicate_shell >&2; exit 1; }
  if [ -n "${cli_native_pid:-}" ];then ! kill -0 "$cli_native_pid" 2>/dev/null || { echo workspace.cli_native_not_reaped >&2;exit 1; };fi
  ! kill -0 "$native_pid" 2>/dev/null || { echo workspace.native_not_reaped >&2; exit 1; }
  [ "$(grep -c '^workspace.profile_selected.sh$' "$log" || true)" -eq 1 ] || { echo workspace.wrong_profile >&2; exit 1; }
  echo workspace.terminal_keyboard_three_confirmed
fi
! grep -q '^workspace.terminal_failure.' "$log" || { echo workspace.failed_after_input >&2; exit 1; }
archive="target/probes/workspace-$mode-native.log"
if [ "$log" != "$archive" ]; then cp "$log" "$archive"; fi
echo workspace.ordered_shutdown_confirmed
