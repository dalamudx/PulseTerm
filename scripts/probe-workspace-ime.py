#!/usr/bin/env python3
"""Real IBus/libpinyin candidates and commits in the production GTK WebView."""
import importlib
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import time
import tty
from typing import Any

startup = importlib.import_module('probe-workspace-startup')
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/probes/workspace-ime'


def worker(directory):
    fixture = Path(directory)
    received = bytearray()
    tty.setraw(0)
    os.write(1, b'IME_READY\r\n')
    (fixture / 'worker.pid').write_text(str(os.getpid()))
    deadline = time.monotonic() + 150
    try:
        while not (fixture / 'finish').exists():
            assert time.monotonic() < deadline, 'ime.worker_timeout'
            if select.select([0], [], [], .02)[0]:
                chunk = os.read(0, 2048)
                assert chunk, 'ime.worker_eof'
                received.extend(chunk)
                assert len(received) <= 1024, 'ime.input_budget'
                os.write(1, chunk)
            pending = fixture / 'input.pending'
            pending.write_text(json.dumps({'pid': os.getpid(), 'hex': received.hex()}))
            pending.replace(fixture / 'input.json')
    finally:
        (fixture / 'finished.json').write_text(json.dumps({'pid': os.getpid(), 'hex': received.hex()}))


