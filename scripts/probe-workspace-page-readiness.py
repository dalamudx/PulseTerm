#!/usr/bin/env python3
"""Production WebView with opt-in delayed display receipts, real refresh and PTY input."""
import importlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

startup = importlib.import_module('probe-workspace-startup')
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/probes/workspace-page-readiness-native'
BINARY = Path(os.environ['CARGO_TARGET_DIR']) / 'debug/pulseterm-workspace'
HELPER = Path(os.environ['PULSE_NATIVE_HELPER'])


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    report = {'version': 1, 'passed': False, 'nativeExitConfirmed': False, 'samples': [],
              'platform': 'Linux Xvfb software rendering',
              'faultBuildOnly': True, 'delayMs': 6500,
              'historicalStartupFaultResolved': False}
    result = OUT / 'result.json'
    result.write_text(json.dumps(report, indent=2) + '\n')
    for mode in ('document', 'presentation'):
        with tempfile.TemporaryDirectory(prefix='pt-ready.', dir='/data/temp') as directory:
            fixture = Path(directory)
            env = os.environ.copy()
            for key, name in [('XDG_DATA_HOME', 'd'), ('XDG_CONFIG_HOME', 'c'),
                              ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'r'),
                              ('PULSE_WORKSPACE_STARTUP_TRACE_DIR', 'traces')]:
                folder = fixture / name
                folder.mkdir(mode=0o700)
                env[key] = str(folder)
            bindir = fixture / 'bin'
            bindir.mkdir(mode=0o700)
            shell = bindir / 'sh'
            shell.write_text('''#!/bin/sh
printf '%s\\n' "$$" > "$PULSE_READINESS_FIXTURE/native.pid"
printf 'READINESS_READY\\r\\n'
while IFS= read -r line; do
    printf '%s\\n' "$line" >> "$PULSE_READINESS_FIXTURE/native.input"
    printf 'echo:%s\\r\\n' "$line"
done
''')
            shell.chmod(0o700)
            env.update(PATH=str(bindir), SHELL=str(shell), TMPDIR=directory, TMP=directory,
                       TEMP=directory, PULSE_READINESS_FIXTURE=directory,
                       PULSE_WORKSPACE_READINESS_FAULT=mode)
            app = None
            owned = {}
            xid = None
            log = OUT / f'{mode}.log'
            try:
                with log.open('w') as output:
                    app = subprocess.Popen([str(BINARY)], env=env, stdout=output,
                                           stderr=subprocess.STDOUT, start_new_session=True)
                    gui_pid = app.pid
                    def count(event):
                        return log.read_text(errors='replace').splitlines().count('workspace.' + event)
                    def accessible(name, action=None):
                        return startup.run(HELPER, gui_pid, name, *([action] if action else []), check=False)
                    startup.wait(lambda: count('readiness_fault_started') == 1, 'fault_started', app)
                    fault_observed = time.monotonic()
                    xid = startup.wait(lambda: startup.run('xdotool', 'search', '--onlyvisible',
                                       '--all', '--pid', gui_pid, '--name', '^PulseTerm · 工作区预览$',
                                       check=False).stdout.strip(), 'window', app)
                    assert len(xid.splitlines()) == 1
                    assert int(startup.run('xdotool', 'getwindowpid', xid).stdout) == gui_pid
                    startup.run('xdotool', 'windowactivate', '--sync', xid)
                    message = ('页面验证未完成 · 请刷新工作区重试' if mode == 'document'
                               else '外部 CLI 就绪未确认 · 可重新加载工作区')
                    startup.wait(lambda: accessible(message).returncode == 0, 'timeout_message', app, seconds=10)
                    timeout_observed_ms = (time.monotonic() - fault_observed) * 1000
                    startup.run('import', '-window', xid, OUT / f'{mode}-timeout.png')
                    startup.wait(lambda: count('readiness_fault_finished') == 1, 'late_receipt', app)
                    # Observe beyond delivery of the real late native receipt, without refreshing.
                    deadline = time.monotonic() + .75
                    while time.monotonic() < deadline:
                        assert accessible(message).returncode == 0, 'readiness.late_receipt_changed_status'
                        assert count('profiles_listed') == (0 if mode == 'document' else 1)
                        assert count('terminal_input_ready') == 0
                    assert not (fixture / 'native.pid').exists()
                    before_profiles = count('profiles_listed')
                    before_cli = count('cli_presentation_ready')
                    assert accessible('重新加载工作区', 'activate').returncode == 0
                    startup.wait(lambda: count('document_confirmed') == 2, 'new_document', app)
                    startup.wait(lambda: count('profiles_listed') == before_profiles + 1, 'new_profiles', app)
                    startup.wait(lambda: count('cli_presentation_ready') == before_cli + 1, 'new_cli_ready', app)
                    startup.wait(lambda: accessible('选择本地 Profile 创建终端').returncode == 0, 'healthy_page', app)
                    assert count('readiness_fault_started') == count('readiness_fault_finished') == 1
                    assert accessible('POSIX sh', 'activate').returncode == 0
                    startup.wait(lambda: count('terminal_input_ready') == 1, 'terminal_ready', app)
                    startup.wait(lambda: accessible('Terminal input', 'focus').returncode == 0,
                                 'terminal_input_focus', app)
                    text = 'READINESS_' + mode.upper()
                    startup.run('xdotool', 'type', '--clearmodifiers', '--delay', '1', text)
                    startup.run('xdotool', 'key', '--clearmodifiers', 'Return')
                    input_path = fixture / 'native.input'
                    startup.wait(lambda: input_path.exists() and input_path.read_text() == text + '\n',
                                 'real_input', app)
                    native_pid = int((fixture / 'native.pid').read_text())
                    owned.update(startup.tree(gui_pid))
                    assert native_pid in owned and len(owned) >= 4
                    startup.run('import', '-window', xid, OUT / f'{mode}-recovered.png')
                    startup.run('xdotool', 'key', '--clearmodifiers', 'alt+F4')
                    app.wait(timeout=12)
                    assert app.returncode == 0 and count('exited') == 1
                    startup.wait(lambda: all(startup.identity(pid) != token for pid, token in owned.items()),
                                 'native_exit', seconds=5)
                    assert input_path.read_bytes() == (text + '\n').encode()
                    (OUT / f'{mode}.input').write_bytes(input_path.read_bytes())
                    traces = list((fixture / 'traces').glob('*.jsonl'))
                    assert len(traces) == 1
                    rows = [json.loads(line) for line in traces[0].read_text().splitlines()]
                    assert all(row['pid'] == gui_pid for row in rows) and len(rows) <= 64
                    stages = [row['stage'] for row in rows]
                    assert stages.count('document_confirmed') == 2
                    assert stages.count('local_page_finished') == 2
                    assert stages[-1] == 'exited'
                    (OUT / f'{mode}.jsonl').write_bytes(traces[0].read_bytes())
                    report['samples'].append({'mode': mode, 'guiPid': gui_pid, 'nativePid': native_pid,
                                              'timeoutObservedMsIncludingAtspi': timeout_observed_ms,
                                              'lateReceiptDidNotAdvance': True, 'explicitReloadRecovered': True,
                                              'exitCode': app.returncode, 'nativeExitConfirmed': True,
                                              'finalInput': input_path.read_text()})
                    result.write_text(json.dumps(report, indent=2) + '\n')
            except Exception as error:
                report['failure'] = str(error) or type(error).__name__
                result.write_text(json.dumps(report, indent=2) + '\n')
                if xid is not None:
                    startup.run('import', '-window', xid, OUT / f'{mode}-failure.png', check=False)
                for trace in (fixture / 'traces').glob('*.jsonl'):
                    (OUT / f'{mode}-failure.jsonl').write_bytes(trace.read_bytes())
                raise
            finally:
                if app is not None and app.poll() is None:
                    owned.update(startup.tree(app.pid))
                    os.killpg(app.pid, signal.SIGTERM)
                    try:
                        app.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(app.pid, signal.SIGKILL)
                        app.wait(timeout=5)
                for pid, token in owned.items():
                    if startup.identity(pid) == token:
                        try:
                            os.kill(pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                assert all(startup.identity(pid) != token for pid, token in owned.items()), 'readiness.cleanup_failed'
    assert len(report['samples']) == 2
    report.update(passed=True, nativeExitConfirmed=True)
    result.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
