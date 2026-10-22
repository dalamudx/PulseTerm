#!/bin/sh
# Fixed synthetic master password in isolated private directories; no real credentials/HOME.
set -eu
umask 077
mode=${1:-basic};case "$mode" in basic|revocation);;*)echo workspace.vault_invalid_probe_mode >&2;exit 2;;esac
mkdir -p target/probes
fixture=$(mktemp -d "$PWD/target/workspace-vault.XXXXXX")
export HOME="$fixture/home" XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
app=
cleanup(){ if [ -n "$app" ];then kill "$app" 2>/dev/null||true;wait "$app" 2>/dev/null||true;fi;rm -rf "$fixture"; }
trap cleanup EXIT
count(){ grep -c "^workspace.$1$" "$log"||true; }
await_count(){ step=$1;expected=$2;attempt=0;while [ "$(count "$step")" -lt "$expected" ];do if ! kill -0 "$app" 2>/dev/null;then status=0;wait "$app"||status=$?;app=;echo "workspace.vault_native_exit.$step.$status" >&2;tail -12 "$log" >&2;exit 1;fi;[ "$attempt" -lt 80 ]||{ echo "workspace.vault_timeout.$step" >&2;shot "failure-$step";tail -12 "$log" >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done; }
start(){ round=$1;log="target/probes/workspace-vault-native-$round.log";target/debug/pulseterm-workspace >"$log" 2>&1 & app=$!;await_count profiles_listed 1;xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true);[ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ];[ "$(xdotool getwindowpid "$xid")" = "$app" ];timeout 2s xdotool windowactivate --sync "$xid"; }
shot(){ timeout 3s import -window "$xid" "target/probes/workspace-vault-$1.png"; }
keys(){ timeout 3s xdotool key --clearmodifiers "$@"; }
type(){ timeout 3s xdotool type --clearmodifiers --delay 1 "$1"; }
open(){ paint=0;while :;do shot sidebar;[ "$(convert target/probes/workspace-vault-sidebar.png -format '%k' info:)" -gt 10 ]&&break;[ "$paint" -lt 10 ]||{ echo workspace.vault_sidebar_paint_timeout >&2;exit 1;};paint=$((paint+1));sleep 0.1;done;timeout 3s tesseract target/probes/workspace-vault-sidebar.png target/probes/workspace-vault-sidebar --psm 11 tsv 2>/dev/null;y=$(awk -F '\t' '$12=="SSH" && $7<170 && $8>150 {n++;y=int($8+$10/2)}END{if(n==2)print y+52}' target/probes/workspace-vault-sidebar.tsv);[ -n "$y" ]||{ echo workspace.vault_sidebar_anchor_not_drawn >&2;exit 1;};timeout 3s xdotool mousemove --window "$xid" 150 "$y" click 1; }
read(){ keys shift+Tab Return; }
close_app(){ [ "$(count session_opened)" -eq 0 ];keys alt+F4;await_count exited 1;wait "$app";app=; }
start 1
open
await_count vault_state_missing 1
sleep 0.2
shot missing
keys shift+Tab shift+Tab shift+Tab shift+Tab ctrl+a
type 'fixed-window-master-password';keys Tab;type 'fixed-window-master-password';keys Tab Return
await_count vault_created_locked 1
sleep 0.2
shot created-locked
keys shift+Tab shift+Tab shift+Tab ctrl+a
type 'wrong-window-master-password';keys Tab Return
await_count vault_operation_rejected 1
sleep 0.2
shot wrong-password
before=$(count vault_state_locked)
read
await_count vault_state_locked "$((before+1))"
sleep 0.2
keys shift+Tab shift+Tab shift+Tab ctrl+a
type 'fixed-window-master-password';keys Tab Return
await_count vault_front_confirmed 1
[ "$(count vault_unlock_ready)" -eq 1 ]
sleep 0.2
shot unlocked
before=$(count vault_state_locked)
keys shift+Tab shift+Tab Return
await_count vault_state_locked "$((before+1))"
sleep 0.2
shot relocked
if [ "$mode" = revocation ];then
confirmed=$(count vault_front_confirmed);admitted=$(count vault_unlock_admitted);rejected=$(count vault_operation_rejected);cancelled=$(count vault_ticket_cancelled)
keys shift+Tab shift+Tab shift+Tab ctrl+a;type 'fixed-window-master-password';keys Tab Return
await_count vault_unlock_admitted "$((admitted+1))"
keys Escape
await_count vault_ticket_cancelled "$((cancelled+1))";await_count vault_operation_rejected "$((rejected+1))"
[ "$(count vault_front_confirmed)" -eq "$confirmed" ]
before=$(count vault_state_locked);open;await_count vault_state_locked "$((before+1))";sleep 0.2;shot cancelled-locked
admitted=$(count vault_unlock_admitted)
keys shift+Tab shift+Tab shift+Tab ctrl+a;type 'fixed-window-master-password';keys Tab Return
await_count vault_unlock_admitted "$((admitted+1))"
keys ctrl+shift+r
await_count document_confirmed 2;await_count profiles_listed 2
[ "$(count vault_front_confirmed)" -eq "$confirmed" ]
before=$(count vault_state_locked);open;await_count vault_state_locked "$((before+1))";sleep 0.2;shot retired-auth-locked
keys Escape
close_app
start 2
open
await_count vault_state_locked 1
shot revoked-reopened-locked
keys Escape
close_app
printf '%s\n' workspace.vault_admitted_auth_close_navigation_revoke_no_late_key_confirmed
exit 0
fi
keys Escape ctrl+shift+r
await_count document_confirmed 2;await_count profiles_listed 2
before=$(count vault_state_locked)
open
await_count vault_state_locked "$((before+1))"
shot refreshed-locked
keys Escape
close_app
start 2
open
await_count vault_state_locked 1
shot reopened-locked
keys Escape
close_app
printf '%s\n' "workspace.vault_master_create_locked_wrong_rejected_confirm_lock_refresh_reopen_confirmed.$mode"
