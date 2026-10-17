#!/bin/sh
# Isolated OS runtime and real X11/WebView; no user display or existing socket is touched.
set -eu
mkdir -p /data/temp target/probes
scratch=$(mktemp -d /data/temp/pulseterm-cli-accept.XXXXXX)
trap 'rm -rf "$scratch"' EXIT
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch"
if [ "${PULSE_CLI_SINGLE_INSTANCE_PROBE:-0}" = 1 ];then : > target/probes/workspace-single-instance-result.txt;fi
export XDG_RUNTIME_DIR="$scratch/runtime"
mkdir -m 700 "$XDG_RUNTIME_DIR"
# Reject syntax before reading runtime/secret; never start GUI or create missing endpoints.
for argument in help settings arbitrary-command; do
  status=0;target/debug/pulse "$argument" >/dev/null 2>&1 || status=$?
  [ "$status" -eq 2 ] || { echo workspace.cli_unknown_argument >&2;exit 1; }
done
for extra in /bin/sh --program=/bin/sh --all;do
  status=0;target/debug/pulse profiles "$extra" >/dev/null 2>&1 || status=$?
  [ "$status" -eq 2 ] || { echo workspace.cli_profiles_accepted_extra >&2;exit 1; }
done
status=0;target/debug/pulse profiles >/dev/null 2>&1 || status=$?
[ "$status" -eq 1 ] || { echo workspace.cli_profiles_unexpected_cold_start >&2;exit 1; }
status=0;target/debug/pulse focus --program=/bin/sh >/dev/null 2>&1 || status=$?
[ "$status" -eq 2 ] || { echo workspace.cli_accepted_program >&2;exit 1; }
status=0;target/debug/pulse focus >/dev/null 2>&1 || status=$?
[ "$status" -eq 1 ] || { echo workspace.cli_unexpected_cold_start >&2;exit 1; }
[ ! -e "$XDG_RUNTIME_DIR/pulseterm-workspace-cli" ] || { echo workspace.cli_created_runtime >&2;exit 1; }
# This process group owns its Openbox, WebView, decoy window, PTY, and socket.
timeout --kill-after=5s 90s xvfb-run -a -s '-screen 0 1280x800x24' dbus-run-session -- sh scripts/probe-window-x11.sh target/probes/workspace-terminal-cli-wm.log sh scripts/probe-workspace-reload.sh terminal-cli
[ ! -e "$XDG_RUNTIME_DIR/pulseterm-workspace-cli/control.sock" ]
[ ! -e "$XDG_RUNTIME_DIR/pulseterm-workspace-cli/control.token" ]
[ "$(wc -l < target/probes/workspace-cli-focus-result.txt)" -eq 3 ]
if [ "${PULSE_CLI_SINGLE_INSTANCE_PROBE:-0}" = 1 ];then
 [ "$(wc -l < target/probes/workspace-single-instance-result.txt)" -eq 3 ]
 [ "$(grep -c '^workspace.single_instance_action_unavailable$' target/probes/workspace-reload-native.log)" -eq 3 ]
 echo workspace.single_instance_three_pages_same_primary_pty_endpoint_and_safe_focus_confirmed
fi
