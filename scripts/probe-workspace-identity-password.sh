#!/bin/sh
# Fixed synthetic credentials in isolated private HOME/SQLite; no plaintext read IPC or SSH target use.
set -eu
umask 077
cargo=$(mise which cargo);RUSTC=$(mise which rustc)
export RUSTC CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
mkdir -p target/probes
fixture=$(mktemp -d "$PWD/target/workspace-identity-password.XXXXXX")
export HOME="$fixture/home" XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
app=
cleanup(){ if [ -n "$app" ];then kill "$app" 2>/dev/null||true;wait "$app" 2>/dev/null||true;fi;rm -rf "$fixture"; }
trap cleanup EXIT
count(){ grep -c "^workspace.$1$" "$log"||true; }
await_count(){ step=$1;expected=$2;attempt=0;while [ "$(count "$step")" -lt "$expected" ];do if ! kill -0 "$app" 2>/dev/null;then status=0;wait "$app"||status=$?;app=;echo "workspace.identity_password_native_exit.$step.$status" >&2;tail -12 "$log" >&2;exit 1;fi;[ "$attempt" -lt 80 ]||{ echo "workspace.identity_password_timeout.$step" >&2;shot "failure-$step";tail -12 "$log" >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done; }
start(){ round=$1;log="target/probes/workspace-identity-password-native-$round.log";target/debug/pulseterm-workspace >"$log" 2>&1 & app=$!;await_count profiles_listed 1;xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true);[ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ];[ "$(xdotool getwindowpid "$xid")" = "$app" ];timeout 2s xdotool windowactivate --sync "$xid"; }
shot(){ timeout 3s import -window "$xid" "target/probes/workspace-identity-password-$1.png"; }
keys(){ timeout 3s xdotool key --clearmodifiers "$@"; }
type(){ timeout 3s xdotool type --clearmodifiers --delay 1 "$1"; }
open(){ kind=$1;paint=0;while :;do shot sidebar;[ "$(convert target/probes/workspace-identity-password-sidebar.png -format '%k' info:)" -gt 10 ]&&break;[ "$paint" -lt 10 ]||{ echo workspace.identity_password_paint_timeout >&2;exit 1;};paint=$((paint+1));sleep 0.1;done;timeout 3s tesseract target/probes/workspace-identity-password-sidebar.png target/probes/workspace-identity-password-sidebar --psm 11 tsv 2>/dev/null;y=$(awk -F '\t' '$12=="SSH" && $7<170 && $8>150 {n++;y=int($8+$10/2)}END{if(n==2)print y}' target/probes/workspace-identity-password-sidebar.tsv);[ -n "$y" ]||{ echo workspace.identity_password_sidebar_anchor >&2;exit 1;};if [ "$kind" = vault ];then y=$((y+52));fi;timeout 3s xdotool mousemove --window "$xid" 150 "$y" click 1; }
drawn(){ name=$1;shot "$name";timeout 3s tesseract "target/probes/workspace-identity-password-$name.png" "target/probes/workspace-identity-password-$name" --psm 11 2>/dev/null;grep -F IDPWONE "target/probes/workspace-identity-password-$name.txt" >/dev/null||{ echo "workspace.identity_password_identity_not_drawn.$name" >&2;exit 1;}; }
close_app(){ [ "$(count session_opened)" -eq 0 ];keys alt+F4;await_count exited 1;wait "$app";app=; }
start 1
open vault;await_count vault_state_missing 1;sleep 0.2
keys shift+Tab shift+Tab shift+Tab shift+Tab ctrl+a;type fixed-window-master-password;keys Tab;type fixed-window-master-password;keys Tab Return
await_count vault_created_locked 1;sleep 0.2;keys Escape
open identities;await_count identities_operation_confirmed 1;sleep 0.2
keys shift+Tab shift+Tab shift+Tab ctrl+a;type IDPWONE;keys Tab ctrl+a;type alice;keys Tab Return
await_count identities_operation_confirmed 2;sleep 0.2;drawn created
keys shift+Tab shift+Tab shift+Tab ctrl+a;type fixed-window-identity-password;keys Tab Tab Return
await_count identity_password_prepare_rejected 1
# Native rejection precedes the JS receipt/field cleanup. Wait for the actual failure modal paint, not a fixed delay.
cutoff=$(( $(date +%s)+8 ))
while :;do
shot locked-rejected
timeout 3s tesseract target/probes/workspace-identity-password-locked-rejected.png target/probes/workspace-identity-password-locked-rejected --psm 11 tsv 2>/dev/null
if awk -F '\t' '$12=="SSH" && $7>200{found=1}END{exit !found}' target/probes/workspace-identity-password-locked-rejected.tsv && ! grep -F IDPWONE target/probes/workspace-identity-password-locked-rejected.tsv >/dev/null;then break;fi
[ "$(date +%s)" -lt "$cutoff" ]||{ echo workspace.identity_password_error_draw_timeout >&2;exit 1;};sleep 0.1
done
[ "$(count identity_password_saved)" -eq 0 ]
# This controlled 960x640 viewport has one visible Done button in the settled failure sheet. Test an independent real mouse close, not a GTK focus-cycle assumption.
timeout 3s xdotool mousemove --window "$xid" 704 400 click 1
sleep 0.1
open vault;await_count vault_state_locked 1;sleep 0.2
keys shift+Tab shift+Tab shift+Tab ctrl+a;type fixed-window-master-password;keys Tab Return
await_count vault_front_confirmed 1;sleep 0.2;keys Escape
open identities;await_count identities_operation_confirmed 3;sleep 0.2
keys shift+Tab shift+Tab shift+Tab ctrl+a;type fixed-window-identity-password;keys Tab Tab Return
await_count identity_password_saved 1;await_count identities_operation_confirmed 4;sleep 0.2;drawn saved
keys Escape ctrl+shift+r;await_count document_confirmed 2;await_count profiles_listed 2
open identities;await_count identities_operation_confirmed 5;sleep 0.2;drawn refreshed
keys Escape;close_app
start 2
open identities;await_count identities_operation_confirmed 1;sleep 0.2;drawn reopened
[ "$(count identity_password_saved)" -eq 0 ];keys Escape;close_app
export PULSE_IDENTITY_PASSWORD_VERIFY_DIRECTORY="$XDG_DATA_HOME/dev.pulseterm.workspace-preview"
"$cargo" test --offline -p pulseterm-desktop --lib --features workspace --locked product_workspace::profiles::password_service::tests::identity_password_window_verify -- --ignored --exact --nocapture >target/probes/workspace-identity-password-verify.log 2>&1
grep -F workspace.identity_password_private_byte_reopen_confirmed target/probes/workspace-identity-password-verify.log >/dev/null
grep -F '1 passed' target/probes/workspace-identity-password-verify.log >/dev/null
unset PULSE_IDENTITY_PASSWORD_VERIFY_DIRECTORY
printf '%s\n' workspace.identity_password_locked_deny_save_refresh_process_reopen_bytes_confirmed
