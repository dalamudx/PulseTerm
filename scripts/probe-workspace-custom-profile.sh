#!/bin/sh
# Fixed task-owned executable; the product receives only its saved Profile identity.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-custom-profile.XXXXXX)
trap 'rm -rf "$fixture"; test ! -e "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache" XDG_RUNTIME_DIR="$fixture/runtime"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR" "$fixture/bin" "$fixture/work"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
ln -s /bin/sh "$fixture/bin/sh"
cat > "$fixture/bin/custom-profile" <<'PROGRAM'
#!/bin/sh
printf '%s\n' "$$" >> "$PROFILE_NATIVE_FIXTURE/pids"
printf '%s\0%s\0' "$PWD" "$TERM_PROGRAM" > "$PROFILE_NATIVE_FIXTURE/receipt.$$"
for arg do printf '%s\0' "$arg" >> "$PROFILE_NATIVE_FIXTURE/receipt.$$"; done
printf 'CUSTOM PROFILE READY\r\n'
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$PROFILE_NATIVE_FIXTURE/input.$$"
  printf 'ECHO:%s\r\n' "$line"
done
PROGRAM
chmod 700 "$fixture/bin/custom-profile"
export PROFILE_NATIVE_FIXTURE="$fixture" LC_ALL=C.UTF-8
PATH="$fixture/bin" SHELL="$fixture/bin/sh" /usr/bin/python3 scripts/probe-workspace-custom-profile.py "$fixture"
