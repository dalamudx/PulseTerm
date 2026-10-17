#!/bin/sh
# Three backend-selected PTYs in an isolated, task-owned fixture.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-workspace-nested.XXXXXX)
trap 'rm -rf -- "$fixture"; test ! -e "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
export XDG_RUNTIME_DIR="$fixture/runtime"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR" "$fixture/bin"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
printf '%s\n' "$$" >> "$PULSETERM_NESTED_FIXTURE/pids"
trap '/usr/bin/stty size < /dev/tty >> "$PULSETERM_NESTED_FIXTURE/size.$$"' WINCH
/usr/bin/stty size < /dev/tty >> "$PULSETERM_NESTED_FIXTURE/size.$$"
printf 'NESTED FIXTURE READY\r\n'
while :; do
  IFS= read -r line || continue
  printf '%s\n' "$line" >> "$PULSETERM_NESTED_FIXTURE/input.$$"
  printf 'ECHO:%s\r\n' "$line"
done
SHELL
chmod 700 "$fixture/bin/sh"
export PULSETERM_NESTED_FIXTURE="$fixture" LC_ALL=C.UTF-8
PATH="$fixture/bin" SHELL="$fixture/bin/sh" /usr/bin/python3 scripts/probe-workspace-nested.py "$fixture"
