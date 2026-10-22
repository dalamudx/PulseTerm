#!/bin/sh
# Actual metadata UI and three native-process reopens; isolated XDG and task-owned temporary directory.
set -eu
umask 077
mkdir -p target/probes
mkdir -p /data/temp
fixture=$(mktemp -d /data/temp/pt-workspace-hosts.XXXXXX)
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" OMP_THREAD_LIMIT=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
app=
cleanup(){ if [ -n "$app" ];then kill "$app" 2>/dev/null||true;wait "$app" 2>/dev/null||true;fi;case "$fixture" in /data/temp/pt-workspace-hosts.*) rm -rf "$fixture";; *) return 1;; esac;test ! -e "$fixture"; }
trap cleanup EXIT
count(){ grep -c "^workspace.$1$" "$log"||true; }
await_count(){ step=$1;expected=$2;attempt=0;while [ "$(count "$step")" -lt "$expected" ];do if ! kill -0 "$app" 2>/dev/null;then status=0;wait "$app"||status=$?;app=;echo "workspace.hosts_native_exit.$step.$status" >&2;tail -12 "$log" >&2;exit 1;fi;[ "$attempt" -lt 80 ]||{ echo "workspace.hosts_timeout.$step" >&2;tail -12 "$log" >&2;exit 1;};attempt=$((attempt+1));sleep 0.1;done; }
start(){ round=$1;log="target/probes/workspace-hosts-native-$round.log";target/debug/pulseterm-workspace >"$log" 2>&1 & app=$!;await_count profiles_listed 1;xid=$(xdotool search --onlyvisible --all --pid "$app" --name '^PulseTerm · 工作区预览$' 2>/dev/null||true);[ "$(printf '%s\n' "$xid"|awk 'NF{n++}END{print n+0}')" -eq 1 ];[ "$(xdotool getwindowpid "$xid")" = "$app" ];timeout 2s xdotool windowactivate --sync "$xid"; }
shot(){ timeout 3s import -window "$xid" "target/probes/workspace-hosts-$1.png"; }
open(){
  n=$1;paint=0
  while :;do
    shot sidebar
    [ "$(convert target/probes/workspace-hosts-sidebar.png -format '%k' info:)" -gt 10 ]&&break
    [ "$paint" -lt 10 ]||{ echo workspace.hosts_paint_timeout >&2;exit 1; }
    paint=$((paint+1));sleep 0.1
  done
  convert target/probes/workspace-hosts-sidebar.png -crop 180x640+0+0 -resize 400% target/probes/workspace-hosts-sidebar-ocr.png
  timeout 3s tesseract target/probes/workspace-hosts-sidebar-ocr.png target/probes/workspace-hosts-sidebar --psm 11 tsv 2>/dev/null
  y=$(awk -F '\t' '$12~/^SSH/&&$7<320&&$8>600{n++;if(n==1)y=int(($8+$10/2)/4)}END{if(n==2)print y}' target/probes/workspace-hosts-sidebar.tsv)
  [ -n "$y" ]||{ echo workspace.hosts_heading_not_drawn >&2;exit 1; }
  timeout 2s xdotool mousemove --window "$xid" 151 "$y" click 1
  await_count hosts_operation_confirmed "$n";sleep 0.2
}
keys(){ timeout 3s xdotool key --clearmodifiers "$@"; }
type(){ timeout 3s xdotool type --clearmodifiers --delay 1 "$1"; }
drawn(){ name=$1;shift;shot "$name";timeout 3s tesseract "target/probes/workspace-hosts-$name.png" "target/probes/workspace-hosts-$name" --psm 11 2>/dev/null;for match in "$@";do grep -F "$match" "target/probes/workspace-hosts-$name.txt" >/dev/null||{ echo "workspace.hosts_not_drawn.$name" >&2;exit 1; };done; }
close_app(){ [ "$(count session_opened)" -eq 0 ];keys alt+F4;await_count exited 1;wait "$app";app=; }
start 1
open 1
keys shift+Tab shift+Tab shift+Tab shift+Tab shift+Tab ctrl+a
type HOSTONE;keys Tab ctrl+a;type EXAMPLE.COM;keys Tab ctrl+a;type 2222;keys Tab ctrl+a;type alice;keys Tab Return
await_count hosts_operation_confirmed 2
sleep 0.2
drawn created HOSTONE example.com 2222 alice
keys Escape
open 3
keys shift+Tab shift+Tab shift+Tab shift+Tab shift+Tab shift+Tab ctrl+a
type HOSTTWO;keys Tab ctrl+a;type example.net;keys Tab ctrl+a;type 2200;keys Tab ctrl+a;type bob;keys Tab Return
await_count hosts_operation_confirmed 4
sleep 0.2
drawn updated HOSTTWO example.net 2200 bob
keys Escape ctrl+shift+r
await_count document_confirmed 2;await_count profiles_listed 2
open 5
drawn refreshed HOSTTWO example.net 2200 bob
keys Escape
close_app
start 2
open 1
drawn reopened HOSTTWO example.net 2200 bob
keys shift+Tab Return
[ "$(count hosts_operation_confirmed)" -eq 1 ]
keys Return
await_count hosts_operation_confirmed 2
sleep 0.2
shot removed
keys Escape
close_app
start 3
open 1
shot deleted-reopened
timeout 3s tesseract target/probes/workspace-hosts-deleted-reopened.png target/probes/workspace-hosts-deleted-reopened --psm 11 2>/dev/null
! grep -F HOSTTWO target/probes/workspace-hosts-deleted-reopened.txt
grep -F 22 target/probes/workspace-hosts-deleted-reopened.txt >/dev/null
[ "$(count hosts_operation_confirmed)" -eq 1 ]
keys Escape
close_app
printf '%s\n' workspace.host_metadata_create_update_refresh_reopen_remove_confirmed
