#!/usr/bin/env python3
"""Production Linux WebView plus synthetic real SSH interactive authentication."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

fixture = Path(sys.argv[1])
binaries = Path(os.environ.get('CARGO_TARGET_DIR', 'target')) / 'debug'
evidence = Path('target/probes')
app_log = evidence / 'workspace-ssh-interactive-native.log'
peer_log = evidence / 'workspace-ssh-interactive-peer.log'
helper = fixture / 'probe-x11-status'
app = peer = None
xid = ''
stage = 'startup'
checks = []

def run(args, check=True):
    return subprocess.run(args, capture_output=True, check=check, timeout=3)

def count(path, name):
    return path.read_text().splitlines().count(name) if path.exists() else 0

def native(name):
    return count(app_log, 'workspace.' + name)

def remote(name):
    return count(peer_log, 'fixture.' + name)

def wait(predicate, name, seconds=8):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        if predicate():
            return
        if app is not None and app.poll() is not None and native('exited') != 1:
            raise RuntimeError('interactive.app_exited.' + name)
        if peer is not None and peer.poll() is not None:
            raise RuntimeError('interactive.peer_exited.' + name)
        time.sleep(0.1)
    raise RuntimeError('interactive.timeout.' + name)

def accessible(name, action=None):
    args = [str(helper), str(app.pid), name]
    if action:
        args.append(action)
    return run(args, False).returncode == 0

def click(name):
    wait(lambda: accessible(name, 'activate'), 'activate.' + name)

def key(*keys):
    run(['xdotool', 'key', '--clearmodifiers', *keys])

def fill(name, value):
    wait(lambda: accessible(name, 'focus'), 'focus.' + name)
    key('ctrl+a')
    run(['xdotool', 'type', '--clearmodifiers', '--delay', '1', value])

def connect():
    click('连接')
    wait(lambda: accessible('选择保存的SSH主机', 'focus'), 'host_ready')
    # The seeded entry has no saved password. Its explicit MFA option is last.
    key('Tab', 'End', 'Tab', 'Tab', 'Return')
    wait(lambda: accessible('Account <literal>', 'focus'), 'first_question')

def stop(process):
    if process is None or process.poll() is not None:
        return
    os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=4)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=4)

signal.signal(signal.SIGTERM, lambda number, frame: sys.exit(128 + number))
try:
    with peer_log.open('wb') as output:
        peer = subprocess.Popen([os.environ.get('PULSE_SSH_FIXTURE_EXECUTABLE', str(binaries / 'xtask')), 'serve-workspace-ssh-fixture',
            str(Path(os.environ['XDG_DATA_HOME']) / 'dev.pulseterm.workspace-preview'),
            'interactive'], stdout=output, stderr=output, start_new_session=True)
    wait(lambda: remote('ssh_gui_seeded_listening') == 1, 'seeded', 20)
    with app_log.open('wb') as output:
        app = subprocess.Popen([str(binaries / 'pulseterm-workspace')],
            stdout=output, stderr=output, start_new_session=True)
    wait(lambda: native('sessions_listed') == 1, 'page')
    def window():
        global xid
        windows = run(['xdotool', 'search', '--onlyvisible', '--all', '--pid', str(app.pid),
                       '--name', '^PulseTerm · 工作区预览$'], False).stdout.split()
        if len(windows) != 1:
            return False
        xid = windows[0].decode()
        return run(['xdotool', 'getwindowpid', xid]).stdout.strip() == str(app.pid).encode()
    wait(window, 'owned_window')
    run(['xdotool', 'windowactivate', '--sync', xid])
    run(['xdotool', 'windowsize', '--sync', xid, '1280', '800'])
    stage = 'unlock_without_ssh_authority'
    click('管理保险库')
    fill('主密码', 'fixed-target-master')
    click('解锁保险库')
    wait(lambda: native('vault_front_confirmed') == 1, 'unlock_front')
    click('重新读取状态')
    wait(lambda: native('vault_state_available') == 1, 'unlocked')
    key('Escape')
    assert remote('ssh_interactive_authenticated') == 0 and native('ssh_session_opened') == 0
    checks.append(stage)

    stage = 'escape_cancels_before_answer_and_drains'
    connect()
    key('Escape')
    wait(lambda: remote('ssh_interactive_stream_exited') == 1, 'cancel_native_exit', 2)
    assert remote('ssh_interactive_authenticated') == 0 and native('ssh_session_opened') == 0
    checks.append(stage)

    stage = 'explicit_retry_two_rounds_echo_and_hidden_answers'
    connect()
    fill('Account <literal>', 'user')
    fill('First verification code', '123456')
    click('提交本轮回答')
    fill('Second verification code', '654321')
    click('提交本轮回答')
    wait(lambda: remote('ssh_interactive_authenticated') == 1, 'authenticated')
    wait(lambda: native('ssh_session_opened') == 1 and native('terminal_input_ready') >= 1, 'terminal')
    assert remote('ssh_password_authenticated') == 0 and remote('ssh_public_key_authenticated') == 0
    checks.append(stage)

    stage = 'real_terminal_input_and_refresh_without_reauthentication'
    run(['xdotool', 'type', '--clearmodifiers', '--delay', '1', 'mfa-echo'])
    key('Return')
    wait(lambda: remote('ssh_input_line_echoed') == 1, 'real_input')
    before = native('document_confirmed')
    key('ctrl+shift+r')
    wait(lambda: native('document_confirmed') == before + 1 and native('terminal_input_ready') >= 2, 'refresh')
    assert remote('ssh_interactive_authenticated') == 1 and native('ssh_session_opened') == 1
    run(['import', '-window', xid, str(evidence / 'workspace-ssh-interactive.png')])
    checks.append(stage)

    stage = 'window_exit_waits_for_native_stream'
    key('alt+F4')
    assert app.wait(timeout=8) == 0
    wait(lambda: remote('ssh_interactive_stream_exited') == 2 and remote('ssh_authenticated_stream_exited') == 1, 'owned_stream_exit', 2)
    assert native('native_drained') == 1 and native('exited') == 1
    assert 'workspace.terminal_failure.' not in app_log.read_text()
    checks.append(stage)
    (evidence / 'workspace-ssh-interactive-result.json').write_text(json.dumps({
        'kind': 'production-linux-webview-synthetic-real-ssh', 'passed': True,
        'checks': checks, 'authenticated': 1, 'native_streams_exited': 2,
        'limitations': ['isolated russh peer', 'no external PAM or enterprise MFA provider',
                        'no macOS or Windows acceptance']}, indent=2) + '\n')
    print('workspace.ssh_interactive_native_usage_passed')
except BaseException as error:
    (evidence / 'workspace-ssh-interactive-failure.json').write_text(json.dumps({
        'stage': stage, 'error': type(error).__name__, 'checks': checks}, indent=2) + '\n')
    if xid and app is not None and app.poll() is None:
        run(['import', '-window', xid, str(evidence / 'workspace-ssh-interactive-failure.png')], False)
    raise
finally:
    stop(app)
    stop(peer)
