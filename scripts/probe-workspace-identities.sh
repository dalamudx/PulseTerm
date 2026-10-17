#!/bin/sh
# Isolated metadata-only test; never authentication, plaintext or user HOME.
set -eu
umask 077
# Resolve trusted pinned tools before replacing fixture XDG; do not auto-install.
cargo=$(mise which cargo)
RUSTC=$(mise which rustc)
export RUSTC CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/workspace-identities.XXXXXX)
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture"
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
export OMP_THREAD_LIMIT=1
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
app=
cleanup(){ if [ -n "$app" ];then kill "$app" 2>/dev/null||true;wait "$app" 2>/dev/null||true;fi;rm -rf "$fixture";test ! -e "$fixture"; }
trap cleanup EXIT
count(){ grep -c "^workspace.$1$" "$log"||true; }
await_count(){ step=$1;expected=$2;attempt=0;while [ "$(count "$step")" -lt "$expected" ];do if ! kill -0 "$app" 2>/dev/null;then status=0;wait "$app"||status=$?;app=;echo "workspace.identities_native_exit.$step.$status" >&2;tail -12 "$log" >&2;exit 1;fi;[ "$attempt" -lt 80 ]||{ echo "workspace.identities_timeout.$step" >&2;tail -12 "$log" >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done; }
start(){ round=$1;log="target/probes/workspace-identities-native-$round.log";target/debug/pulseterm-workspace >"$log" 2>&1 & app=$!;await_count profiles_listed 1;xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true);[ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ];[ "$(xdotool getwindowpid "$xid")" = "$app" ];timeout 2s xdotool windowactivate --sync "$xid"; }
shot(){ timeout 3s import -window "$xid" "target/probes/workspace-identities-$1.png"; }
keys(){ timeout 3s xdotool key --clearmodifiers "$@"; }
type(){ timeout 3s xdotool type --clearmodifiers --delay 1 "$1"; }
open(){ n=$1;paint=0;while :;do shot sidebar;[ "$(magick target/probes/workspace-identities-sidebar.png -format '%k' info:)" -gt 10 ]&&break;[ "$paint" -lt 10 ]||{ echo workspace.identities_paint_timeout >&2;exit 1;};paint=$((paint+1));sleep 0.1;done;timeout 3s tesseract target/probes/workspace-identities-sidebar.png target/probes/workspace-identities-sidebar --psm 11 tsv 2>/dev/null;y=$(awk -F '\t' '$12 ~ /^SSH/ && $7<85 && $8>150 {n++;y=int($8+$10/2)}END{if(n==2)print y}' target/probes/workspace-identities-sidebar.tsv|tail -1);[ -n "$y" ]||{ echo workspace.identities_heading_not_drawn >&2;exit 1;};timeout 3s xdotool mousemove --window "$xid" 105 "$y" click 1;await_count identities_operation_confirmed "$n";sleep 0.2; }
drawn(){ name=$1;shift;shot "$name";timeout 3s tesseract "target/probes/workspace-identities-$name.png" "target/probes/workspace-identities-$name" --psm 11 2>/dev/null;for match in "$@";do grep -F "$match" "target/probes/workspace-identities-$name.txt" >/dev/null||{ echo "workspace.identities_not_drawn.$name" >&2;exit 1; };done; }
close_app(){ [ "$(count session_opened)" -eq 0 ];keys alt+F4;await_count exited 1;wait "$app";app=; }
start absent
open 1
shot no-vault
keys Escape
close_app
export PULSE_IDENTITY_FIXTURE_DIRECTORY="$XDG_DATA_HOME/dev.pulseterm.workspace-preview"
"$cargo" test --offline -p pulseterm-desktop --lib --features workspace --locked product_workspace::identities::tests::identity_window_fixture -- --ignored --exact --nocapture > target/probes/workspace-identities-fixture.log 2>&1
grep -F workspace.identity_metadata_only_fixture_confirmed target/probes/workspace-identities-fixture.log >/dev/null
grep -F '1 passed' target/probes/workspace-identities-fixture.log >/dev/null
unset PULSE_IDENTITY_FIXTURE_DIRECTORY
start 1
open 1
keys shift+Tab shift+Tab shift+Tab ctrl+a
type IDENTITYONE;keys Tab ctrl+a;type alice;keys Tab Return
await_count identities_operation_confirmed 2
sleep 0.2
drawn created IDENTITYONE alice
keys shift+Tab shift+Tab shift+Tab shift+Tab shift+Tab shift+Tab shift+Tab ctrl+a
type IDENTITYTWO;keys Tab ctrl+a;type bob;keys Tab Return
await_count identities_operation_confirmed 3
sleep 0.2
drawn updated IDENTITYTWO bob
keys Escape ctrl+shift+r
await_count document_confirmed 2;await_count profiles_listed 2
open 4
drawn refreshed IDENTITYTWO bob
keys Escape
close_app
start 2
open 1
drawn reopened IDENTITYTWO bob
keys shift+Tab shift+Tab shift+Tab shift+Tab Return
[ "$(count identities_operation_confirmed)" -eq 1 ]
keys Return
await_count identities_operation_confirmed 2
sleep 0.2
shot removed
keys Escape
close_app
start 3
open 1
shot deleted-reopened
timeout 3s tesseract target/probes/workspace-identities-deleted-reopened.png target/probes/workspace-identities-deleted-reopened --psm 11 2>/dev/null
! grep -F IDENTITYTWO target/probes/workspace-identities-deleted-reopened.txt
[ "$(count identities_operation_confirmed)" -eq 1 ]
keys Escape
close_app
printf '%s\n' workspace.identity_metadata_no_vault_create_update_refresh_reopen_remove_confirmed
