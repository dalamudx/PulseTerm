#!/usr/bin/env python3
"""Linux new-process startup reference; macOS/Windows product gates stay unevaluated."""
import json
import os
from pathlib import Path
import signal
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/probes/workspace-startup'
BINARY = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')) / 'release/pulseterm-workspace'
HELPER = Path(os.environ['PULSE_NATIVE_HELPER'])


def run(*args, check=True):
    return subprocess.run(list(map(str, args)), check=check, capture_output=True, text=True, timeout=3)


def identity(pid):
    try:
        fields = (Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()
        return fields[19] if fields[0] != 'Z' else None
    except (FileNotFoundError, ProcessLookupError):
        return None


def tree(root):
    pending, owned = [root], {}
    while pending:
        pid = pending.pop()
        if pid in owned or (token := identity(pid)) is None:
            continue
        owned[pid] = token
        for path in (Path('/proc') / str(pid) / 'task').glob('*/children'):
            try:
                pending.extend(map(int, path.read_text().split()))
            except FileNotFoundError:
                pass
    return owned


def wait(predicate, tag, process=None, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process is not None and process.poll() is not None:
            raise RuntimeError(f'startup.app_exited.{process.returncode}.{tag}')
        value = predicate()
        if value:
            return value
        time.sleep(.01)
    raise RuntimeError('startup.timeout.' + tag)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    report = {'version': 1, 'measurementsComplete': False, 'nativeExitConfirmed': False,
              'platform': 'linux', 'build': 'release', 'samples': [],
              'productStartupGateEvaluated': False,
              'productTargets': {'appleSiliconMacMs': 400, 'windowsMs': 700},
              'limitations': ['Xvfb software rendering; no macOS or Windows acceptance',
                              'Observer, accessibility actions and explicit Profile selection are included',
                              'No OS page-cache eviction; first-use and existing-config launches are separate',
                              'Desktop session and accessibility registry are reused; no pixel latency measurement']}
    path = OUT / 'result.json'
    path.write_text(json.dumps(report, indent=2) + '\n')
    with tempfile.TemporaryDirectory(prefix='pt-start.', dir='/data/temp') as directory:
        fixture = Path(directory)
        env = os.environ.copy()
        for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                          ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
            folder = fixture / name
            folder.mkdir(mode=0o700)
            env[key] = str(folder)
        env.update(TMPDIR=directory, TMP=directory, TEMP=directory, PULSE_STARTUP_FIXTURE=directory)
        bindir = fixture / 'bin'
        bindir.mkdir(mode=0o700)
        shell = bindir / 'sh'
        shell.write_text('''#!/bin/sh
printf '%s\\n' "$$" > "$PULSE_STARTUP_FIXTURE/$$.pid"
printf 'STARTUP_READY\\r\\n'
while :; do
  if IFS= read -r line; then :; else result=$?; [ "$result" -gt 128 ] && continue; break; fi
  printf '%s\\n' "$line" >> "$PULSE_STARTUP_FIXTURE/$$.input"
  printf 'echo:%s\\r\\n' "$line"
done
''')
        shell.chmod(0o700)
        env.update(PATH=str(bindir), SHELL=str(shell))
        for attempt in range(6):
            app = None
            owned = {}
            xid = None
            log = OUT / f'launch-{attempt}.log'
            try:
                with log.open('w') as output:
                    started = time.monotonic_ns()
                    app = subprocess.Popen([str(BINARY)], env=env, stdout=output,
                                           stderr=subprocess.STDOUT, start_new_session=True)
                    gui_pid = app.pid
                    def count(event):
                        return log.read_text(errors='replace').splitlines().count('workspace.' + event)
                    def accessible(name, mode=None):
                        return run(HELPER, gui_pid, name, *([mode] if mode else []), check=False)
                    wait(lambda: count('profiles_listed') == 1, 'profiles', app)
                    xid = wait(lambda: run('xdotool', 'search', '--onlyvisible', '--all', '--pid', gui_pid,
                                            '--name', '^PulseTerm · 工作区预览$', check=False).stdout.strip(),
                               'window', app)
                    assert len(xid.splitlines()) == 1
                    assert int(run('xdotool', 'getwindowpid', xid).stdout) == app.pid
                    run('xdotool', 'windowactivate', '--sync', xid)
                    wait(lambda: accessible('POSIX sh').returncode == 0, 'profile_accessible', app)
                    profile_ready = time.monotonic_ns()
                    selected = time.monotonic_ns()
                    assert accessible('POSIX sh', 'activate').returncode == 0
                    wait(lambda: count('terminal_input_ready') == 1, 'input_ready', app)
                    input_ready = time.monotonic_ns()
                    assert accessible('Terminal input', 'focus').returncode == 0
                    text = f'STARTUP_{attempt:02d}'
                    run('xdotool', 'type', '--clearmodifiers', '--delay', '1', text)
                    run('xdotool', 'key', '--clearmodifiers', 'Return')
                    def received():
                        paths = list(fixture.glob('*.input'))
                        return any(item.read_text() == text + '\n' for item in paths)
                    wait(received, 'real_pty_input', app)
                    first_input = time.monotonic_ns()
                    owned.update(tree(app.pid))
                    assert len(owned) >= 4, 'startup.webview_and_native_processes_missing'
                    native = list(fixture.glob('*.pid'))
                    assert len(native) == attempt + 1
                    native_pids = [int(item.read_text().strip()) for item in native]
                    current_native = [pid for pid in native_pids if identity(pid) is not None]
                    assert len(current_native) == 1 and current_native[0] in owned
                    run('import', '-window', xid, OUT / f'launch-{attempt}.png')
                    run('xdotool', 'key', '--clearmodifiers', 'alt+F4')
                    app.wait(timeout=12)
                    assert app.returncode == 0 and count('exited') == 1
                    wait(lambda: all(identity(pid) != token for pid, token in owned.items()),
                         'native_exit', seconds=5)
                    actual_input = (fixture / f'{current_native[0]}.input').read_bytes()
                    assert actual_input == (text + '\n').encode(), 'startup.unexpected_final_input'
                    (OUT / f'launch-{attempt}.input').write_bytes(actual_input)
                    report['samples'].append({
                        'kind': 'first-use-new-config' if attempt == 0 else 'new-process-existing-config',
                        'guiPid': app.pid, 'nativePid': current_native[0], 'exitCode': app.returncode,
                        'processToAccessibleProfileMs': (profile_ready - started) / 1e6,
                        'profileSelectionToInputReadyMs': (input_ready - selected) / 1e6,
                        'processToInputReadyIncludingSelectionMs': (input_ready - started) / 1e6,
                        'processToFirstRealInputObservedMs': (first_input - started) / 1e6,
                        'nativeExitConfirmed': True, 'input': actual_input.decode('utf-8')})
                    path.write_text(json.dumps(report, indent=2) + '\n')
            except Exception as error:
                report['failure'] = str(error)
                path.write_text(json.dumps(report, indent=2) + '\n')
                if xid is not None:
                    run('import', '-window', xid, OUT / 'failure.png', check=False)
                raise
            finally:
                if app is not None and app.poll() is None:
                    owned.update(tree(app.pid))
                    os.killpg(app.pid, signal.SIGTERM)
                    try:
                        app.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(app.pid, signal.SIGKILL)
                        app.wait(timeout=5)
                for pid, token in owned.items():
                    if identity(pid) == token:
                        try:
                            os.kill(pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
        samples = report['samples']
        assert len({sample['guiPid'] for sample in samples}) == 6
        assert len({sample['nativePid'] for sample in samples}) == 6
        assert len(list(fixture.glob('*.input'))) == 6
        report.update(measurementsComplete=True, nativeExitConfirmed=True,
                      existingConfigMedianInputReadyMs=statistics.median(
                          sample['processToInputReadyIncludingSelectionMs'] for sample in samples[1:]))
        path.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'measurementsComplete': True, 'nativeExitConfirmed': True,
                          'existingConfigMedianInputReadyMs': report['existingConfigMedianInputReadyMs'],
                          'productStartupGateEvaluated': False}))


if __name__ == '__main__':
    main()
