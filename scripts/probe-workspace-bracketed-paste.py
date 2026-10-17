#!/usr/bin/env python3
"""Actual clipboard/key events and Bash's own READLINE_LINE; no synthetic paste."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

fixture = Path(sys.argv[1])
evidence = Path('target/probes')
native = fixture / 'native'
log_path = evidence / 'workspace-bracketed-paste-native.log'
app = None
owner = None
xid = None
native_pid = None
stage = 'startup'
expected = []
checks = []


def run(args, timeout=3, check=True):
    return subprocess.run(args, capture_output=True, timeout=timeout, check=check)


def key(*keys):
    run(['/usr/bin/xdotool', 'key', '--clearmodifiers', *keys])


def current_app():
    if app is None:
        raise RuntimeError('workspace.bracketed_paste.app_not_started')
    return app


def wait_for(predicate, label):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        if predicate():
            return
        if current_app().poll() is not None:
            raise RuntimeError('workspace.bracketed_paste.app_exited.' + label)
        time.sleep(0.1)
    raise RuntimeError('workspace.bracketed_paste.timeout.' + label)


def count(label):
    return log_path.read_text().splitlines().count('workspace.' + label)


def records():
    path = Path(str(native) + '.records')
    if not path.exists():
        return None
    data = path.read_bytes()
    if data and not data.endswith(b'\0'):
        return None
    return [part.decode('utf-8') for part in data.split(b'\0')[:-1]]


def expect_records():
    def ready():
        actual = records()
        return actual is not None and len(actual) >= len(expected)
    wait_for(ready, 'application_receipts')
    actual = records()
    if actual != expected:
        (evidence / 'workspace-bracketed-paste-receipts-failure.json').write_text(json.dumps({
            'stage': stage, 'expected_count': len(expected),
            'actual_count': len(actual) if actual is not None else None,
            'pending_close_markers_executed': sum(part.startswith('CLOSE_WITHOUT_ENTER_') for part in actual) if actual is not None else None,
        }, indent=2) + '\n')
        raise RuntimeError('workspace.bracketed_paste.application_receipts_mismatch.' + stage)


def capture(text, mode='on'):
    path = Path(str(native) + '.captured')
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        before = int(path.read_text()) if path.exists() else 0
        key('ctrl+x', 'ctrl+b')
        while time.monotonic() < deadline and (not path.exists() or int(path.read_text()) <= before):
            if current_app().poll() is not None:
                raise RuntimeError('workspace.bracketed_paste.app_exited.readline_capture')
            time.sleep(0.05)
        if not path.exists() or int(path.read_text()) != before + 1:
            raise RuntimeError('workspace.bracketed_paste.readline_capture_unconfirmed.' + stage)
        if (Path(str(native) + '.buffer').read_bytes() == text.encode('utf-8')
                and Path(str(native) + '.mode').read_text() == mode):
            expect_records()
            return
        # Only observe again; never repeat the user paste or execute its buffer.
        time.sleep(0.1)
    raise RuntimeError('workspace.bracketed_paste.readline_buffer_or_mode_unconfirmed.' + stage)


def accessible(text):
    return run([str(evidence / 'x11-status'), str(current_app().pid), text], timeout=2, check=False).returncode == 0


def paste(text, decision=None):
    global owner
    if owner is not None:
        owner.terminate()
        owner.wait(timeout=3)
    path = fixture / 'paste.txt'
    path.write_bytes(text.encode('utf-8'))
    clip_log = fixture / 'clipboard.log'
    with clip_log.open('wb') as output:
        owner = subprocess.Popen([str(evidence / 'x11-clipboard'), str(path)], stdout=output, stderr=output)
    wait_for(lambda: 'clipboard.ready' in clip_log.read_text(), 'clipboard_owner')
    key('ctrl+shift+v')
    wait_for(lambda: 'clipboard.served' in clip_log.read_text(), 'clipboard_served')
    if decision is not None:
        wait_for(lambda: accessible('取消'), 'paste_confirmation')
        key('Tab', 'Return') if decision else key('Return')


def enter(values):
    expected.extend(values)
    key('Return')
    expect_records()


def switch(mode):
    run(['/usr/bin/xdotool', 'type', '--clearmodifiers', '--delay', '0', 'fixture_switch ' + mode])
    enter(['MODE_' + mode])
    capture('', mode)
    bindings = Path(str(native) + '.bindings').read_text()
    if 'set enable-bracketed-paste ' + mode + '\n' not in bindings:
        raise RuntimeError('workspace.bracketed_paste.real_readline_binding_mismatch')
    marker = 'BRACKET MODE ' + mode + ' READY'
    def painted():
        image = fixture / 'mode.png'
        crop = fixture / 'mode-terminal.png'
        run(['/usr/bin/import', '-window', xid, str(image)])
        run(['/usr/bin/magick', str(image), '-crop', '300x150+200+464', '+repage', '-resize', '300%', str(crop)])
        text = run(['/usr/bin/tesseract', str(crop), 'stdout', '-l', 'eng', '--psm', '6']).stdout.decode()
        return marker in ' '.join(text.split())
    wait_for(painted, 'mode_rendered_' + mode)


def interrupted(signum, _frame):
    raise SystemExit(128 + signum)


signal.signal(signal.SIGTERM, interrupted)
try:
    with log_path.open('wb') as output:
        app = subprocess.Popen(['target/debug/pulseterm-workspace'], stdout=output, stderr=output)
    wait_for(lambda: count('sessions_listed') == 1, 'first_page')
    def find_window():
        global xid
        result = run(['/usr/bin/xdotool', 'search', '--onlyvisible', '--all', '--pid', str(current_app().pid), '--name', '^PulseTerm · 工作区预览$'], check=False)
        windows = result.stdout.split()
        if len(windows) != 1:
            return False
        xid = windows[0].decode()
        return run(['/usr/bin/xdotool', 'getwindowpid', xid]).stdout.strip() == str(current_app().pid).encode()
    wait_for(find_window, 'window_identity')
    run(['/usr/bin/xdotool', 'windowactivate', '--sync', xid])
    wait_for(lambda: accessible('POSIX sh'), 'profile_button')
    run(['/usr/bin/xdotool', 'mousemove', '--window', xid, '80', '164', 'click', '1'])
    wait_for(lambda: count('terminal_input_ready') == 1, 'input_ready')
    wait_for(lambda: Path(str(native) + '.pid').exists(), 'application_startup')
    native_pid = int(Path(str(native) + '.pid').read_text())
    capture('')

    stage = 'cancel_on'
    paste('fixture_record CANCEL_ONE\nfixture_record CANCEL_TWO', False)
    capture('')
    checks.append(stage)

    stage = 'approve_on'
    text = 'fixture_record ON_ONE\nfixture_record ON_TWO'
    paste(text, True)
    capture(text)
    enter(['ON_ONE', 'ON_TWO'])
    checks.append(stage)

    stage = 'pending_refresh'
    text = 'fixture_record REFRESH_ONE\nfixture_record REFRESH_TWO'
    paste(text, True)
    capture(text)
    key('ctrl+shift+r')
    wait_for(lambda: count('document_confirmed') == 2, 'page_refresh')
    wait_for(lambda: count('terminal_input_ready') == 2, 'refresh_input')
    if int(Path(str(native) + '.pid').read_text()) != native_pid or count('session_opened') != 1:
        raise RuntimeError('workspace.bracketed_paste.refresh_restarted_application')
    capture(text)
    enter(['REFRESH_ONE', 'REFRESH_TWO'])
    checks.append(stage)

    stage = 'restored_unicode_mode'
    text = 'fixture_record "你好e\u0301"\nfixture_record "第二行"'
    paste(text, True)
    capture(text)
    enter(['你好e\u0301', '第二行'])
    checks.append(stage)

    stage = 'exact_byte_limit'
    text = 'fixture_record ' + 'x' * (1012 - len('fixture_record '))
    assert len(text.encode('utf-8')) == 1012
    paste(text)
    capture(text)
    enter([text[len('fixture_record '):]])
    checks.append(stage)

    stage = 'over_byte_limit'
    paste('fixture_record ' + 'y' * (1013 - len('fixture_record ')))
    wait_for(lambda: accessible('未粘贴：单次内容最多 1012 UTF-8 字节'), 'byte_rejection')
    capture('')
    checks.append(stage)

    stage = 'mode_off'
    switch('off')
    expected.extend(['OFF_ONE', 'OFF_TWO'])
    paste('fixture_record OFF_ONE\nfixture_record OFF_TWO\n', True)
    expect_records()
    capture('', 'off')
    checks.append(stage)

    stage = 'mode_reenabled'
    switch('on')
    text = 'fixture_record AGAIN_ONE\nfixture_record AGAIN_TWO'
    paste(text, True)
    capture(text)
    enter(['AGAIN_ONE', 'AGAIN_TWO'])
    capture('')
    checks.append(stage)
    if 'workspace.terminal_failure.' in log_path.read_text():
        raise RuntimeError('workspace.bracketed_paste.terminal_failure')
    def final_painted():
        image = fixture / 'final.png'
        crop = fixture / 'final-terminal.png'
        run(['/usr/bin/import', '-window', xid, str(image)])
        run(['/usr/bin/magick', str(image), '-crop', '300x150+200+464', '+repage', '-resize', '300%', str(crop)])
        text = run(['/usr/bin/tesseract', str(crop), 'stdout', '-l', 'eng', '--psm', '6']).stdout.decode()
        if 'APPLICATION RECEIPT 13' not in ' '.join(text.split()):
            return False
        (evidence / 'workspace-bracketed-paste-final.png').write_bytes(image.read_bytes())
        return True
    wait_for(final_painted, 'final_receipt_painted')
    wait_for(lambda: accessible('终端已连接 · 多行粘贴需要确认'), 'old_paste_notice_cleared')
    stage = 'close_pending_paste'
    pending = 'fixture_record CLOSE_WITHOUT_ENTER_ONE\nfixture_record CLOSE_WITHOUT_ENTER_TWO'
    paste(pending, True)
    capture(pending)
    expect_records()
    stage = 'exit'
    key('alt+F4')
    app.wait(timeout=8)
    if app.returncode != 0 or count('exited') != 1:
        raise RuntimeError('workspace.bracketed_paste.exit_unconfirmed')
    try:
        os.kill(native_pid, 0)
    except ProcessLookupError:
        pass
    else:
        raise RuntimeError('workspace.bracketed_paste.native_not_reaped')
    expect_records()
    checks.append('close_pending_approved_paste_zero_execution')
    (evidence / 'workspace-bracketed-paste-result.json').write_text(json.dumps({
        'platform': 'Linux X11', 'application': 'actual Bash / GNU Readline',
        'checks': checks, 'exact_application_receipts': len(expected),
        'same_native_pid_refresh': True, 'no_auto_execution_while_bracketed': True,
        'byte_limit': 1012, 'unicode_and_combining_bytes_exact': True,
        'application_reaped': True, 'application_exit': app.returncode,
        'close_pending_approved_paste_zero_execution': True,
        'limitations': ['Not system IME validation', 'Not cross-platform validation', 'Other interactive applications remain pending']
    }, indent=2) + '\n')
    print('workspace.bracketed_paste_real_bash_readline_cancel_approve_pending_refresh_unicode_byte_limits_mode_cycle_exit_confirmed')
except BaseException:
    if xid is not None and app is not None and app.poll() is None:
        run(['/usr/bin/import', '-window', xid, str(evidence / 'workspace-bracketed-paste-failure.png')], check=False)
    raise
finally:
    if owner is not None and owner.poll() is None:
        owner.terminate()
        owner.wait(timeout=3)
    if app is not None and app.poll() is None:
        if xid is not None:
            run(['/usr/bin/xdotool', 'key', '--clearmodifiers', 'alt+F4'], check=False)
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
        try:
            os.kill(native_pid, 0)
        except ProcessLookupError:
            pass
        else:
            raise RuntimeError('workspace.bracketed_paste.cleanup_native_remaining')