def main():
    fixture = Path(os.environ['PULSE_IME_FIXTURE'])
    OUT.mkdir(parents=True, exist_ok=True)
    result = OUT / 'result.json'
    report: dict[str, Any] = dict(version=1, passed=False, scope='Linux production WebView, real IBus/libpinyin via XTest',
                  limitations=['Isolated Xvfb software rendering, not a physical desktop',
                               'Pinyin only; other CJK engines, candidate DPI and other platforms remain open'])
    result.write_text(json.dumps(report, indent=2) + '\n')
    env = os.environ.copy()
    for key, folder in [('HOME', 'home'), ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                        ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        path = fixture / folder
        path.mkdir(mode=0o700, exist_ok=True)
        path.chmod(0o700)
        env[key] = str(path)
    env.update(TMPDIR=str(fixture), TMP=str(fixture), TEMP=str(fixture), GTK_IM_MODULE='ibus',
               IBUS_ADDRESS='unix:path=' + str(fixture / 'ibus.sock'), IBUS_USE_PORTAL='0', XMODIFIERS='@im=ibus')
    schemas = fixture / 'schemas'
    schemas.mkdir()
    env['GSETTINGS_SCHEMA_DIR'] = str(schemas)
    def run(*args, check=True):
        return subprocess.run(list(map(str, args)), env=env, check=check, capture_output=True, text=True, timeout=4)
    run('glib-compile-schemas', '--targetdir', schemas, '/usr/share/glib-2.0/schemas')
    queries = list(Path('/usr/lib').glob('*/libgtk-3-0*/gtk-query-immodules-3.0'))
    assert queries, 'ime.missing_gtk_module_query'
    modules = list(Path('/usr/lib').glob('*/gtk-3.0/3.0.0/immodules/im-ibus.so'))
    assert len(modules) == 1, 'ime.missing_gtk_ibus_module'
    module_file = fixture / 'immodules.cache'
    module_file.write_text(run(queries[0], modules[0]).stdout)
    env['GTK_IM_MODULE_FILE'] = str(module_file)
    bindir = fixture / 'bin'
    bindir.mkdir()
    shell = bindir / 'sh'
    shell.write_text('#!/bin/sh\nexec /usr/bin/python3 "' + str(Path(__file__).resolve()) + '" worker "$PULSE_IME_FIXTURE"\n')
    shell.chmod(0o700)
    helper = Path(env['PULSE_NATIVE_HELPER'])
    binary = Path(env['CARGO_TARGET_DIR']) / 'debug/pulseterm-workspace'
    daemon = app = None
    owned = {}
    xid = None
    try:
        with (OUT / 'ibus.log').open('w') as ibus_log, (OUT / 'workspace.log').open('w') as app_log:
            daemon = subprocess.Popen(['ibus-daemon', '--address', env['IBUS_ADDRESS'], '--cache=none',
                                       '--emoji-extension=disable'], env=env, stdout=ibus_log,
                                      stderr=subprocess.STDOUT, start_new_session=True)
            startup.wait(lambda: run('ibus', 'list-engine', check=False).returncode == 0, 'ibus_ready', daemon)
            report['ibusVersion'] = run('ibus', 'version').stdout.strip()
            app = subprocess.Popen([str(binary)], env={**env, 'PATH': str(bindir), 'SHELL': str(shell)},
                                   stdout=app_log, stderr=subprocess.STDOUT, start_new_session=True)
            def count(name):
                return (OUT / 'workspace.log').read_text().splitlines().count('workspace.' + name)
            def accessible(name, action=None):
                return run(helper, app.pid, name, *([action] if action else []), check=False)
            xid = startup.wait(lambda: run('xdotool', 'search', '--onlyvisible', '--all', '--pid', app.pid,
                                          '--name', '^PulseTerm · 工作区预览$', check=False).stdout.strip(), 'window', app)
            assert len(xid.splitlines()) == 1
            run('xdotool', 'windowactivate', '--sync', xid)
            startup.wait(lambda: accessible('POSIX sh', 'activate').returncode == 0, 'profile', app)
            startup.wait(lambda: count('terminal_input_ready') == 1 and (fixture / 'input.json').exists(), 'terminal_ready', app)
            native_pid = int((fixture / 'worker.pid').read_text())
            native_token = startup.identity(native_pid)
            assert native_token is not None, 'ime.native_worker_missing'
            owned[native_pid] = native_token
            def focus():
                startup.wait(lambda: accessible('Terminal input', 'focus').returncode == 0, 'terminal_focus', app)
            def raw():
                return bytes.fromhex(json.loads((fixture / 'input.json').read_text())['hex'])
            def unchanged(expected):
                deadline = time.monotonic() + .4
                while time.monotonic() < deadline:
                    assert raw() == expected, 'ime.unexpected_input.' + raw().hex()
                    time.sleep(.02)
            def activate():
                setter = run('ibus', 'engine', 'libpinyin', check=False)
                startup.wait(lambda: run('ibus', 'engine', check=False).stdout.strip() == 'libpinyin', 'pinyin_selected', daemon)
                report['engineSetterExitCode'] = setter.returncode
            def panels():
                visible = set()
                for pid in startup.tree(daemon.pid):
                    try:
                        if (Path('/proc') / str(pid) / 'comm').read_text().strip() != 'ibus-ui-gtk3':
                            continue
                    except FileNotFoundError:
                        continue
                    candidate = run(helper, pid, '中文', 'inspect', check=False)
                    if candidate.returncode == 0 and 'showing=1' in candidate.stdout:
                        visible.update(run('xdotool', 'search', '--onlyvisible', '--pid', pid, check=False).stdout.split())
                return visible
            focus()
            activate()
            def candidate(tag, expected):
                startup.wait(lambda: not panels(), 'previous_candidate_hidden_' + tag, app)
                before = panels()
                run('xdotool', 'type', '--clearmodifiers', '--delay', '100', 'zhongwen')
                visible = startup.wait(lambda: panels() - before, 'candidate_' + tag, app)
                unchanged(expected)
                run('import', '-window', 'root', OUT / (tag + '-candidate.png'))
                report.setdefault('candidateWindows', []).append(sorted(visible))
            candidate('cancel', b'')
            run('xdotool', 'key', '--clearmodifiers', 'Escape')
            unchanged(b'')
            startup.wait(lambda: not panels(), 'cancel_candidate_hidden', app)
            run('import', '-window', 'root', OUT / 'cancelled.png')
            report['candidateEscapeZeroInput'] = True
            candidate('space', b'')
            run('xdotool', 'key', '--clearmodifiers', 'space')
            expected = '中文'.encode()
            startup.wait(lambda: raw() == expected, 'first_commit', app)
            unchanged(expected)
            startup.wait(lambda: not panels(), 'space_candidate_hidden', app)
            report['spaceCommitsExactlyOnce'] = True
            candidate('digit', expected)
            run('xdotool', 'key', '--clearmodifiers', '1')
            expected = '中文中文'.encode()
            startup.wait(lambda: raw() == expected, 'digit_commit', app)
            unchanged(expected)
            startup.wait(lambda: not panels(), 'digit_candidate_hidden', app)
            report['digitCommitsExactlyOnce'] = True
            run('import', '-window', xid, OUT / 'committed.png')
            before = count('terminal_input_ready')
            run('xdotool', 'key', '--clearmodifiers', 'ctrl+shift+r')
            startup.wait(lambda: count('terminal_input_ready') == before + 1, 'refreshed_view', app)
            focus()
            unchanged(expected)
            report['refreshSendsNoDuplicateInput'] = True
            run('xdotool', 'key', '--clearmodifiers', 'Return')
            expected += b'\r'
            startup.wait(lambda: raw() == expected, 'plain_return_after_composition', app)
            (fixture / 'finish').touch()
            startup.wait(lambda: (fixture / 'finished.json').exists(), 'worker_finished', app)
            finished = json.loads((fixture / 'finished.json').read_text())
            assert bytes.fromhex(finished['hex']) == expected
            shutil.copyfile(fixture / 'finished.json', OUT / 'final-input.json')
            owned.update(startup.tree(app.pid))
            report['guiPid'] = app.pid
            run('xdotool', 'windowactivate', '--sync', xid)
            run('xdotool', 'key', '--clearmodifiers', 'alt+F4')
            app.wait(timeout=12)
            assert app.returncode == 0, 'ime.gui_exit'
            startup.wait(lambda: all(startup.identity(pid) != token for pid, token in owned.items()), 'native_tree_exit', None)
            report.update(guiExitCode=app.returncode, ptyWorkerExited=True, nativeTreeExited=True,
                          finalInputHex=expected.hex(), finalInputUtf8=expected.decode())
            owned.update(startup.tree(daemon.pid))
            run('ibus', 'exit', check=False)
            daemon.wait(timeout=5)
            assert daemon.returncode == 0, 'ime.ibus_exit'
            startup.wait(lambda: all(startup.identity(pid) != token for pid, token in owned.items()), 'ime_tree_exit')
            report['imeProcessTreeExited'] = True
            report.update(ibusExitCode=daemon.returncode, passed=True)
    except Exception as error:
        report['failure'] = str(error) or type(error).__name__
        if (fixture / 'input.json').exists():
            shutil.copyfile(fixture / 'input.json', OUT / 'failure-input.json')
        run('import', '-window', 'root', OUT / 'failure.png', check=False)
        raise
    finally:
        for process in (app, daemon):
            if process is not None:
                owned.update(startup.tree(process.pid))
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=4)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait(timeout=4)
        for pid, token in owned.items():
            if startup.identity(pid) == token:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        result.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
        assert all(startup.identity(pid) != token for pid, token in owned.items()), 'ime.cleanup_failed'
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == 'worker':
        worker(sys.argv[2])
    else:
        main()
