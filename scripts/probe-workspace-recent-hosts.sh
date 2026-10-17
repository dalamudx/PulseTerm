#!/bin/sh
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pulseterm-workspace-ssh-session.XXXXXX)
printf 'owned-fixture-v1' > "$fixture/owned-workspace-ssh-session.fixture"
cleanup(){ rm -rf -- "$fixture"; test ! -e "$fixture"; }
trap cleanup EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
if [ "${PULSE_CLI_ROUTE:-0}" = "1" ]; then export PULSE_SSH_AGENT_ROUTE=1; fi
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
if [ "${PULSE_SSH_AGENT_ROUTE:-0}" = "1" ]; then export PULSE_SSH_AGENT=1 PULSE_SSH_ROUTE=1; fi
if [ "${PULSE_CLI_AGENT:-0}" = "1" ] || [ "${PULSE_CLI_ROUTE:-0}" = "1" ]; then
  export PULSE_SSH_AGENT=1 XDG_RUNTIME_DIR="$fixture/runtime"
  mkdir -m 700 "$XDG_RUNTIME_DIR"
fi
if [ "${PULSE_SSH_AGENT:-0}" = "1" ]; then export SSH_AUTH_SOCK="$fixture/agent.sock"; fi
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
python3 scripts/probe-workspace-recent-hosts.py
