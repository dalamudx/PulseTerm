#!/bin/sh
set -eu
umask 077
mkdir -p /data/temp target/probes
config_probe_dir=$(mktemp -d /data/temp/workspace-ssh-config.XXXXXX)
cleanup(){ rm -rf "$config_probe_dir"; test ! -e "$config_probe_dir"; }
trap cleanup EXIT
export TMPDIR="$config_probe_dir" TMP="$config_probe_dir" TEMP="$config_probe_dir"
export XDG_DATA_HOME="$config_probe_dir/data" XDG_CONFIG_HOME="$config_probe_dir/config" XDG_CACHE_HOME="$config_probe_dir/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
export PULSE_CONFIG_PROBE_ROOT="$config_probe_dir" OMP_THREAD_LIMIT=1 PYTHONDONTWRITEBYTECODE=1
export PULSE_WORKSPACE_SSH_CONFIG_DIRECTORY="$config_probe_dir/ssh"
cc -std=c11 -Wall -Wextra -Werror scripts/probe-x11-status.c -o "$config_probe_dir/status-observer" $(pkg-config --cflags --libs atspi-2 gobject-2.0)
export PULSE_CONFIG_STATUS_OBSERVER="$config_probe_dir/status-observer"
export PULSE_CONFIG_PROBE_MODE="${1:-preview}"
python3 scripts/probe-workspace-ssh-config.py
