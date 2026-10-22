#!/usr/bin/env python3
"""Linux production WebView process-tree baseline; run inside the owned X11 supervisor."""
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / 'target' / 'probes' / 'workspace-performance'
run_label = os.environ.get('PULSE_PERFORMANCE_RUN')
if run_label is not None:
    assert re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9-]{0,79}', run_label), 'performance.invalid_run_label'
    EVIDENCE /= run_label
EVIDENCE.mkdir(parents=True, exist_ok=run_label is None)
output_name = os.environ.get('PULSE_PERFORMANCE_OUTPUT', 'result.json')
assert Path(output_name).name == output_name and output_name.endswith('.json'), 'performance.invalid_output_name'
REPORT = EVIDENCE / output_name
REPORT.unlink(missing_ok=True)
assert sys.platform == 'linux', 'performance.requires_linux_procfs'
PROFILE = os.environ.get('PULSE_PERFORMANCE_PROFILE', 'dev')
assert PROFILE in ('dev', 'release'), 'performance.invalid_build_profile'
BUILD = 'release' if PROFILE == 'release' else 'debug'
BINARY = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')) / BUILD / 'pulseterm-workspace'


def run(*args, check=True):
    return subprocess.run(list(map(str, args)), check=check, capture_output=True, text=True, timeout=5)


