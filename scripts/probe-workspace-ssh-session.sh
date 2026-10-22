#!/bin/sh
# Actual Linux GUI plus isolated real russh peer. No user HOME, system account changes or remote OS claim.
set -eu
umask 077
# Bound observer CPU use; preserve all existing OCR, native and outer deadlines.
export OMP_THREAD_LIMIT=1 MAGICK_THREAD_LIMIT=1
probe_mode=${1:-window};mode=$probe_mode;private_key=0;authenticated=ssh_password_authenticated
case "$mode" in key-*)private_key=1;mode=${mode#key-};authenticated=ssh_public_key_authenticated;;esac
case "$mode" in window|close|eof|cancel|commands|cli);;*)echo workspace.ssh_invalid_probe_mode >&2;exit 2;;esac
if [ "$private_key" -eq 1 ];then case "$mode" in window|close|eof|cancel|cli);;*)echo workspace.ssh_invalid_private_key_probe_mode >&2;exit 2;;esac;fi
mkdir -p target/probes
mkdir -p /data/temp
fixture=$(mktemp -d /data/temp/pulseterm-workspace-ssh-session.XXXXXX)
printf 'owned-fixture-v1' > "$fixture/owned-workspace-ssh-session.fixture"
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
app=;peer=;xid=;cli_request=
opened=ssh_session_opened
if [ "$mode" = cli ];then
 opened=cli_ssh_opened
 export XDG_RUNTIME_DIR="$fixture/runtime"
 mkdir -m 700 "$XDG_RUNTIME_DIR"
