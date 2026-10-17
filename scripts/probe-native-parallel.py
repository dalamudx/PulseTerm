#!/usr/bin/env python3
"""Linux diagnostic for existing local PTY tests; no deadline/assertion changes."""
from contextlib import contextmanager
import ctypes
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/probes/native-parallel'
EXPECTED = {
    'closed_actor_without_delivered_native_receipt_does_not_retire_capacity',
    'document_and_window_retirement_do_not_drop_root_owner',
    'normal_handoff_survives_window_close_and_retains_true_native_exit',
    'coincident_session_ids_only_reap_their_committing_registry',
    'dropped_cleanup_waiter_does_not_abort_native_join_or_reap',
    'cancelled_finish_keeps_receiver_for_detached_drop_cleanup',
    'missing_completion_fails_closed_without_claiming_reap',
    'native_error_is_not_a_join_proof_for_finish_or_cancel',
    'natural_exit_preserves_receipt_and_retires_registered_slot',
}


def identity(pid):
    try:
        raw = Path(f'/proc/{pid}/stat').read_text()
        tail = raw.rsplit(')', 1)[1].split()
        return {'pid': pid, 'ppid': int(tail[1]), 'start': tail[19]}
    except (OSError, ValueError, IndexError):
        return None


def same_process(row):
    current = identity(row['pid'])
    return current is not None and current['start'] == row['start']


def descendants(roots):
    rows = [r for p in Path('/proc').iterdir() if p.name.isdigit()
            if (r := identity(int(p.name)))]
    ids = set(roots)
    while True:
        added = {r['pid'] for r in rows if r['ppid'] in ids} - ids
        if not added:
            break
        ids.update(added)
    return [r for r in rows if r['pid'] in ids]


def snapshot(rows):
    result = []
    for row in rows:
        threads = []
        try:
            for task in Path(f"/proc/{row['pid']}/task").iterdir():
                try:
                    threads.append({'tid': int(task.name),
                                    'name': (task / 'comm').read_text().strip(),
                                    'wait': (task / 'wchan').read_text().strip()})
                except OSError:
                    pass
        except OSError:
            pass
        result.append({**row, 'threads': threads})
    return result


def stop_owned(known, children):
    # Verify start time before signalling; never address a recycled PID.
    for sig in (signal.SIGTERM, signal.SIGKILL):
        known.update({r['pid']: r for r in descendants([os.getpid()]) if r['pid'] != os.getpid()})
        for pid, row in list(known.items()):
            if same_process(row):
                try:
                    os.kill(pid, sig)
                except ProcessLookupError:
                    pass
        until = time.monotonic() + 1
        while time.monotonic() < until:
            for child in children:
                child.poll()
            for pid, row in known.items():
                if pid not in {child.pid for child in children} and same_process(row):
                    try:
                        os.waitpid(pid, os.WNOHANG)
                    except ChildProcessError:
                        pass
            if not any(same_process(row) for pid, row in known.items()):
                return
            time.sleep(0.02)
    remaining = [pid for pid, row in known.items() if same_process(row)]
    if remaining:
        raise RuntimeError(f'owned processes remain: {remaining}')


def run_case(binary, env, mode, cycle, threads, copies, selected_count):
    children, files, known = [], [], {}
    started = time.monotonic()
    prefix = f'{mode}-{cycle:02d}'
    last = []
    try:
        for copy in range(copies):
            output = OUT / f'{prefix}-{copy}.log'
            log = output.open('w')
            files.append((log, output))
            argv = [str(binary), 'local_']
            if threads is not None:
                argv += [f'--test-threads={threads}']
            child = subprocess.Popen(argv, cwd=ROOT, env=env, stdout=log,
                                     stderr=subprocess.STDOUT)
            children.append(child)
            row = identity(child.pid)
            if row:
                known[child.pid] = row
        deadline = started + 20
        while any(child.poll() is None for child in children):
            rows = descendants([child.pid for child in children])
            known.update({r['pid']: r for r in rows})
            if time.monotonic() - started >= 3:
                last = snapshot(rows)
            if any(child.returncode not in (None, 0) for child in children):
                raise RuntimeError(f'{prefix}: test failure; no retry')
            if time.monotonic() >= deadline:
                raise RuntimeError(f'{prefix}: supervisor timeout; not a test pass')
            time.sleep(0.05)
        for log, output in files:
            log.flush()
            text = output.read_text()
            passed = re.search(r'test result: ok\. (\d+) passed; 0 failed;', text)
            if not passed or int(passed[1]) != selected_count:
                raise RuntimeError(f'{prefix}: missing or mismatched test receipt')
        if any(child.returncode != 0 for child in children):
            raise RuntimeError(f'{prefix}: failed exit')
        # A green test process with a surviving native child is also a failure.
        if any(same_process(row) for pid, row in known.items()):
            raise RuntimeError(f'{prefix}: native descendant outlived tests')
        return {'mode': mode, 'cycle': cycle, 'pids': [p.pid for p in children],
                'copies': copies, 'tests_per_process': selected_count,
                'seconds': round(time.monotonic() - started, 3),
                'native_children_remaining': 0, 'exit_codes': [p.returncode for p in children]}
    except BaseException:
        last = snapshot([row for row in known.values() if same_process(row)])
        (OUT / f'{prefix}-failure-state.json').write_text(json.dumps(last, indent=2))
        raise
    finally:
        try:
            stop_owned(known, children)
        finally:
            for log, _ in files:
                log.close()


