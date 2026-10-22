#!/usr/bin/env python3
"""Real keyboard/GTK paste into Vim; F6 only observes the private application."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

fixture = Path(sys.argv[1])
evidence = Path('target/probes')
log = evidence / 'workspace-vim-native.log'
app = owner = xid = native_pid = None
stage = 'startup'
checks = []


def active_app():
    if app is None:
        raise RuntimeError('workspace.vim.application_not_started')
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
            raise RuntimeError('workspace.vim.app_exited.' + label)
        time.sleep(0.1)
    raise RuntimeError('workspace.vim.timeout.' + label)


def count(label):
    return log.read_text().splitlines().count('workspace.' + label)


def accessible(text):
    return run([str(evidence / 'x11-status'), str(active_app().pid), text], timeout=2, check=False).returncode == 0


def state():
    try:
        return json.loads((fixture / 'state.json').read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def capture(lines, mode):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        before = (state() or {}).get('count', 0)
        key('F6')
        while time.monotonic() < deadline:
            value = state()
            if value and value['count'] > before:
                if value['lines'] == lines and value['mode'] == mode:
                    if not value['bracketed_enabled']:
                        raise RuntimeError('workspace.vim.bracketed_mode_missing')
                    return value
                break
            if active_app().poll() is not None:
                raise RuntimeError('workspace.vim.app_exited.capture')
            time.sleep(0.05)
        # Observe only; never resubmit a paste while awaiting delivery.
        time.sleep(0.1)
    raise RuntimeError('workspace.vim.buffer_or_mode_mismatch.' + stage)


def paste(text, approve):
    global owner
    if owner is not None and owner.poll() is None:
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
    wait_for(lambda: accessible('取消'), 'paste_confirmation')
    key('Tab', 'Return') if approve else key('Return')


def painted(receipt, name):
    marker = 'VIM FIXTURE ' + str(receipt['count'])
    image = evidence / name
    def matches():
        run(['/usr/bin/import', '-window', xid, str(image)])
        crop = fixture / 'paint.png'
        run(['/usr/bin/convert', str(image), '-crop', '300x15+200+584', '+repage', '-resize', '400%', str(crop)])
        text = run(['/usr/bin/tesseract', str(crop), 'stdout', '-l', 'eng', '--psm', '7']).stdout.decode()
        return marker in ' '.join(text.split())
    wait_for(matches, 'visible_application_receipt')


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
    wait_for(lambda: count('terminal_input_ready') == 1, 'input_ready')
    wait_for(lambda: (fixture / 'native.pid').exists() and state() is not None, 'vim_started')
    native_pid = int((fixture / 'native.pid').read_text())
    if (Path('/proc') / str(native_pid) / 'exe').resolve() != Path('/usr/bin/vim').resolve():
        raise RuntimeError('workspace.vim.application_identity_mismatch')
    if str(fixture / 'vimrc').encode() not in (Path('/proc') / str(native_pid) / 'cmdline').read_bytes().split(b'\0'):
        raise RuntimeError('workspace.vim.private_configuration_missing')
    capture([''], 'n')
    key('i')
    capture([''], 'i')

    stage = 'cancel_multiline'
    paste('UNAPPROVED ONE\nUNAPPROVED TWO', False)
    capture([''], 'i')
    checks.append(stage)

    stage = 'approve_unicode_no_autoindent'
    text = 'if (ready) {\n  漢字🙂e\u0301\n}'
    lines = text.split('\n')
    paste(text, True)
    capture(lines, 'i')
    checks.append(stage)

    stage = 'same_vim_insert_buffer_refresh'
    ready = count('terminal_input_ready')
    page = count('document_confirmed')
    key('ctrl+shift+r')
    wait_for(lambda: count('document_confirmed') == page + 1, 'new_page_confirmed')
    wait_for(lambda: count('terminal_input_ready') == ready + 1, 'refreshed_input')
    if int((fixture / 'native.pid').read_text()) != native_pid or count('session_opened') != 1:
        raise RuntimeError('workspace.vim.restarted')
    os.kill(native_pid, 0)
    receipt = capture(lines, 'i')
    painted(receipt, 'workspace-vim-refresh.png')
    checks.append(stage)

    stage = 'single_undo_block'
    key('Escape', 'u')
    capture([''], 'n')
    checks.append(stage)
    key('i')
    paste(text, True)
    capture(lines, 'i')

    stage = 'save_exact_application_bytes'
    key('Escape')
    run(['/usr/bin/xdotool', 'type', '--clearmodifiers', '--delay', '0', ':write'])
    key('Return')
    saved = (text + '\n').encode('utf-8')
    wait_for(lambda: (fixture / 'document.txt').read_bytes() == saved, 'saved_document')
    receipt = capture(lines, 'n')
    if receipt['modified']:
        raise RuntimeError('workspace.vim.save_unconfirmed')
    painted(receipt, 'workspace-vim-saved.png')
    checks.append(stage)

    stage = 'close_unsaved_buffer_native_exit'
    key('G', 'A')
    paste('\nUNSAVED TAIL', True)
    receipt = capture(lines + ['UNSAVED TAIL'], 'i')
    if not receipt['modified']:
        raise RuntimeError('workspace.vim.pending_edit_missing')
    key('alt+F4')
    if app.wait(timeout=8) != 0:
        raise RuntimeError('workspace.vim.workspace_exit_failed')
    try:
        os.kill(native_pid, 0)
    except ProcessLookupError:
        pass
    else:
        raise RuntimeError('workspace.vim.native_still_alive')
    if (fixture / 'document.txt').read_bytes() != saved:
        raise RuntimeError('workspace.vim.unsaved_file_changed_on_close')
    if 'workspace.terminal_failure' in log.read_text():
        raise RuntimeError('workspace.vim.terminal_failure')
    checks.append(stage)
    (evidence / 'workspace-vim-result.json').write_text(json.dumps({'schema': 1, 'checks': checks, 'native_exit': True, 'saved_utf8_bytes': len(saved), 'clipboard_api': 'actual GTK/X11 keyboard paste', 'scope': 'Linux Vim private rc; not system IME or cross-platform'}, indent=2) + '\n')
    print('workspace.vim_real_application_cancel_bracketed_unicode_autoindent_refresh_undo_write_close_confirmed')
except BaseException:
    (evidence / 'workspace-vim-failure.json').write_text(json.dumps({'stage': stage}, indent=2) + '\n')
    if xid is not None and app is not None and app.poll() is None:
        run(['/usr/bin/import', '-window', xid, str(evidence / 'workspace-vim-failure.png')], check=False)
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
        try:
            os.kill(native_pid, 0)
        except ProcessLookupError:
            pass
        else:
            # Only terminate this task's private Vim, never a reused PID.
            proc = Path('/proc') / str(native_pid) / 'cmdline'
            if proc.exists() and str(fixture / 'vimrc').encode() in proc.read_bytes().split(b'\0'):
                os.kill(native_pid, signal.SIGTERM)
                deadline = time.monotonic() + 3
                while proc.exists() and time.monotonic() < deadline:
                    time.sleep(0.05)
            if proc.exists():
                raise RuntimeError('workspace.vim.cleanup_native_remaining')
