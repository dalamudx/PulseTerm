#!/usr/bin/env python3
"""Production WebView + real cat 1GiB + tty SIGINT; Linux/X11 only."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

KIND = os.environ.get('PULSE_LOAD_KIND', 'load')
assert KIND in ('load', 'echo'), 'load.invalid_kind'
VERIFY = os.environ.get("PULSE_LOAD_VERIFY", "0") == "1"
assert not VERIFY or KIND == "load", "load.invalid_verification_kind"
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/probes/workspace-load'
label = os.environ.get('PULSE_LOAD_RUN')
if label:
    assert label.replace('-', '').isalnum(), 'load.invalid_run_label'
    OUT /= label
OUT.mkdir(parents=True, exist_ok=True)
RESULT = OUT / 'result.json'
RESULT.unlink(missing_ok=True)
BINARY = Path(os.environ['CARGO_TARGET_DIR']) / 'release/pulseterm-workspace'
HELPER = Path(os.environ.get("PULSE_NATIVE_HELPER", str(ROOT / "target/probes/x11-status")))


def run(*args, check=True):
    return subprocess.run(list(map(str, args)), check=check, capture_output=True, text=True, timeout=4)


def identity(pid):
    try:
        parts = (Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()
        return parts[19] if parts[0] != 'Z' else None
    except (FileNotFoundError, ProcessLookupError): return None


def tree(pid):
    pending, seen = [pid], set()
    while pending:
        current = pending.pop()
        if current in seen: continue
        seen.add(current)
        for file in (Path('/proc') / str(current) / 'task').glob('*/children'):
            try: pending.extend(map(int, file.read_text().split()))
            except FileNotFoundError: pass
    return {pid: value for pid in seen if (value := identity(pid)) is not None}


def deadline_signal(_number, _frame):
    raise TimeoutError('load.external_deadline')


signal.signal(signal.SIGTERM, deadline_signal)
with tempfile.TemporaryDirectory(prefix='workspace-load.', dir='/data/temp') as directory:
    fixture = Path(directory)
    env = dict(os.environ)
    for key, folder in [('HOME', 'home'), ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                        ('XDG_RUNTIME_DIR', 'runtime'), ('XDG_CACHE_HOME', 'cache')]:
        value = fixture / folder
        value.mkdir(mode=0o700)
        env[key] = str(value)
    bindir = fixture / 'bin'
    bindir.mkdir(mode=0o700)
    run('/usr/bin/install', '-m', '700', Path(os.environ.get('PULSE_LOAD_SHELL', str(ROOT / 'target/probes/load-shell'))), bindir / 'sh')
    env.update(PATH=str(bindir), SHELL=str(bindir / 'sh'), PULSE_LOAD_FIXTURE=directory,
               TMPDIR=directory, TMP=directory, TEMP=directory)
    row = b'LOAD ' + b'0123456789abcdef' * 3 + b'0123456789\n'
    assert len(row) == 64
    with (fixture / 'text-1gib').open('wb') as data:
        for _ in range(16384): data.write(row * 1024)
    assert (fixture / 'text-1gib').stat().st_size == 1073741824
    log_path = OUT / 'native.log'
    report = {'version': 2 if VERIFY else 1, 'passed': False, 'measurementsComplete': False,
              'sourceCommit': run('git', 'rev-parse', 'HEAD').stdout.strip(),
              'platform': 'linux', 'build': 'release', 'display': 'X11 Xvfb software rendering',
              'textBytes': 1073741824, 'command': '/bin/cat',
              'limitations': ['No echo p95, cold-start, hardware, IME or cross-platform acceptance',
                              'Visible screen verification only; not full hidden parser/history state' if VERIFY else 'Final marker verification; no full parser-state comparison']}
    app, xid, owned = None, None, {}

    def wait(predicate, name, seconds=15):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if app is not None and app.poll() is not None: raise RuntimeError('load.app_exited.' + name)
            value = predicate()
            if value: return value
            time.sleep(.025)
        raise TimeoutError('load.timeout.' + name)

    def read(name):
        file = fixture / (name + '.json')
        return json.loads(file.read_text()) if file.exists() else None

    def count(name):
        return log_path.read_text(errors='replace').splitlines().count('workspace.' + name)

    def accessible(name, action='inspect'):
        assert app is not None
        return run(HELPER, app.pid, name, action, check=False)

    def xdo(*args, check=True): return run('/usr/bin/xdotool', *args, check=check)

    with log_path.open('w') as log:
        try:
            app = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            wait(lambda: count('profiles_listed') == 1, 'profiles', 30)
            app_pid = app.pid
            xid = wait(lambda: xdo('search', '--onlyvisible', '--all', '--pid', app_pid, '--name', '^PulseTerm · 工作区预览$', check=False).stdout.strip(), 'window')
            assert len(xid.splitlines()) == 1
            xdo('windowactivate', '--sync', xid)
            wait(lambda: accessible('POSIX sh', 'activate').returncode == 0, 'start_profile')
            wait(lambda: count('terminal_input_ready') == 1, 'input_ready', 30)
            owned.update(tree(app.pid))
            assert accessible('Terminal input', 'focus').returncode == 0
            if KIND == 'echo':
                import runpy
                runpy.run_path(str(ROOT / 'scripts/probe-workspace-echo.py'))['verify'](globals())
            else:
                xdo('type', '--clearmodifiers', '--delay', '0', 'I')
                interruption = wait(lambda: read('interrupt-running'), 'interrupt_running')
                owned.update(tree(app.pid))
                time.sleep(.15)
                assert identity(interruption['pid']) is not None, 'load.cat_not_running_before_interrupt'
                sent = time.monotonic_ns()
                xdo('key', '--clearmodifiers', '--delay', '0', 'ctrl+c')
                interrupted = wait(lambda: read('interrupt-finished'), 'interrupt_finished', 5)
                assert interrupted['signal'] == signal.SIGINT and interrupted['signalNs'] >= sent
                (OUT / 'interrupt-native.json').write_text(json.dumps(interrupted, indent=2) + '\n')
                latency_ms = (interrupted['exitedNs'] - sent) / 1000000
                report.update(interruptChildExitMs=latency_ms, interruptSignalMs=(interrupted['signalNs']-sent)/1000000,
                              interruptUnder100Ms=latency_ms < 100, interruptedCatSignal=interrupted['signal'])
                xdo('type', '--clearmodifiers', '--delay', '0', 'F')
                complete_running = wait(lambda: read('full-running'), 'full_running')
                owned.update(tree(app.pid))
                full_seconds = int(os.environ.get('PULSE_LOAD_DIAGNOSTIC_SECONDS', '600'))
                assert 1 <= full_seconds <= 600
                report['diagnosticOnly'] = full_seconds != 600
                progress = []

                def full_progress():
                    now = time.monotonic()
                    # One sample per second; the key/exit latency above has already been measured.
                    if not progress or now - progress[-1]['observedSeconds'] >= 1:
                        snapshot = {'observedSeconds': now, 'processCpuSeconds': {}}
                        try:
                            io = (Path('/proc') / str(complete_running['pid']) / 'io').read_text()
                            snapshot['catIo'] = {key: int(value) for key, value in (line.split(':', 1) for line in io.splitlines())}
                        except (FileNotFoundError, ProcessLookupError): pass
                        for pid in tree(app_pid):
                            try:
                                stat = (Path('/proc') / str(pid) / 'stat').read_text()
                                fields = stat.rsplit(')', 1)[1].split()
                                snapshot['processCpuSeconds'][str(pid)] = {
                                    'name': stat.split('(', 1)[1].rsplit(')', 1)[0],
                                    'seconds': (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK')}
                            except (FileNotFoundError, ProcessLookupError): pass
                        progress.append(snapshot)
                    return read('full-finished')

                report['fullOutputProgress'] = progress
                complete = wait(full_progress, 'full_1gib', full_seconds)
                assert complete['exitCode'] == 0 and complete['signal'] == 0
                assert complete_running['pid'] == complete['pid']
                (OUT / 'full-native.json').write_text(json.dumps(complete, indent=2) + '\n')
                report['fullCatElapsedMs'] = (complete['exitedNs'] - complete['startedNs']) / 1000000
                image = OUT / 'complete.png'
                text = OUT / 'complete-ocr'
                ocr_image = OUT / 'complete-ocr.png'

                def painted():
                    run('/usr/bin/import', '-window', xid, image)
                    # The fixed single terminal ends at the window bottom; omit repeated flood rows from OCR.
                    run('/usr/bin/convert', image, '-gravity', 'South', '-crop', '100%x25%+0+0', '+repage', '-resize', '300%', ocr_image)
                    run('/usr/bin/env', 'OMP_THREAD_LIMIT=1', '/usr/bin/tesseract', ocr_image, text, '--psm', '6')
                    return 'load complete 1073741824' in text.with_suffix('.txt').read_text().lower()

                wait(painted, 'painted_completion', 30)
                report.update(measurementsComplete=True, full1GiBCatExitedNormally=True, finalMarkerPainted=True)
                if VERIFY:
                    prefix = 'workspace.load_screen_equal:'
                    assert not any(line.startswith(prefix) for line in log_path.read_text().splitlines())
                    xdo('key', '--clearmodifiers', 'ctrl+shift+F8')
                    def screen_receipt():
                        values = [line[len(prefix):] for line in log_path.read_text().splitlines() if line.startswith(prefix)]
                        assert len(values) <= 1, 'load.duplicate_screen_receipt'
                        return values[0] if values else None
                    value = wait(screen_receipt, 'original_screen_comparison', 15)
                    fields = value.split(':')
                    assert len(fields) == 6
                    columns, rows, revision, source_bytes, utf8_bytes = map(int, fields[:5])
                    assert columns > 0 and rows > 0 and revision > 0
                    assert source_bytes >= 1073741824 and utf8_bytes >= 1073741824
                    assert len(fields[5]) == 16 and all(char in '0123456789abcdef' for char in fields[5])
                    assert count('load_marker_admitted') == 1
                    report['originalScreenEqual'] = True
                    report['screenComparison'] = dict(columns=columns, rows=rows, revision=revision, sourceBytes=source_bytes, utf8Bytes=utf8_bytes, digest=fields[5], beforeAnyRefresh=True)
                    xdo('type', '--clearmodifiers', '--delay', '0', 'Q')
                    final_input = wait(lambda: read('load-input-finished'), 'final_native_input', 5)
                    assert final_input['count'] == 3 and final_input['bytesHex'] == '494651'
                    (OUT / 'input-final.json').write_text(json.dumps(final_input, indent=2) + '\n')
                    wait(lambda: identity(final_input['pid']) is None, 'native_fixture_exit', 5)
                    report['fixtureInputBytesHex'] = final_input['bytesHex']
                    report['fixtureInputExact'] = True
                    report['fixtureExitedBeforeWindowClose'] = True
            xdo('key', '--clearmodifiers', 'alt+F4')
            app.wait(timeout=15)
            assert app.returncode == 0 and count('exited') == 1
            report['normalAppExit'] = True
        except Exception:
            if xid is not None: run('/usr/bin/import', '-window', xid, OUT / 'failure.png', check=False)
            raise
        finally:
            if app is not None:
                owned.update(tree(app.pid))
                if app.poll() is None:
                    try:
                        if xid is not None:
                            xdo('windowactivate', xid, check=False)
                            xdo('key', '--clearmodifiers', 'alt+F4', check=False)
                        else: app.terminate()
                    except (subprocess.SubprocessError, ProcessLookupError):
                        pass
                    try: app.wait(timeout=15)
                    except subprocess.TimeoutExpired:
                        os.killpg(app.pid, signal.SIGKILL)
                        app.wait(timeout=3)
                        report['forcedAppCleanup'] = True
                deadline = time.monotonic() + 5
                while any(identity(pid) == value for pid, value in owned.items()) and time.monotonic() < deadline: time.sleep(.05)
                remaining = [(pid, value) for pid, value in owned.items() if identity(pid) == value]
                if remaining:
                    for pid, value in remaining:
                        if identity(pid) == value: os.kill(pid, signal.SIGKILL)
                    report['forcedNativeCleanup'] = True
                report['nativeProcessesDrained'] = not remaining
            target_met = report.get('echoP95Under16Ms') if KIND == 'echo' else report.get('interruptUnder100Ms')
            report['passed'] = bool(report['measurementsComplete'] and target_met and (not VERIFY or report.get('originalScreenEqual') is True and report.get('fixtureInputExact') is True and report.get('fixtureExitedBeforeWindowClose') is True) and report.get('nativeProcessesDrained') and report.get('normalAppExit') and not report.get('forcedAppCleanup'))
            RESULT.write_text(json.dumps(report, indent=2) + '\n')
    assert report['passed'], 'load.performance_gate_failed'
    print('workspace.real_cat_1gib_sigint_latency_and_native_exit_confirmed')