def build_tests(env):
    known = {}
    with (OUT / 'build.log').open('w') as errors, (OUT / 'cargo-artifacts.jsonl').open('w') as output:
        child = subprocess.Popen(['cargo', 'test', '-p', 'pulse-app', '--lib', '--no-run',
                                  '--locked', '--message-format=json'],
                                 cwd=ROOT, env=env, stdout=output, stderr=errors)
        deadline = time.monotonic() + 300
        try:
            while child.poll() is None:
                known.update({r['pid']: r for r in descendants([child.pid])})
                if time.monotonic() >= deadline:
                    raise RuntimeError('build timeout; no test started')
                time.sleep(0.05)
            if child.returncode:
                raise subprocess.CalledProcessError(child.returncode, 'cargo test --no-run')
        finally:
            stop_owned(known, [child])
    return (OUT / 'cargo-artifacts.jsonl').read_text()


@contextmanager
def task_temp():
    tmp = tempfile.mkdtemp(prefix='native-parallel-run.', dir='/data/temp')
    try:
        yield tmp
    finally:
        remaining = [r['pid'] for r in descendants([os.getpid()]) if r['pid'] != os.getpid()]
        if remaining:
            raise RuntimeError(f'cleanup blocked by owned processes {remaining}; retained {tmp}')
        shutil.rmtree(tmp)


def main():
    if sys.platform != 'linux':
        raise RuntimeError('unsupported platform; Linux /proc is required')
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    cycles = int(os.environ.get('PULSE_NATIVE_PARALLEL_CYCLES', '8'))
    if not 1 <= cycles <= 32:
        raise ValueError('cycles must be 1..32')
    # Reap only this task's orphaned descendants if a failed test runtime exits.
    if ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), 'subreaper admission failed')
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'cycles_per_mode': cycles, 'original_deadline_seconds': 5,
              'production_modified': False, 'test_threads_environment_override': None,
              'host_cpu_count': os.cpu_count(), 'cpu_affinity_count': len(os.sched_getaffinity(0)),
              'runs': [], 'status': 'running'}
    receipt = OUT / 'result.json'
    with task_temp() as tmp:
        env = {**os.environ, 'TMPDIR': tmp, 'TMP': tmp, 'TEMP': tmp}
        env.pop('RUST_TEST_THREADS', None)
        try:
            rows = [json.loads(line) for line in build_tests(env).splitlines() if line.startswith('{')]
            binaries = [row['executable'] for row in rows if row.get('executable')
                        and row.get('target', {}).get('name') == 'pulse_app'
                        and row.get('profile', {}).get('test')]
            if len(binaries) != 1:
                raise RuntimeError('ambiguous or missing Cargo test executable')
            binary = Path(binaries[0])
            listing = subprocess.check_output([str(binary), 'local_', '--list'],
                                               env=env, cwd=ROOT, text=True, timeout=10)
            (OUT / 'selected-tests.txt').write_text(listing)
            selected = [line.split(': test')[0] for line in listing.splitlines() if line.endswith(': test')]
            if not EXPECTED <= {name.rsplit('::', 1)[-1] for name in selected}:
                raise RuntimeError('historically failing cases missing from selection')
            result['selected_test_count'] = len(selected)
            result['test_executable'] = str(binary.relative_to(ROOT))
            for mode, threads, copies in [('default', None, 1), ('threads64', 64, 1),
                                         ('overlap-default', None, 2)]:
                for cycle in range(cycles):
                    row = run_case(binary, env, mode, cycle, threads, copies, len(selected))
                    result['runs'].append(row)
                    print(json.dumps(row), flush=True)
            result['status'] = 'passed'
            result['historical_failure_resolved'] = False
        except BaseException:
            result['status'] = 'failed'
            raise
        finally:
            receipt.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
