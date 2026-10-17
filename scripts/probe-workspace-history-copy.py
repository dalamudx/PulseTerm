#!/usr/bin/env python3
"""Trusted mouse selection and system clipboard across 600 wrapped logical rows."""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

fixture = Path(sys.argv[1])
evidence = Path('target/probes')
log = evidence / 'workspace-history-copy-native.log'
app = owner = xid = native_pid = None
stage = 'startup'
checks = []


def active_app():
    if app is None:
        raise RuntimeError('workspace.history_copy.app_not_started')
    return app


def run(args, timeout=3, check=True):
    return subprocess.run(args, capture_output=True, timeout=timeout, check=check)


def key(*keys):
    run(['/usr/bin/xdotool', 'key', '--clearmodifiers', *keys])


def wait_for(predicate, label):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        if predicate():
            return
        if active_app().poll() is not None:
            raise RuntimeError('workspace.history_copy.app_exited.' + label)
        time.sleep(0.1)
    raise RuntimeError('workspace.history_copy.timeout.' + label)


def count(label):
    return log.read_text().splitlines().count('workspace.' + label)


def accessible(text):
    return run([str(evidence / 'x11-status'), str(active_app().pid), text], timeout=2, check=False).returncode == 0


def painted(marker, row):
    def matches():
        image = fixture / 'paint.png'
        crop = fixture / 'paint-row.png'
        run(['/usr/bin/import', '-window', xid, str(image)])
        run(['/usr/bin/magick', str(image), '-crop', '300x15+200+' + str(149 + row * 15), '+repage', '-resize', '400%', str(crop)])
        text = run(['/usr/bin/tesseract', str(crop), 'stdout', '-l', 'eng', '--psm', '7']).stdout.decode()
        return marker in ' '.join(text.split())
    wait_for(matches, 'visible_' + marker.replace(' ', '_'))


def scroll(direction):
    run(['/usr/bin/xdotool', 'key', '--clearmodifiers', '--repeat', '100', '--delay', '5', 'shift+' + direction])


def reset_clipboard():
    global owner
    if owner is not None and owner.poll() is None:
        owner.terminate()
        owner.wait(timeout=3)
    sentinel = fixture / 'sentinel.txt'
    sentinel.write_text('HISTORY_COPY_SENTINEL')
    with (fixture / 'clipboard.log').open('wb') as output:
        owner = subprocess.Popen([str(evidence / 'x11-clipboard'), str(sentinel)], stdout=output, stderr=output)
    wait_for(lambda: 'clipboard.ready' in (fixture / 'clipboard.log').read_text(), 'clipboard_owner')
    probe = fixture / 'sentinel-read.txt'
    run([str(evidence / 'x11-clipboard'), 'read', str(probe)])
    if probe.read_bytes() != sentinel.read_bytes():
        raise RuntimeError('workspace.history_copy.sentinel_not_current')


def copy_history(name):
    pid = native_pid
    if pid is None:
        raise RuntimeError('workspace.history_copy.producer_not_started')
    reset_clipboard()
    run(['/usr/bin/xdotool', 'mousemove', '--window', xid, '700', '500', 'click', '1'])
    scroll('Prior')
    painted('BEGIN HISTORY', 0)
    # A real selection start is retained while scrolling; Shift-click extends it.
    run(['/usr/bin/xdotool', 'mousemove', '--window', xid, '200', '156', 'mousedown', '1', 'mousemove', '--window', xid, '240', '156', 'mouseup', '1'])
    scroll('Next')
    painted('HISTORY READY', 30)
    run(['/usr/bin/xdotool', 'keydown', 'Shift', 'mousemove', '--window', xid, '288', '591', 'click', '1', 'keyup', 'Shift'])
    key('ctrl+c')
    copied = fixture / 'copied.txt'
    expected = (fixture / 'expected.txt').read_bytes()
    def exact():
        result = run([str(evidence / 'x11-clipboard'), 'read', str(copied)], timeout=3, check=False)
        return result.returncode == 0 and copied.read_bytes() == expected
    wait_for(exact, 'exact_system_clipboard_' + name)
    os.kill(pid, 0)
    if (fixture / 'input.txt').read_text() != 'EMIT\n':
        raise RuntimeError('workspace.history_copy.copy_or_scroll_wrote_to_pty')
    (evidence / ('workspace-history-copy-' + name + '.txt')).write_bytes(copied.read_bytes())
    run(['/usr/bin/import', '-window', xid, str(evidence / ('workspace-history-copy-' + name + '.png'))])
    return hashlib.sha256(expected).hexdigest(), len(expected)


def interrupted(signum, _frame):
    raise SystemExit(128 + signum)