def process_identity(pid):
    try:
        fields = (Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()
        return fields[19] if fields[0] != 'Z' else None
    except (FileNotFoundError, ProcessLookupError):
        return None


def process_tree(root):
    pending, seen = [root], set()
    while pending:
        pid = pending.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for children in (Path('/proc') / str(pid) / 'task').glob('*/children'):
            try:
                pending.extend(map(int, children.read_text().split()))
            except FileNotFoundError:
                pass
    return {pid: identity for pid in seen if (identity := process_identity(pid)) is not None}


def memory(root):
    members = process_tree(root)
    values = []
    for pid, identity in members.items():
        try:
            rollup = (Path('/proc') / str(pid) / 'smaps_rollup').read_text()
            status = (Path('/proc') / str(pid) / 'status').read_text()
        except FileNotFoundError:
            raise RuntimeError('performance.process_changed_during_sample') from None
        assert process_identity(pid) == identity, 'performance.process_identity_changed'
        fields = {key: int(value) * 1024 for key, value in re.findall(r'^(Rss|Pss|Private_Clean|Private_Dirty):\s+(\d+) kB$', rollup, re.M)}
        assert 'Rss' in fields and 'Pss' in fields, 'performance.missing_memory_fields'
        values.append({'pid': pid, 'name': status.splitlines()[0].split(':', 1)[1].strip(),
                       'rssBytes': fields['Rss'], 'pssBytes': fields['Pss'],
                       'privateBytes': fields['Private_Clean'] + fields['Private_Dirty']})
    assert any(value['pid'] == root for value in values), 'performance.root_exited'
    assert len(values) >= 3, 'performance.webview_processes_missing'
    return {'processCount': len(values), 'rssBytes': sum(p['rssBytes'] for p in values),
            'pssBytes': sum(p['pssBytes'] for p in values), 'processes': values}


with tempfile.TemporaryDirectory(prefix='workspace-performance.', dir='/data/temp') as directory:
    fixture = Path(directory)
    env = os.environ.copy()
    for name, folder in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                         ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        path = fixture / folder
        path.mkdir(mode=0o700)
        env[name] = str(path)
    env.update(TMPDIR=directory, TMP=directory, TEMP=directory, PULSE_PERFORMANCE_FIXTURE=directory)
    bindir = fixture / 'bin'
    bindir.mkdir(mode=0o700)
    shell = bindir / 'sh'
    shell.write_text('''#!/bin/sh
printf '%s\\n' "$$" > "$PULSE_PERFORMANCE_FIXTURE/$$.pid"
printf 'PERFORMANCE_READY\\r\\n'
while :; do
  if IFS= read -r line; then :; else result=$?; [ "$result" -gt 128 ] && continue; break; fi
  printf '%s\\n' "$line" >> "$PULSE_PERFORMANCE_FIXTURE/$$.input"
  case "$line" in
    HISTORY) /usr/bin/awk 'BEGIN { for(i=0;i<100050;i++) printf "HISTORY_%06d %060d\\r\\n",i,i; }';;
  esac
  printf 'output-complete\\n' > "$PULSE_PERFORMANCE_FIXTURE/$$.written"
  printf 'echo:%s\\r\\n' "$line"
done
''')
    shell.chmod(0o700)
    env.update(PATH=str(bindir), SHELL=str(shell))
    helper = fixture / 'native-helper'
    flags = run('pkg-config', '--cflags', '--libs', 'x11', 'atspi-2', 'gobject-2.0', 'glib-2.0').stdout.split()
    run('cc', ROOT / 'scripts' / 'probe-x11-status.c', '-o', helper, *flags)
    log_path = EVIDENCE / 'native.log'
    owned = {}
    snapshots = {}
    app = None
    def wait(predicate, tag, seconds=15):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if app is not None and app.poll() is not None:
                raise RuntimeError('performance.app_exited.' + str(app.returncode))
            value = predicate()
            if value:
                return value
            time.sleep(.05)
        raise RuntimeError('performance.timeout.' + tag)
    def count(event):
        return log_path.read_text(errors='replace').splitlines().count('workspace.' + event)
    def accessible(name, mode=None):
        return run(helper, app.pid, name, *([mode] if mode else []), check=False)
    def sample(tag):
        time.sleep(.5)
        readings = [memory(app.pid) for _ in range(3)]
        readings.sort(key=lambda value: value['pssBytes'])
        result = readings[1]
        result['pssSampleBytes'] = [value['pssBytes'] for value in readings]
        owned.update(process_tree(app.pid))
        run('import', '-window', xid, EVIDENCE / (tag + '.png'))
        snapshots[tag] = result
        return result
    def input_line(text):
        run('xdotool', 'type', '--clearmodifiers', '--delay', '1', text)
        run('xdotool', 'key', '--clearmodifiers', 'Return')
    with log_path.open('w') as output:
        try:
            started = time.monotonic()
            app = subprocess.Popen([str(BINARY)], env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            wait(lambda: count('profiles_listed') == 1, 'profiles')
            xid = wait(lambda: run('xdotool', 'search', '--onlyvisible', '--all', '--pid', app.pid,
                                 '--name', '^PulseTerm · 工作区预览$', check=False).stdout.strip(), 'window')
            assert len(xid.splitlines()) == 1 and int(run('xdotool', 'getwindowpid', xid).stdout) == app.pid
            run('xdotool', 'windowactivate', '--sync', xid)
            wait(lambda: accessible('POSIX sh').returncode == 0, 'painted_profile')
            empty = sample('empty')
            assert accessible('POSIX sh', 'activate').returncode == 0
            wait(lambda: count('terminal_input_ready') == 1, 'first_terminal')
            first_input_ready_ms = (time.monotonic() - started) * 1000
            single = sample('single')
            assert accessible('Terminal input', 'focus').returncode == 0
            time.sleep(.1)
            history_started = time.monotonic()
            input_line('HISTORY')
            wait(lambda: any('HISTORY' in p.read_text().splitlines() for p in fixture.glob('*.input')), 'history_writer', 45)
            wait(lambda: bool(list(fixture.glob('*.written'))), 'history_output', 90)
            assert accessible('查看当前窗格历史与查找', 'activate').returncode == 0
            def retained_history():
                result = accessible('历史分页状态', 'readonly-text')
                if result.returncode == 0 and '共保留 100000 行' not in result.stdout:
                    accessible('最新历史', 'activate')
                return result.stdout if result.returncode == 0 and '共保留 100000 行' in result.stdout else ''
            status = wait(retained_history, 'retained_100000')
            history_retained_ms = (time.monotonic() - history_started) * 1000
            run('xdotool', 'key', '--clearmodifiers', 'Escape')
            history = sample('history-100000')
            for goal in range(2, 21):
                assert accessible('POSIX sh', 'activate').returncode == 0
                wait(lambda: count('session_opened') == goal and count('terminal_input_ready') >= goal,
                     'tab_' + str(goal))
            assert len(list(fixture.glob('*.pid'))) == 20, 'performance.native_session_count'
            twenty = sample('twenty-tabs')
            goals = {'singleTotalPssUnder120MB': single['pssBytes'] < 120_000_000,
                     'twentyTotalPssUnder250MB': twenty['pssBytes'] < 250_000_000,
                     'historyTotalPssDeltaAtMost20MB': history['pssBytes'] - single['pssBytes'] <= 20_000_000}
            run('xdotool', 'key', '--clearmodifiers', 'alt+F4')
            app.wait(timeout=15)
            assert app.returncode == 0 and count('exited') == 1
            deadline = time.monotonic() + 5
            while any(process_identity(pid) == identity for pid, identity in owned.items()) and time.monotonic() < deadline:
                time.sleep(.05)
            assert not any(process_identity(pid) == identity for pid, identity in owned.items()), 'performance.native_processes_not_drained'
            report = {'schemaVersion': 1, 'measurementsComplete': True, 'platform': 'linux',
                      'build': BUILD, 'display': 'X11 Xvfb with software rendering',
                      "runLabel": run_label,
                      'memoryUnit': 'bytes; MB targets use 1000000 bytes', 'sampleStatistic': 'median of three total PSS samples',
                      'firstInputReadyMsIncludingEmptySample': first_input_ready_ms,
                      'historyRetainedObservationMs': history_retained_ms,
                      'empty': empty, 'single': single, 'history100000': history, 'twentyTabs': twenty,
                      'twentyTabsFirstSessionRetainedRows': 100000,
                      'retainedHistoryStatus': status.strip(), 'historyTotalPssDeltaBytes': history['pssBytes'] - single['pssBytes'],
                      'targetsMet': goals, 'allMemoryTargetsMet': all(goals.values()), 'nativeProcessesDrained': True,
                      'acceptance': {'releaseBuild': BUILD == 'release', 'hardwareRendering': False, 'coldStartup': False,
                                     'ime': False, 'oneGBInterrupt': False, 'echoP95': False, 'crossPlatform': False}}
            REPORT.write_text(json.dumps(report, indent=2) + '\n')
            print(json.dumps({'measurementsComplete': True, 'targetsMet': goals,
                              'singlePssBytes': single['pssBytes'], 'twentyPssBytes': twenty['pssBytes'],
                              'historyDeltaBytes': history['pssBytes'] - single['pssBytes']}))
        except Exception as failure:
            if 'xid' in locals():
                run('import', '-window', xid, EVIDENCE / 'failure.png', check=False)
            REPORT.write_text(json.dumps({'schemaVersion': 1, 'measurementsComplete': False,
                'failure': str(failure), 'samples': snapshots,
                'inputLines': [p.read_text().splitlines() for p in fixture.glob('*.input')],
                'outputCompleteCount': len(list(fixture.glob('*.written')))}, indent=2) + '\n')
            raise
        finally:
            if app is not None and app.poll() is None:
                owned.update(process_tree(app.pid))
                app.terminate()
                try:
                    app.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait(timeout=5)
            for pid, identity in owned.items():
                if process_identity(pid) == identity:
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
