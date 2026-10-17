#!/bin/sh
# Real Bash/GNU Readline in a private PTY; never source the current user's files.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-workspace-bracketed-paste.XXXXXX)
trap 'rm -rf "$fixture"; test ! -e "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$fixture/bin"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
printf 'set enable-bracketed-paste on\n' > "$fixture/inputrc"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
exec /usr/bin/bash --noprofile --rcfile "$PULSETERM_BRACKET_RC" -i
SHELL
chmod 700 "$fixture/bin/sh"
cat > "$fixture/bashrc" <<'BASH'
unset PROMPT_COMMAND
HISTFILE=/dev/null
set +o history
PS1='BRACKET> '
fixture_mode=on
fixture_count=0
fixture_capture_count=0
printf '%s\n' "$$" > "$PULSETERM_BRACKET_FIXTURE.pid"
: > "$PULSETERM_BRACKET_FIXTURE.records"
fixture_record() {
  printf '%s\0' "$1" >> "$PULSETERM_BRACKET_FIXTURE.records"
  fixture_count=$((fixture_count + 1))
  printf 'APPLICATION RECEIPT %d\n' "$fixture_count"
}
fixture_capture() {
  printf '%s' "$READLINE_LINE" > "$PULSETERM_BRACKET_FIXTURE.buffer"
  printf '%s' "$fixture_mode" > "$PULSETERM_BRACKET_FIXTURE.mode"
  fixture_capture_count=$((fixture_capture_count + 1))
  printf '%d\n' "$fixture_capture_count" > "$PULSETERM_BRACKET_FIXTURE.captured"
}
fixture_switch() {
  case "$1" in on|off) ;; *) return 1 ;; esac
  bind "set enable-bracketed-paste $1" || return 1
  fixture_mode=$1
  bind -v > "$PULSETERM_BRACKET_FIXTURE.bindings"
  fixture_record "MODE_$1"
  printf '\n  BRACKET MODE %s READY\n' "$1"
}
bind 'set enable-bracketed-paste on' || exit 1
bind -x '"\C-x\C-b": fixture_capture' || exit 1
bind -v > "$PULSETERM_BRACKET_FIXTURE.bindings"
printf 'Bash GNU Readline bracketed paste fixture\n'
BASH
export PULSETERM_BRACKET_FIXTURE="$fixture/native" PULSETERM_BRACKET_RC="$fixture/bashrc"
export INPUTRC="$fixture/inputrc" LC_ALL=C.UTF-8
PATH="$fixture/bin" SHELL="$fixture/bin/sh" /usr/bin/python3 scripts/probe-workspace-bracketed-paste.py "$fixture"
