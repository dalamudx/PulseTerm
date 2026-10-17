#!/bin/sh
# Sourced ONLY by isolated real workspace controller after its existing three-page checks.
[ "$mode" = terminal-cli ] && [ "${PULSE_CLI_NEW_TAB_PROBE:-0}" = 1 ]
cli_name=cli-new-tab
case ${PULSE_CLI_SPLIT_PROBE:-none} in
  none) ;;
  row) cli_name=cli-split-row;cli_split_flag=-v ;;
  column) cli_name=cli-split-column;cli_split_flag=-h ;;
  *) echo workspace.cli_invalid_test_direction >&2;exit 1 ;;
esac
cli_open_command=new-tab
cli_split_command=split
case ${PULSE_CLI_ALIAS_PROBE:-0} in
  0) ;;
  1)
    cli_open_command=open
    cli_split_command=split-pane
    cli_name="cli-alias-${cli_name#cli-}"
    ;;
  *) echo workspace.cli_invalid_test_alias >&2;exit 1 ;;
esac
[ "$(grep -c '^workspace.session_opened$' "$log" || true)" -eq 1 ]
cli_requests=0
cli_ask() {
  cli_requests=$((cli_requests+1))
  if [ -n "${cli_split_flag:-}" ];then
    target/debug/pulse "$cli_split_command" "$cli_split_flag" sh > "target/probes/$cli_name-$cli_requests.txt" 2>&1 &
  else
    target/debug/pulse "$cli_open_command" sh > "target/probes/$cli_name-$cli_requests.txt" 2>&1 &
  fi
  cli_request=$!
  attempt=0
  while [ "$(grep -c '^workspace.cli_approval_requested$' "$log" || true)" -lt "$cli_requests" ] && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
  [ "$(grep -c '^workspace.cli_approval_requested$' "$log" || true)" -eq "$cli_requests" ] || { echo workspace.cli_approval_missing >&2;exit 1; }
  timeout 2s xdotool windowactivate --sync "$xid"
  # Fixed black terminal centre must be covered by the actually drawn light modal.
  # Native event enqueue alone is not proof of visible consent.
  attempt=0
  while [ "$attempt" -lt 40 ];do
    timeout 3s import -window "$xid" "$fixture/cli-approval.png"
    colour=$(convert "$fixture/cli-approval.png" -format '%[pixel:p{480,320}]' info:)
    [ "$colour" = 'srgb(0,0,0)' ] || break
    kill -0 "$cli_request" 2>/dev/null || { echo workspace.cli_rejected_before_modal >&2;exit 1; }
    attempt=$((attempt+1));sleep 0.1
  done
  [ "$colour" != 'srgb(0,0,0)' ] || { echo workspace.cli_modal_not_drawn >&2;exit 1; }
  cp "$fixture/cli-approval.png" "target/probes/$cli_name-confirmation-$cli_requests.png"
}
cli_rejected() {
  timeout 8s tail --pid="$cli_request" --sleep-interval=0.1 -f /dev/null
  cli_status=0;wait "$cli_request" || cli_status=$?;cli_request=
  [ "$cli_status" -ne 0 ] || { echo workspace.cli_cancel_accepted >&2;exit 1; }
  [ "$(grep -c '^workspace.session_opened$' "$log" || true)" -eq 1 ] || { echo workspace.cli_cancel_spawned >&2;exit 1; }
}
# Existing wrapper has a fourth final-capture recovery beyond the first three pages.
attempt=0
while [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -lt 4 ] && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
[ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -eq 4 ]
sleep 0.2
cli_ask
# Initial focus is Cancel; Return is NOT approval.
timeout 3s import -window "$xid" target/probes/$cli_name-confirmation.png
timeout 2s xdotool key --clearmodifiers Return
cli_rejected
cli_ask
kill "$cli_request";cli_rejected
sleep 0.3
ready_before=$(grep -c '^workspace.terminal_input_ready$' "$log" || true)
cli_ask
# The modal blocks terminal shortcuts. Actual page reload is an independent controller event.
# Dedicated window-capture reload hook runs even while the GUI modal is open.
timeout 2s xdotool key --clearmodifiers ctrl+shift+r
cli_rejected
attempt=0
while [ "$(grep -c '^workspace.document_confirmed$' "$log" || true)" -lt 5 ] && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
[ "$(grep -c '^workspace.document_confirmed$' "$log" || true)" -eq 5 ]
attempt=0
while [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -le "$ready_before" ] && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
[ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -gt "$ready_before" ]
cli_ask
ready_before=$(grep -c '^workspace.terminal_input_ready$' "$log" || true)
# Real user approval after the old wire deadline; native startup has not begun.
if [ -n "${cli_split_flag:-}" ];then sleep 6;kill -0 "$cli_request";fi
timeout 2s xdotool key --clearmodifiers Tab Return
# A finite user-decision phase is distinct from the unchanged 5s native startup/auth budgets.
timeout 8s tail --pid="$cli_request" --sleep-interval=0.1 -f /dev/null
wait "$cli_request";cli_request=
grep -Eq '^session [1-9][0-9]*$' "target/probes/$cli_name-$cli_requests.txt"
attempt=0
while [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -le "$ready_before" ] && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
[ "$(grep -c '^workspace.session_opened$' "$log" || true)" -eq 2 ]
[ "$(grep -c '^workspace.cli_session_opened$' "$log" || true)" -eq 1 ]
cli_native_pid=$(cat "$fixture/native.pid")
[ "$cli_native_pid" != "$native_pid" ];kill -0 "$cli_native_pid";kill -0 "$native_pid"
if [ -n "${cli_split_flag:-}" ];then
  attempt=0
  while [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -lt $((ready_before+2)) ] && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
  [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -ge $((ready_before+2)) ] || { echo workspace.cli_split_missing_pane >&2;exit 1; }
  if [ "$cli_split_flag" = -v ];then timeout 2s xdotool mousemove --window "$xid" 730 330 click 1;else timeout 2s xdotool mousemove --window "$xid" 480 510 click 1;fi
  sleep 0.2
  timeout 3s import -window "$xid" "target/probes/$cli_name-layout.png"