fi
log="target/probes/workspace-ssh-session-native-$probe_mode.log"
peerlog="target/probes/workspace-ssh-session-peer-$probe_mode.log"
cleanup(){ if [ -n "$cli_request" ];then kill "$cli_request" 2>/dev/null||true;wait "$cli_request" 2>/dev/null||true;fi;if [ -n "$app" ];then kill "$app" 2>/dev/null||true;wait "$app" 2>/dev/null||true;fi;if [ -n "$peer" ];then kill "$peer" 2>/dev/null||true;wait "$peer" 2>/dev/null||true;fi;rm -rf "$fixture"; }
trap cleanup EXIT
count(){ grep -c "^workspace.$1$" "$log"||true; }
peer_count(){ grep -c "^fixture.$1$" "$peerlog"||true; }
shot(){ timeout 3s import -window "$xid" "target/probes/workspace-ssh-session-$1.png"; }
keys(){ timeout 3s xdotool key --clearmodifiers "$@"; }
type(){ timeout 3s xdotool type --clearmodifiers --delay 1 "$1"; }
await_count(){
 step=$1;expected=$2;attempt=0
 while [ "$(count "$step")" -lt "$expected" ];do
  kill -0 "$app"
  # Focus a uniquely PID-owned startup window before waiting for frontend readiness.
  # This is a controller stimulus, not an authorization receipt; no extra deadline or retry.
  if [ "$step" = profiles_listed ] && [ "$expected" -eq 1 ] && [ -z "$xid" ];then
   startup_xids=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
   if [ "$(printf '%s\n' "$startup_xids" | awk 'NF{n++}END{print n+0}')" -eq 1 ] && [ "$(xdotool getwindowpid "$startup_xids" 2>/dev/null || true)" = "$app" ];then
    xid=$startup_xids
    xdotool windowactivate "$xid"
   fi
  fi
  if [ "$attempt" -ge 80 ];then
   echo "workspace.ssh_session_timeout.$step" >&2
   ps -o pid,ppid,stat,etime,comm -p "$app" > "target/probes/workspace-ssh-session-timeout-$mode.txt" || true
   if [ -z "$xid" ];then
    timeout_xids=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null || true)
    printf 'owned visible windows: %s\n' "$timeout_xids" >> "target/probes/workspace-ssh-session-timeout-$mode.txt"
    if [ "$(printf '%s\n' "$timeout_xids" | awk 'NF{n++}END{print n+0}')" -eq 1 ] && [ "$(xdotool getwindowpid "$timeout_xids" 2>/dev/null || true)" = "$app" ];then xid=$timeout_xids;fi
   fi
   if [ -n "$xid" ];then shot "failure-$step" || true;fi
   tail -18 "$log" >&2;tail -8 "$peerlog" >&2;exit 1
  fi
  attempt=$((attempt+1));sleep 0.1
 done
}
await_peer(){ step=$1;expected=$2;attempt=0;while [ "$(peer_count "$step")" -lt "$expected" ];do kill -0 "$peer";[ "$attempt" -lt 80 ]||{ echo "fixture.ssh_session_timeout.$step" >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done; }
sidebar(){ paint=0;while :;do shot sidebar;[ "$(convert target/probes/workspace-ssh-session-sidebar.png -format '%k' info:)" -gt 10 ]&&break;[ "$paint" -lt 10 ];paint=$((paint+1));sleep 0.1;done;convert target/probes/workspace-ssh-session-sidebar.png -crop 180x640+0+0 -resize 400% target/probes/workspace-ssh-session-sidebar-ocr.png;timeout 3s tesseract target/probes/workspace-ssh-session-sidebar-ocr.png target/probes/workspace-ssh-session-sidebar --psm 11 tsv 2>/dev/null; }
click_sidebar(){ kind=$1;sidebar;y=$(awk -F '\t' -v kind="$kind" '$12~/^SSH/&&$7<320&&$8>600{n++;if(n==1)first=int(($8+$10/2)/4);second=int(($8+$10/2)/4)}END{if(n==2){if(kind=="host")print first;else print second+52}}' target/probes/workspace-ssh-session-sidebar.tsv);[ -n "$y" ]||{ echo "workspace.ssh_sidebar_anchor_not_drawn.$kind" >&2;exit 1;};timeout 3s xdotool mousemove --window "$xid" 150 "$y" click 1; }
visible(){ name=$1;shift;shot "$name";if [ "$name" = selection ];then : > "target/probes/workspace-ssh-session-$name.txt";for top in 262 334;do convert "target/probes/workspace-ssh-session-$name.png" -crop "485x22+229+$top" -resize 400% "target/probes/workspace-ssh-session-select-$top.png";timeout 3s tesseract "target/probes/workspace-ssh-session-select-$top.png" stdout --psm 7 2>/dev/null >> "target/probes/workspace-ssh-session-$name.txt";done;else convert "target/probes/workspace-ssh-session-$name.png" -crop 780x120+200+149 -resize 400% "target/probes/workspace-ssh-session-$name-ocr.png";timeout 3s tesseract "target/probes/workspace-ssh-session-$name-ocr.png" "target/probes/workspace-ssh-session-$name" --psm 6 2>/dev/null;fi;for text in "$@";do grep -Fi "$text" "target/probes/workspace-ssh-session-$name.txt" >/dev/null||{ echo "workspace.ssh_session_not_drawn.$name.$text" >&2;exit 1;};done; }
# Seed fresh private app ciphertext and known pin via backend fixture, never page-side key or approval injection.
peer_mode=normal;[ "$mode" != cancel ]||peer_mode=stall
if [ "$private_key" -eq 1 ];then peer_mode=key;[ "$mode" != cancel ]||peer_mode=key-stall;fi
target/debug/xtask serve-workspace-ssh-fixture "$XDG_DATA_HOME/dev.pulseterm.workspace-preview" "$peer_mode" > "$peerlog" 2>&1 & peer=$!
attempt=0;while ! grep -F fixture.ssh_gui_seeded_listening "$peerlog" >/dev/null;do kill -0 "$peer";[ "$attempt" -lt 200 ]||{ tail -15 "$peerlog" >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
target/debug/pulseterm-workspace > "$log" 2>&1 & app=$!
await_count profiles_listed 1
xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true)
[ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ];[ "$(xdotool getwindowpid "$xid")" = "$app" ];timeout 2s xdotool windowactivate --sync "$xid"
if [ "$mode" = cli ];then
 host_id=$(sed -n 's/^fixture.ssh_gui_host_id.//p' "$peerlog")
 identity_id=$(sed -n 's/^fixture.ssh_gui_identity_id.//p' "$peerlog")
 status=0;target/debug/pulse ssh "$host_id" --identity "$identity_id" > target/probes/workspace-cli-ssh-locked.txt 2>&1 || status=$?
 [ "$status" -eq 1 ];[ "$(count cli_ssh_approval_requested)" -eq 0 ];[ "$(peer_count "$authenticated")" -eq 0 ]
fi
# Unlock only through the existing master-password UI with actual keyboard and frontend confirmation.
click_sidebar vault;await_count vault_state_locked 1;sleep 0.2
keys shift+Tab shift+Tab shift+Tab ctrl+a;type fixed-target-master;keys Tab Return
await_count vault_front_confirmed 1;sleep 0.2;keys shift+Tab Return;await_count vault_state_available 1;sleep 0.2;keys Escape
if [ "$mode" = cli ];then
 . scripts/probe-workspace-cli-ssh-approval.sh
else
# Existing host management returns focus to the original button; previous Tab is distinct Connect.
click_sidebar host;await_count hosts_operation_confirmed 1;keys Escape
if [ "$mode" = commands ];then keys ctrl+shift+p;sleep 0.2;type connect;keys Return;else keys shift+Tab Return;fi
sleep 0.2
# Full identity label is visible in the retained raw screenshot; OCR adds L at I_ glyph.
visible selection GUI_HOST IDENTITY
keys Tab Tab Tab Return
fi
if [ "$mode" = cancel ];then
 await_peer ssh_handshake_stream_owned 1
 keys Escape;await_count ssh_session_cancelled 1
 attempt=0;while [ "$(peer_count ssh_handshake_stream_exited)" -lt 1 ];do [ "$attempt" -lt 20 ]||{ echo fixture.ssh_cancel_exit_exceeded_two_seconds >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
 [ "$(count "$opened")" -eq 0 ];[ "$(peer_count "$authenticated")" -eq 0 ]
 # Reuse admission only after actual I/O exit; no re-unlock or new epoch is granted.
 click_sidebar host;await_count hosts_operation_confirmed 3;keys Escape shift+Tab Return;sleep 0.2
 keys Tab Tab Tab Return;await_peer ssh_handshake_stream_owned 2
 keys ctrl+shift+r
 attempt=0;while [ "$(peer_count ssh_handshake_stream_exited)" -lt 2 ];do [ "$attempt" -lt 20 ]||{ echo fixture.ssh_navigation_exit_exceeded_two_seconds >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done
 await_count profiles_listed 2
 [ "$(count "$opened")" -eq 0 ];[ "$(count session_opened)" -eq 0 ];[ "$(count terminal_input_ready)" -eq 0 ]
 keys alt+F4;await_count exited 1;wait "$app";app=
 printf '%s\n' workspace.actual_ssh_dialog_cancel_and_navigation_native_stream_exit_no_session_confirmed
 exit 0
fi
await_count "$opened" 1;await_count terminal_input_ready 1
[ "$(peer_count "$authenticated")" -eq 1 ];[ "$(count session_opened)" -eq 0 ]
if [ "$private_key" -eq 1 ];then ! grep -F fixture.ssh_password_auth_attempt_rejected "$peerlog" >/dev/null;fi
type SSHINPUTONE;keys Return;await_peer ssh_input_line_echoed 1;sleep 0.2;visible echo-one GUI_SSH_READY SSHINPUTONE
# Page retirement keeps the same authenticated root session and does not unlock again or reconnect.
before=$(count terminal_input_ready);profiles=$(count profiles_listed);keys ctrl+shift+r;await_count profiles_listed "$((profiles+1))";await_count terminal_input_ready "$((before+1))"
[ "$(peer_count "$authenticated")" -eq 1 ];[ "$(count "$opened")" -eq 1 ]
type SSHINPUTTWO;keys Return;await_peer ssh_input_line_echoed 2
# Native resizing uses SSH request admission, not a remote kernel reply; redraw/input resume through new generation.
before=$(count terminal_input_ready);timeout 2s xdotool windowsize "$xid" 1000 650;await_count terminal_input_ready "$((before+1))"
type SSHINPUTTHREE;keys Return;await_peer ssh_input_line_echoed 3;sleep 0.2;visible echo-three GUI_SSH_READY SSHINPUTONE SSHINPUTTWO SSHINPUTTHREE
[ "$(peer_count "$authenticated")" -eq 1 ];grep -F fixture.ssh_window_request. "$peerlog" >/dev/null
if [ "$mode" = eof ];then
 ready=$(count terminal_input_ready);type exit;keys Return;await_count terminal_eof_acked 1
 await_peer ssh_remote_exit_status_sent.7 1;await_peer ssh_authenticated_stream_exited 1
 [ "$(peer_count ssh_input_line_echoed)" -eq 3 ];[ "$(count terminal_input_ready)" -eq "$ready" ]
 for expected in 2 3;do
  keys ctrl+shift+r;await_count terminal_eof_acked "$expected"
  [ "$(count terminal_input_ready)" -eq "$ready" ];[ "$(peer_count "$authenticated")" -eq 1 ];[ "$(count "$opened")" -eq 1 ]
 done
 type AFTEREOF;keys Return;sleep 0.2
 [ "$(peer_count ssh_input_line_echoed)" -eq 3 ];[ "$(count terminal_input_ready)" -eq "$ready" ]
 ! grep -F workspace.terminal_failure. "$log" >/dev/null
 visible ended GUI_SSH_READY SSHINPUTONE SSHINPUTTWO SSHINPUTTHREE SSH_END
 cp target/probes/workspace-ssh-session-ended.png target/probes/workspace-ssh-session-ended-eof.png
fi
if [ "$mode" = close ];then
 timeout 3s xdotool mousemove --window "$xid" 942 82 click 1;sleep 0.2;keys space;sleep 0.2
 [ "$(count protocol_session_resources_joined)" -eq 0 ];[ "$(peer_count ssh_authenticated_stream_exited)" -eq 0 ]
 timeout 3s xdotool mousemove --window "$xid" 942 82 click 1;sleep 0.2;keys Tab Return
 await_count protocol_session_resources_joined 1;await_peer ssh_authenticated_stream_exited 1
fi
keys alt+F4;await_count exited 1;wait "$app";app=
await_peer ssh_authenticated_stream_exited 1
if [ "$mode" = cli ];then
 [ ! -e "$XDG_RUNTIME_DIR/pulseterm-workspace-cli/control.sock" ];[ ! -e "$XDG_RUNTIME_DIR/pulseterm-workspace-cli/control.token" ]
 echo workspace.cli_saved_ssh_locked_cancel_disconnect_navigation_approval_and_native_drain_confirmed
fi
cp target/probes/workspace-ssh-session-echo-three.png "target/probes/workspace-ssh-session-echo-three-$probe_mode.png"
printf '%s\n' "workspace.actual_saved_ssh_ciphertext_channel_dom_keyboard_refresh_resize_root_shutdown_confirmed.$probe_mode"
