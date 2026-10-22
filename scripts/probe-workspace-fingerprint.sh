#!/bin/sh
# Independent OpenSSH + actual GUI keyboard/refresh/reopen; no JS or user directory injection.
set -eu
umask 077
mkdir -p target/probes
fixture=$(mktemp -d "${TMPDIR:-/data/temp}/workspace-fingerprint.XXXXXX")
export HOME="$fixture/home" XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
app=;server=
cleanup(){ status=$?;if [ "$status" -ne 0 ] && [ -n "${xid:-}" ];then shot failure||true;fi;if [ -n "$app" ];then kill "$app" 2>/dev/null||true;wait "$app" 2>/dev/null||true;fi;if [ -n "$server" ];then kill "$server" 2>/dev/null||true;wait "$server" 2>/dev/null||true;fi;rm -rf "$fixture"; }
trap cleanup EXIT
cc -Wall -Wextra -Werror scripts/probe-x11-status.c -o "$fixture/x11-status" $(pkg-config --cflags --libs atspi-2 gio-2.0 gobject-2.0 glib-2.0)
port=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1]);s.close()')
ssh-keygen -q -t ed25519 -N '' -f "$fixture/key"
ssh-keygen -q -t ed25519 -N '' -f "$fixture/changed-key"
start_server(){ round=$1;hostkey=$2;serverlog="target/probes/workspace-fingerprint-sshd-$round.log";printf 'ListenAddress 127.0.0.1\nPort %s\nHostKey %s\nPidFile %s\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication no\nPermitRootLogin no\nUsePAM no\nAllowTcpForwarding no\nPermitTTY no\nLogLevel DEBUG2\n' "$port" "$hostkey" "$fixture/sshd.pid" > "$fixture/sshd.conf";/usr/sbin/sshd -D -e -f "$fixture/sshd.conf" > "$serverlog" 2>&1 & server=$!;attempt=0;while ! grep -F 'Server listening' "$serverlog" >/dev/null;do kill -0 "$server";[ "$attempt" -lt 80 ];attempt=$((attempt+1));sleep 0.1;done; }
count(){ grep -c "^workspace.$1$" "$log"||true; }
await_count(){ step=$1;expected=$2;attempt=0;while [ "$(count "$step")" -lt "$expected" ];do if ! kill -0 "$app" 2>/dev/null;then echo "workspace.fingerprint_native_exit.$step" >&2;tail -15 "$log" >&2;exit 1;fi;[ "$attempt" -lt 80 ]||{ echo "workspace.fingerprint_timeout.$step" >&2;tail -15 "$log" >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done; }
start_app(){ round=$1;log="target/probes/workspace-fingerprint-native-$round.log";"${PULSE_WORKSPACE_BINARY:-${CARGO_TARGET_DIR:-target}/debug/pulseterm-workspace}" > "$log" 2>&1 & app=$!;await_count profiles_listed 1;xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true);[ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ];[ "$(xdotool getwindowpid "$xid")" = "$app" ];timeout 2s xdotool windowactivate --sync "$xid"; }
shot(){ timeout 3s import -window "$xid" "target/probes/workspace-fingerprint-$1.png"; }
keys(){ timeout 3s xdotool key --clearmodifiers "$@"; }
type(){ timeout 3s xdotool type --clearmodifiers --delay 1 "$1"; }
open_hosts(){ n=$1;shot sidebar;timeout 3s "$fixture/x11-status" "$app" '管理 SSH 主机' activate;await_count hosts_operation_confirmed "$n";sleep 0.2; }
open_fingerprint(){ n=$1;timeout 3s "$fixture/x11-status" "$app" '核对指纹' focus;keys space;await_count ssh_fingerprint_prepared "$n"; }
visible(){ name=$1;shift;shot "$name";timeout 3s tesseract "target/probes/workspace-fingerprint-$name.png" "target/probes/workspace-fingerprint-$name" --psm 11 2>/dev/null;for text in "$@";do grep -F "$text" "target/probes/workspace-fingerprint-$name.txt" >/dev/null||{ echo "workspace.fingerprint_not_drawn.$name" >&2;exit 1;};done; }
check(){ python3 scripts/workspace-fingerprint-fixture.py "$XDG_DATA_HOME/dev.pulseterm.workspace-preview/state.db" "$1" "$fixture/key.pub" "$port"; }
close_app(){ [ "$(count session_opened)" -eq 0 ];keys alt+F4;await_count exited 1;wait "$app";app=; }
start_server 1 "$fixture/key"
start_app 1;open_hosts 1
timeout 3s "$fixture/x11-status" "$app" '显示名称' focus;keys ctrl+a;type FINGERPRINT
timeout 3s "$fixture/x11-status" "$app" '主机地址' focus;keys ctrl+a;type 127.0.0.1
timeout 3s "$fixture/x11-status" "$app" '端口' focus;keys ctrl+a;type "$port"
timeout 3s "$fixture/x11-status" "$app" '用户名' focus;keys ctrl+a;type fixed
timeout 3s "$fixture/x11-status" "$app" '添加' activate
await_count hosts_operation_confirmed 2;sleep 0.2
# Focus the distinct fingerprint action before opening its original-page approval.
open_fingerprint 1;await_count ssh_fingerprint_observed.unknown 1;sleep 0.2
visible unknown FINGERPRINT 127.0.0.1 ssh-ed25519 SHA256
# Default Cancel (Space), not implicit trust or authentication.
keys space;await_count ssh_fingerprint_cancelled 1;check empty
# Returning focus is the fingerprint button; inspect again, then retire the page instead of approving.
keys space;await_count ssh_fingerprint_observed.unknown 2;keys ctrl+shift+r
await_count document_confirmed 2;await_count profiles_listed 2;check empty
open_hosts 3;open_fingerprint 3;await_count ssh_fingerprint_observed.unknown 3;sleep 0.2
# Default Cancel remains focused; forward Tab explicitly chooses Save.
keys Tab space;await_count ssh_fingerprint_confirmed 1;check saved
visible saved SHA256 ssh-ed25519
keys Escape
# Focus returns to the fingerprint action; known keys have no approval ticket.
keys space;await_count ssh_fingerprint_observed.already_pinned 1;sleep 0.2;visible matched SHA256 ssh-ed25519
[ "$(count ssh_fingerprint_confirmed)" -eq 1 ];keys Escape Escape;close_app
start_app 2;open_hosts 1;open_fingerprint 1;await_count ssh_fingerprint_observed.already_pinned 1;check saved;keys Escape
kill "$server";wait "$server"||true;server=
start_server 2 "$fixture/changed-key"
keys space;await_count ssh_fingerprint_observed.changed 1;sleep 0.2;visible changed SHA256 ssh-ed25519
[ "$(count ssh_fingerprint_confirmed)" -eq 0 ];check saved
keys Escape Escape;close_app
! grep -E 'userauth-request|Accepted |Failed password|Failed publickey' target/probes/workspace-fingerprint-sshd-1.log target/probes/workspace-fingerprint-sshd-2.log
printf '%s\n' workspace.actual_fingerprint_cancel_approve_reload_reopen_changed_blocked_auth_zero_confirmed