fi
timeout 2s xdotool type --clearmodifiers --delay 1 cli-new-tab-verified
timeout 2s xdotool key --clearmodifiers Return
attempt=0
while ! grep -q '^cli-new-tab-verified$' "$fixture/native.pid.input" && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
grep -q '^cli-new-tab-verified$' "$fixture/native.pid.input"
if [ "${PULSE_CLI_SELECTION_PROBE:-0}" = 1 ];then . scripts/probe-workspace-cli-selection.sh;fi
ready_before=$(grep -c '^workspace.terminal_input_ready$' "$log" || true)
timeout 2s xdotool key --clearmodifiers ctrl+shift+r
attempt=0
while [ "$(grep -c '^workspace.terminal_input_ready$' "$log" || true)" -le "$ready_before" ] && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
[ "$(grep -c '^workspace.document_confirmed$' "$log" || true)" -eq 6 ]
[ "$(cat "$fixture/native.pid")" = "$cli_native_pid" ];kill -0 "$cli_native_pid";kill -0 "$native_pid"
timeout 2s xdotool type --clearmodifiers --delay 1 cli-new-tab-recovered
timeout 2s xdotool key --clearmodifiers Return
attempt=0
while ! grep -q '^cli-new-tab-recovered$' "$fixture/native.pid.input" && [ "$attempt" -lt 80 ];do attempt=$((attempt+1));sleep 0.1;done
grep -q '^cli-new-tab-recovered$' "$fixture/native.pid.input"
# Capture final live echo after the individual glyph frames have had their existing
# draw/ACK gates; this observation delay changes no application or protocol deadline.
sleep 1
timeout 3s import -window "$xid" target/probes/$cli_name-rendered.png
cp "$fixture/native.pid.input" target/probes/$cli_name-input.txt
echo workspace.cli_gui_approval_cancel_disconnect_recovery_confirmed
if [ -n "${cli_split_flag:-}" ];then echo "workspace.cli_split_${PULSE_CLI_SPLIT_PROBE}_confirmed";fi