signal.signal(signal.SIGTERM, interrupted)
try:
    with log.open('wb') as output:
        app = subprocess.Popen(['target/debug/pulseterm-workspace'], stdout=output, stderr=output)
    wait_for(lambda: count('sessions_listed') == 1, 'first_page')
    def find_window():
        global xid
        result = run(['/usr/bin/xdotool', 'search', '--onlyvisible', '--all', '--pid', str(active_app().pid), '--name', '^PulseTerm · 工作区预览$'], check=False)
        windows = result.stdout.split()
        if len(windows) != 1:
            return False
        xid = windows[0].decode()
        return run(['/usr/bin/xdotool', 'getwindowpid', xid]).stdout.strip() == str(active_app().pid).encode()
    wait_for(find_window, 'window_identity')
    run(['/usr/bin/xdotool', 'windowactivate', '--sync', xid])
    wait_for(lambda: accessible('POSIX sh'), 'profile_button')
    run(['/usr/bin/xdotool', 'mousemove', '--window', xid, '80', '164', 'click', '1'])
    wait_for(lambda: count('terminal_input_ready') == 1 and count('terminal_grid_ready.92.31') == 1, 'initial_input_and_geometry')
    wait_for(lambda: (fixture / 'native.pid').exists(), 'producer_started')
    native_pid = int((fixture / 'native.pid').read_text())
    if str(fixture / 'producer.py').encode() not in (Path('/proc') / str(native_pid) / 'cmdline').read_bytes().split(b'\0'):
        raise RuntimeError('workspace.history_copy.producer_identity_mismatch')
    run(['/usr/bin/xdotool', 'type', '--clearmodifiers', '--delay', '0', 'EMIT'])
    key('Return')
    wait_for(lambda: (fixture / 'emitted').exists(), 'producer_emitted')
    painted('HISTORY READY', 30)
    stage = 'live_deep_wrapped_unicode_copy'
    digest, byte_count = copy_history('live')
    checks.append(stage)

    stage = 'same_process_page_refresh_deep_copy'
    ready = count('terminal_input_ready')
    page = count('document_confirmed')
    key('ctrl+shift+r')
    wait_for(lambda: count('document_confirmed') == page + 1, 'new_page_confirmed')
    wait_for(lambda: count('terminal_input_ready') == ready + 1, 'refreshed_input')
    if int((fixture / 'native.pid').read_text()) != native_pid or count('session_opened') != 1:
        raise RuntimeError('workspace.history_copy.producer_restarted')
    new_digest, new_bytes = copy_history('refresh')
    if (new_digest, new_bytes) != (digest, byte_count):
        raise RuntimeError('workspace.history_copy.refreshed_text_changed')
    checks.append(stage)

    stage = 'native_exit'
    key('alt+F4')
    if active_app().wait(timeout=8) != 0:
        raise RuntimeError('workspace.history_copy.workspace_exit_failed')
    try:
        os.kill(native_pid, 0)
    except ProcessLookupError:
        pass
    else:
        raise RuntimeError('workspace.history_copy.producer_alive')
    if 'workspace.terminal_failure' in log.read_text():
        raise RuntimeError('workspace.history_copy.terminal_failure')
    checks.append(stage)
    (evidence / 'workspace-history-copy-result.json').write_text(json.dumps({'schema': 1, 'checks': checks, 'logical_rows': 600, 'copied_utf8_bytes': byte_count, 'sha256': digest, 'live_and_refresh_equal': True, 'native_exit': True, 'limits': ['Within existing retained history', 'Not 4 MiB boundary or cross-platform acceptance']}, indent=2) + '\n')
    print('workspace.history_copy_actual_600_wrapped_unicode_rows_live_refresh_clipboard_exact_no_pty_input_native_exit_confirmed')
except BaseException:
    (evidence / 'workspace-history-copy-failure.json').write_text(json.dumps({'stage': stage}, indent=2) + '\n')
    copied = fixture / 'copied.txt'
    expected = fixture / 'expected.txt'
    if copied.exists() and expected.exists():
        actual_bytes = copied.read_bytes()
        expected_bytes = expected.read_bytes()
        first = next((i for i, pair in enumerate(zip(actual_bytes, expected_bytes)) if pair[0] != pair[1]), min(len(actual_bytes), len(expected_bytes)))
        (evidence / 'workspace-history-copy-failure-copied.txt').write_bytes(actual_bytes)
        (evidence / 'workspace-history-copy-failure-expected.txt').write_bytes(expected_bytes)
        (evidence / 'workspace-history-copy-failure-diff.json').write_text(json.dumps({'stage': stage, 'expected_bytes': len(expected_bytes), 'actual_bytes': len(actual_bytes), 'first_difference': first, 'expected_byte': expected_bytes[first] if first < len(expected_bytes) else None, 'actual_byte': actual_bytes[first] if first < len(actual_bytes) else None, 'expected_newlines': expected_bytes.count(b'\n'), 'actual_newlines': actual_bytes.count(b'\n')}, indent=2) + '\n')
    if xid is not None and app is not None and app.poll() is None:
        run(['/usr/bin/import', '-window', xid, str(evidence / 'workspace-history-copy-failure.png')], check=False)
    raise
finally:
    if owner is not None and owner.poll() is None:
        owner.terminate()
        owner.wait(timeout=3)
    if app is not None and app.poll() is None:
        if xid is not None:
            key('alt+F4')
        try:
            app.wait(timeout=8)
        except subprocess.TimeoutExpired:
            app.terminate()
            try:
                app.wait(timeout=3)
            except subprocess.TimeoutExpired:
                app.kill()
                app.wait(timeout=3)
    if native_pid is not None:
        proc = Path('/proc') / str(native_pid) / 'cmdline'
        if proc.exists() and str(fixture / 'producer.py').encode() in proc.read_bytes().split(b'\0'):
            os.kill(native_pid, signal.SIGTERM)
            deadline = time.monotonic() + 3
            while proc.exists() and time.monotonic() < deadline:
                time.sleep(0.05)
        if proc.exists():
            raise RuntimeError('workspace.history_copy.cleanup_native_remaining')
