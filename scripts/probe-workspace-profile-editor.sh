#!/bin/sh
# Task-owned automatic shell records exact native options without evaluating them.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-workspace-profile-editor.XXXXXX)
trap 'rm -rf "$fixture"; test ! -e "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache" XDG_RUNTIME_DIR="$fixture/runtime"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR" "$fixture/bin" "$fixture/work"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
printf '%s\n' "$$" >> "$PULSETERM_EDITOR_FIXTURE/pids"
printf '%s\0%s\0%s\0' "$PWD" "${PROFILE_VALUE-}" "${EDITOR_REMOVE-unset}" > "$PULSETERM_EDITOR_FIXTURE/receipt.$$"
for arg do printf '%s\0' "$arg" >> "$PULSETERM_EDITOR_FIXTURE/receipt.$$"; done
trap '/usr/bin/stty size < /dev/tty >> "$PULSETERM_EDITOR_FIXTURE/size.$$"' WINCH
/usr/bin/stty size < /dev/tty >> "$PULSETERM_EDITOR_FIXTURE/size.$$"
printf 'PROFILE EDITOR READY\r\n'
while :; do
  IFS= read -r line || continue
  printf '%s\n' "$line" >> "$PULSETERM_EDITOR_FIXTURE/input.$$"
  printf 'ECHO:%s\r\n' "$line"
  case "$line" in
    ALPHA_KEYS_*|ALPHA_COLORS_*) /usr/bin/python3 "$PULSETERM_EDITOR_FIXTURE/settings-query.py" "$line" "$$";;
  esac
done
SHELL
chmod 700 "$fixture/bin/sh"
export PULSETERM_EDITOR_FIXTURE="$fixture" EDITOR_REMOVE=inherited LC_ALL=C.UTF-8
PATH="$fixture/bin" SHELL="$fixture/bin/sh" /usr/bin/python3 scripts/probe-workspace-profile-editor.py "$fixture"
