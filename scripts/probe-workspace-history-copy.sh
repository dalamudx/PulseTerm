#!/bin/sh
# Private live producer; compare actual X11 copied bytes across deep scrollback.
set -eu
umask 077
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-workspace-history-copy.XXXXXX)
trap 'rm -rf "$fixture"; test ! -e "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$fixture/bin"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
exec /usr/bin/python3 "$PULSETERM_HISTORY_FIXTURE/producer.py"
SHELL
chmod 700 "$fixture/bin/sh"
cat > "$fixture/producer.py" <<'PYTHON'
import os
from pathlib import Path
import sys
import termios

fixture = Path(os.environ['PULSETERM_HISTORY_FIXTURE'])
(fixture / 'native.pid').write_text(str(os.getpid()))
attributes = termios.tcgetattr(sys.stdin.fileno())
attributes[3] &= ~termios.ECHO
termios.tcsetattr(sys.stdin.fileno(), termios.TCSANOW, attributes)
lines = ['H%04d 漢字🙂e\u0301 ' % n + 'A' * 105 + ' END%04d' % n for n in range(600)]
block = 'BEGIN HISTORY\n' + '\n'.join(lines) + '\nEND HISTORY'
(fixture / 'expected.txt').write_bytes(block.encode('utf-8'))
for command in sys.stdin:
    with (fixture / 'input.txt').open('a') as log:
        log.write(command)
    if command == 'EMIT\n':
        sys.stdout.write(block + '\nHISTORY READY')
        sys.stdout.flush()
        (fixture / 'emitted').write_text('600')
PYTHON
export PULSETERM_HISTORY_FIXTURE="$fixture" LC_ALL=C.UTF-8
PATH="$fixture/bin" SHELL="$fixture/bin/sh" /usr/bin/python3 scripts/probe-workspace-history-copy.py "$fixture"
