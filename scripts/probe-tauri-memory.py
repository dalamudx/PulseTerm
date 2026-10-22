#!/usr/bin/env python3
"""Same Tauri binary, blank versus workspace idle memory. No product performance acceptance."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time

spec = importlib.util.spec_from_file_location('presentation_floor', Path(__file__).with_name('probe-presentation-floor.py'))
floor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(floor)
MODES = ('blank', 'workspace', 'workspace', 'blank')


def verify_page(log, mode):
    lines = log.splitlines()
    expected = 'workspace.runtime_memory.page.' + mode
    if lines.count(expected) != 1 or any(line.startswith('workspace.runtime_memory.page.') and line != expected for line in lines):
        raise ValueError('Actual native page mode is missing, duplicate or mismatched')
    for event in ('session_opened', 'terminal_input_ready', 'shutdown_failed', 'terminal_failure'):
        if 'workspace.' + event in lines:
            raise ValueError('Unexpected terminal or failure in idle memory sample')
    if mode == 'blank':
        for event in ('document_confirmed', 'profiles_requested', 'profiles_listed', 'cli_presentation_ready'):
            if 'workspace.' + event in lines:
                raise ValueError('Blank page entered a product-authority path')
    elif mode != 'workspace' or lines.count('workspace.profiles_listed') != 1 or lines.count('workspace.document_confirmed') != 1:
        raise ValueError('Workspace page not confirmed and loaded')


def run_group(binary, out, index, mode):
    record = {'index': index, 'mode': mode, 'complete': False}
    log = out / f'{index}-{mode}.log'
    profile = Path(tempfile.mkdtemp(prefix='pulseterm-tauri-memory.', dir='/data/temp'))
    environment = dict(os.environ, PULSE_RUNTIME_MEMORY_PAGE=mode)
    for key, name in [('HOME', 'home'), ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        directory = profile / name
        directory.mkdir(mode=0o700)
        environment[key] = str(directory)
    environment.update(TMPDIR=str(profile), TMP=str(profile), TEMP=str(profile))
    app, owned = None, {}
    try:
        with log.open('x') as stream:
            app = subprocess.Popen([str(binary)], stdout=stream, stderr=subprocess.STDOUT, env=environment, start_new_session=True)
        deadline = time.monotonic() + 25
        while True:
            owned.update(floor.tree(app.pid))
            if app.poll() is not None:
                raise RuntimeError('Tauri host exited before actual page readiness')
            current = log.read_text(errors='replace')
            if 'workspace.runtime_memory.page.' + mode in current:
                if mode == 'blank' or 'workspace.profiles_listed' in current:
                    verify_page(current, mode)
                    break
            if time.monotonic() >= deadline:
                raise TimeoutError('Actual native page readiness missing')
            time.sleep(.05)
        windows = floor.command('xdotool', 'search', '--onlyvisible', '--all', '--pid', app.pid,
                                '--name', '^PulseTerm · 工作区预览$').stdout.splitlines()
        if len(windows) != 1:
            raise RuntimeError('Expected one owned visible Tauri window')
        xid = int(windows[0])
        if int(floor.command('xdotool', 'getwindowpid', xid).stdout) != app.pid:
            raise RuntimeError('Foreign window')
        floor.command('xdotool', 'windowactivate', '--sync', xid)
        geometry = floor.command('xdotool', 'getwindowgeometry', '--shell', xid).stdout
        record['windowGeometry'] = geometry
        fields = dict(line.split('=', 1) for line in geometry.splitlines())
        if fields.get('WIDTH') != '960' or fields.get('HEIGHT') != '640':
            raise RuntimeError('Tauri client geometry changed')
        time.sleep(.5)
        readings = []
        for _ in range(3):
            readings.append(floor.memory(app.pid, 'tauri'))
            owned.update(floor.tree(app.pid))
            time.sleep(.1)
        identities = [{(p['pid'], p['startIdentity']) for p in reading['processes']} for reading in readings]
        if any(members != identities[0] for members in identities):
            raise RuntimeError('Tauri process tree changed between samples')
        verify_page(log.read_text(errors='replace'), mode)
        record.update(memory=sorted(readings, key=lambda r: r['pssBytes'])[1], memoryReadings=readings,
                      actualPageVerified=True, zeroTerminalSessions=True)
        floor.command('import', '-window', xid, out / f'{index}-{mode}.png')
        floor.command('xdotool', 'key', '--clearmodifiers', 'alt+F4')
        app.wait(timeout=15)
        record['hostExitCode'] = app.returncode
        lines = log.read_text(errors='replace').splitlines()
        verify_page('\n'.join(lines), mode)
        if app.returncode != 0 or lines.count('workspace.shutdown_complete') != 1 or lines.count('workspace.exited') != 1:
            raise RuntimeError('Product physical shutdown not confirmed')
        record.update(productShutdownConfirmed=True, complete=True)
    except Exception as error:
        record['error'] = f'{type(error).__name__}: {error}'
        if isinstance(error, subprocess.CalledProcessError):
            record['commandStderr'] = error.stderr
    finally:
        if app:
            owned.update(floor.tree(app.pid))
            if app.poll() is None:
                record['forcedHostCleanup'] = True
                app.terminate()
                try:
                    app.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    os.killpg(app.pid, signal.SIGKILL)
                    app.wait(timeout=5)
            deadline = time.monotonic() + 5
            while any(floor.identity(pid) == stamp for pid, stamp in owned.items()) and time.monotonic() < deadline:
                time.sleep(.05)
            remaining = [(pid, stamp) for pid, stamp in owned.items() if floor.identity(pid) == stamp]
            record['nativeProcessesDrained'] = not remaining
            if remaining:
                record['forcedNativeCleanup'] = True
                for pid, stamp in remaining:
                    if floor.identity(pid) == stamp:
                        os.kill(pid, signal.SIGKILL)
                deadline = time.monotonic() + 3
                while any(floor.identity(pid) == stamp for pid, stamp in remaining) and time.monotonic() < deadline:
                    time.sleep(.05)
                record['remainingProcesses'] = [pid for pid, stamp in remaining if floor.identity(pid) == stamp]
        if not record.get('remainingProcesses'):
            shutil.rmtree(profile)
            record['temporaryProfileCleaned'] = not profile.exists()
        else:
            record['remainingProfile'] = str(profile)
        (out / f'{index}-{mode}-result.json').write_text(json.dumps(record, indent=2) + '\n')
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if not args.binary.is_absolute() or not args.binary.is_file():
        parser.error('Existing absolute Tauri validation binary required')
    args.out.mkdir(parents=True, exist_ok=False)
    digest = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    report = {'schemaVersion': 1, 'complete': False, 'sameBinarySha256': digest, 'order': MODES,
              'groups': [], 'productMemoryGateEvaluated': False,
              'environment': {key: os.environ.get(key) for key in ('GDK_BACKEND', 'LIBGL_ALWAYS_SOFTWARE', 'WEBKIT_DISABLE_DMABUF_RENDERER',
                                'WEBKIT_DISABLE_COMPOSITING_MODE', 'WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS', 'GALLIUM_DRIVER')},
              'limitations': ['Single ABBA idle-page sequence; not randomized or hardware/cross-platform acceptance',
                             'Blank page preserves native service initialization; workspace on-demand Profile/config hydration differs',
                             'No PTY, history, cold-start, echo, IME or physical display acceptance',
                             'PSS depends on other processes sharing mappings; private residency is also retained',
                             'No product memory target changed or evaluated by this diagnostic']}
    previous = signal.getsignal(signal.SIGTERM)
    def interrupted(_signum, _frame):
        raise InterruptedError('Tauri memory suite interrupted')
    signal.signal(signal.SIGTERM, interrupted)
    try:
        for index, mode in enumerate(MODES, 1):
            if hashlib.sha256(args.binary.read_bytes()).hexdigest() != digest:
                raise RuntimeError('Tauri binary changed during suite')
            row = run_group(args.binary, args.out, index, mode)
            report['groups'].append(row)
            if not row['complete'] or not row.get('nativeProcessesDrained') or not row.get('temporaryProfileCleaned'):
                raise RuntimeError('Incomplete Tauri memory group; remaining groups not launched')
        report['complete'] = True
    finally:
        signal.signal(signal.SIGTERM, previous)
        (args.out / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'complete': True, 'sameBinary': digest, 'groups': [
        {'mode': row['mode'], 'pssBytes': row['memory']['pssBytes'], 'privateBytes': row['memory']['privateBytes']} for row in report['groups']]}))


if __name__ == '__main__':
    main()
