#!/bin/sh
# Shared private fixture for the five GUI authentication scenarios.
set -eu
mode=${1:-ssh}
[ "$#" -le 1 ] || exit 2
case "$mode" in ssh|key|password|agent|temporary) ;; *) echo 'unknown authentication scenario' >&2; exit 2;; esac
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pulseterm-workspace-ssh-session.XXXXXX)
cleanup() {
  case "$fixture" in /data/temp/pulseterm-workspace-ssh-session.??????) rm -rf -- "$fixture";; *) exit 2;; esac
}
trap cleanup EXIT
printf 'owned-fixture-v1' > "$fixture/owned-workspace-ssh-session.fixture"
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache" XDG_RUNTIME_DIR="$fixture/runtime"
export GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=0 GTK_A11Y=1
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
mkdir -p "target/probes/workspace-$mode-interactive"
cc -Wall -Wextra -Werror scripts/probe-x11-status.c -o "$fixture/probe-x11-status" $(pkg-config --cflags --libs atspi-2 gio-2.0 gobject-2.0 glib-2.0)
python3 "scripts/probe-workspace-$mode-interactive.py" "$fixture"
