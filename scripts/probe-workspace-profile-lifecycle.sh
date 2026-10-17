#!/bin/sh
# Private automatic Profile; deletion/restoration never owns live sessions.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-workspace-profile-lifecycle.XXXXXX)
trap 'rm -rf "$fixture"; test ! -e "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$fixture/bin"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
printf '%s\n' "$$" >> "$PULSETERM_LIFECYCLE_FIXTURE/pids"
printf 'PROFILE LIFECYCLE READY\r\n'
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$PULSETERM_LIFECYCLE_FIXTURE/input"
  printf 'ECHO:%s\r\n' "$line"
done
SHELL
chmod 700 "$fixture/bin/sh"
export PULSETERM_LIFECYCLE_FIXTURE="$fixture" LC_ALL=C.UTF-8
PATH="$fixture/bin" SHELL="$fixture/bin/sh" /usr/bin/python3 scripts/probe-workspace-profile-lifecycle.py "$fixture"
