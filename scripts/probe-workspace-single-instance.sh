#!/bin/sh
# Sourced after an independently activated decoy in the real CLI window probe.
# Own only secondary GUI processes/temp files; never touch another app's runtime.
(
 set -eu
 secondary_log=$(mktemp "$TMPDIR/pulseterm-gui-secondary.XXXXXX")
 token_copy=$(mktemp "$TMPDIR/pulseterm-gui-token.XXXXXX")
 trap 'rm -f "$secondary_log" "$token_copy"' EXIT
 runtime="$XDG_RUNTIME_DIR/pulseterm-workspace-cli"
 socket_identity=$(stat -c '%d:%i:%u:%a' "$runtime/control.sock")
 token_identity=$(stat -c '%d:%i:%u:%a' "$runtime/control.token")
 cp "$runtime/control.token" "$token_copy"
 # Same broker/bundle, independent second GUI. It must exit without setup.
 timeout --kill-after=1s 5s target/debug/pulseterm-workspace > "$secondary_log" 2>&1
 if grep -Eq '^workspace\.(navigation_started|session_opened)$' "$secondary_log";then
  echo workspace.secondary_reached_setup >&2;exit 1
 fi
 attempt=0
 while [ "$(xdotool getactivewindow)" != "$ids" ] && [ "$attempt" -lt 40 ];do attempt=$((attempt+1));sleep 0.05;done
 [ "$(xdotool getactivewindow)" = "$ids" ] || { echo workspace.secondary_focus_missing >&2;exit 1; }
 # Forwarded action-shaped argv is not scheduling or execution permission yet.
 timeout --kill-after=1s 5s target/debug/pulseterm-workspace --startup-actions '{"jsonrpc":"2.0","id":"a7d68e84-72b2-435c-8fa8-4c3525ff803e","method":"workspace.new-tab","params":{"profile":"sh"}}' > "$secondary_log" 2>&1
 grep -q '^workspace.single_instance_action_unavailable$' "$log"
 [ "$(stat -c '%d:%i:%u:%a' "$runtime/control.sock")" = "$socket_identity" ]
 [ "$(stat -c '%d:%i:%u:%a' "$runtime/control.token")" = "$token_identity" ]
 cmp -s "$runtime/control.token" "$token_copy"
 [ "$(grep -c '^workspace.session_opened$' "$log" || true)" = "$opened_before" ]
 kill -0 "$current_pid"
 kill -0 "$app"
 printf 'round=%s primary_pid=%s pty_pid=%s xid=%s endpoints_unchanged=true\n' "$round" "$app" "$current_pid" "$ids" >> target/probes/workspace-single-instance-result.txt
 # Keep the existing CLI focus check a fresh decoy→primary observation.
 timeout 2s xdotool windowactivate --sync "$decoy_id"
 [ "$(xdotool getactivewindow)" = "$decoy_id" ]
)
