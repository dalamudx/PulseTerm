# Sourced inside the isolated real SSH/WebView fixture; never an execution authorization source.
cli_requests=0
cli_ask(){
 cli_requests=$((cli_requests+1))
 target/debug/pulse ssh "$host_id" --identity "$identity_id" > "target/probes/workspace-cli-ssh-result-$probe_mode-$cli_requests.txt" 2>&1 & cli_request=$!
 await_count cli_ssh_approval_requested "$cli_requests"
 timeout 2s xdotool windowactivate --sync "$xid"
 attempt=0
 while :;do
  shot "cli-approval-$probe_mode-$cli_requests"
  magick "target/probes/workspace-ssh-session-cli-approval-$probe_mode-$cli_requests.png" -resize 200% "$fixture/cli-approval-ocr.png"
  timeout 3s tesseract "$fixture/cli-approval-ocr.png" "$fixture/cli-approval-ocr" --psm 11 2>/dev/null
  if grep -F GUI_HOST "$fixture/cli-approval-ocr.txt" >/dev/null && grep -F IDENTITY "$fixture/cli-approval-ocr.txt" >/dev/null;then break;fi
  kill -0 "$cli_request";[ "$attempt" -lt 15 ]||{ echo workspace.cli_ssh_modal_not_drawn >&2;exit 1;}
  attempt=$((attempt+1));sleep 0.1
 done
 [ "$(peer_count "$authenticated")" -eq 0 ]
 [ "$(count cli_ssh_opened)" -eq 0 ];[ "$(count session_opened)" -eq 0 ]
}
cli_rejected(){
 timeout 8s tail --pid="$cli_request" --sleep-interval=0.1 -f /dev/null
 status=0;wait "$cli_request" || status=$?;cli_request=
 [ "$status" -eq 1 ]
 [ "$(peer_count "$authenticated")" -eq 0 ];[ "$(count cli_ssh_opened)" -eq 0 ]
}
cli_ask;keys Return;cli_rejected
cli_ask;kill "$cli_request";wait "$cli_request" 2>/dev/null||true;cli_request=;sleep 0.3
[ "$(peer_count "$authenticated")" -eq 0 ];[ "$(count cli_ssh_opened)" -eq 0 ]
cli_ask;profiles=$(count profiles_listed);keys ctrl+shift+r;await_count profiles_listed "$((profiles+1))";cli_rejected
# The approving page cannot authorize an obsolete saved-source snapshot.
cli_ask
target/debug/xtask serve-workspace-ssh-fixture "$XDG_DATA_HOME/dev.pulseterm.workspace-preview" mutate-host > target/probes/workspace-cli-ssh-source-mutation-$probe_mode.log 2>&1
grep -Fx fixture.ssh_gui_source_changed target/probes/workspace-cli-ssh-source-mutation-$probe_mode.log >/dev/null
keys Tab Return;cli_rejected
# Slow human consent is separate from the original native 5s startup deadline.
cli_ask;sleep 6;kill -0 "$cli_request";keys Tab Return
await_count cli_ssh_opened 1
status=0;timeout 8s tail --pid="$cli_request" --sleep-interval=0.1 -f /dev/null;wait "$cli_request"||status=$?;cli_request=
[ "$status" -eq 0 ];grep -Ex 'session [1-9][0-9]*' "target/probes/workspace-cli-ssh-result-$probe_mode-$cli_requests.txt" >/dev/null
