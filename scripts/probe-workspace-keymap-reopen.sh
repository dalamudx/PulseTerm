#!/bin/sh
# Two real app processes reopen the same GUI-saved private source. No user directory discovery.
set -eu
mkdir -p /data/temp target/probes
umask 077
fixture=$(mktemp -d /data/temp/pulseterm-keymap-reopen.XXXXXX)
trap 'rm -rf "$fixture"' EXIT
printf 'owned-fixture-v1' > "$fixture/owned-keymap-reopen.fixture"
export PULSETERM_KEYMAP_REOPEN_ROOT="$fixture"
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture"
# Each child retains its original 80-second budget and completely drains before the next process.
timeout --kill-after=5s 80s sh scripts/probe-workspace-reload.sh terminal-keymap
file="$fixture/config/dev.pulseterm.workspace-preview/keymap.toml"
cp "$file" target/probes/keymap-process-reopen-before.toml
first_pid=$(cat "$fixture/native.pid")
if kill -0 "$first_pid" 2>/dev/null;then echo keymap.first_native_not_reaped >&2;exit 1;fi
rm "$fixture/native.pid" "$fixture/native.pid.input" "$fixture/native.pid.size"
timeout --kill-after=5s 80s sh scripts/probe-workspace-reload.sh terminal-keymap-reopen
cmp "$file" target/probes/keymap-process-reopen-before.toml
second_pid=$(cat "$fixture/native.pid")
[ "$first_pid" != "$second_pid" ] || { echo keymap.process_identity_not_changed >&2;exit 1; }
if kill -0 "$second_pid" 2>/dev/null;then echo keymap.second_native_not_reaped >&2;exit 1;fi
cp "$file" target/probes/keymap-process-reopen-after.toml
printf 'keymap.actual_two_application_processes_gui_saved_same_file_reopen_and_native_drain_confirmed\n'
