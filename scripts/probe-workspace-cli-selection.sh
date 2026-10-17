#!/bin/sh
# Sourced by the private two-PTY CLI fixture; never selects a user's session.
[ "${PULSE_CLI_SELECTION_PROBE:-0}" = 1 ]
[ "$(cat "target/probes/$cli_name-$cli_requests.txt")" = 'session 2' ]
selection_name=cli-selection-tabs
if [ -n "${cli_split_flag:-}" ];then selection_name=cli-selection-split-${PULSE_CLI_SPLIT_PROBE};fi
# Canonical but absent targets fail without changing the root collection.
status=0;timeout 7s target/debug/pulse focus-tab 18446744073709551615 > "target/probes/$selection_name-absent.txt" 2>&1 || status=$?
[ "$status" -eq 1 ]
selection_round=0
selection_ready=$(grep -c '^workspace.terminal_input_ready$' "$log" || true)
for selection_id in 1 2 2 1 2;do
 selection_round=$((selection_round+1))
 timeout 7s target/debug/pulse focus-tab "$selection_id" > "target/probes/$selection_name-$selection_round.txt" 2>&1
 [ "$(cat "target/probes/$selection_name-$selection_round.txt")" = "selected session $selection_id" ]
 if [ "$selection_id" = 1 ];then selection_pid=$native_pid;else selection_pid=$cli_native_pid;fi
 kill -0 "$native_pid";kill -0 "$cli_native_pid"
 marker="$selection_name-$selection_round"
 timeout 2s xdotool type --clearmodifiers --delay 1 "$marker"
 timeout 2s xdotool key --clearmodifiers Return
 attempt=0
 while ! grep -qx "$selection_pid:$marker" "$fixture/native.pid.selection" && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
 grep -qx "$selection_pid:$marker" "$fixture/native.pid.selection"
 [ "$(grep -c ":$marker$" "$fixture/native.pid.selection")" -eq 1 ]
done
# The actual existing two-pane leases are retained, not remounted on focus.
if [ -n "${cli_split_flag:-}" ];then
 [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -eq "$selection_ready" ]
fi
[ "$(grep -c '^workspace.session_opened$' "$log" || true)" -eq 2 ]
sleep 1
timeout 3s import -window "$xid" "target/probes/$selection_name-rendered.png"
cp "$fixture/native.pid.selection" "target/probes/$selection_name-pid-input.txt"
echo "workspace.${selection_name}_original_page_existing_root_sessions_exact_keyboard_and_no_duplicate_pty_confirmed"
