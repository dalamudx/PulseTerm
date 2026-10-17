#!/usr/bin/env python3
"""Production WebView, encrypted synthetic private key, explicit key+MFA and real SSH."""
import hashlib
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import time

fixture = Path(sys.argv[1])
binaries = Path(os.environ.get('CARGO_TARGET_DIR', 'target')) / 'debug'
evidence = Path('target/probes')
app_log = evidence / 'workspace-key-interactive-native.log'
peer_log = evidence / 'workspace-key-interactive-peer.log'
helper = fixture / 'probe-x11-status'
policy = fixture / 'joint-policy'
os.environ['PULSE_SSH_KEY_MFA_POLICY'] = str(policy)
policy.write_text('partial')
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
            raise RuntimeError('joint.app_exited.' + name)
        if peer is not None and peer.poll() is not None:
            raise RuntimeError('joint.peer_exited.' + name)
        time.sleep(0.1)
    raise RuntimeError('joint.timeout.' + name)
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
def database():
    return sqlite3.connect('file:' + str(Path(os.environ['XDG_DATA_HOME']) / 'dev.pulseterm.workspace-preview' / 'state.db') + '?mode=ro', uri=True)
def documents():
    with database() as db:
        result = {}
        for name, raw in db.execute("SELECT key,value FROM app_state"):
            try:
                result[name] = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                pass
        return result
def snapshot():
    with database() as db:
        result = {}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            rows = db.execute('SELECT * FROM "' + table.replace('"', '""') + '"').fetchall()
            if table == 'app_state':
                rows = [row for row in rows if row[0] != 'catalogue.recent-ssh-hosts.v1']
            result[table] = hashlib.sha256(repr(sorted(rows, key=repr)).encode()).hexdigest()
        return result
failure = '连接未成功，请检查已解锁的保险库、身份凭据和主机；未自动重试。'
def connect(joint=True, temporary=False):
    if temporary:
        click('快速连接')
        fill('快速连接地址', address)
        click('核对地址')
        click('临时连接')
    else:
        click('连接')
    wait(lambda: accessible('选择保存的SSH身份', 'focus'), 'identity')
    key('End' if joint else 'Home')
    click('连接')
def questions():
    wait(lambda: accessible('Account <literal>', 'focus'), 'first_question')
def first(code='123456'):
    fill('Account <literal>', 'user')
    fill('First verification code', code)
    click('提交本轮回答')
def authenticate():
    questions()
    first()
    fill('Second verification code', '654321')
    click('提交本轮回答')
def refused(streams):
    wait(lambda: accessible(failure), 'refused')
    wait(lambda: remote('ssh_key_joint_stream_exited') == streams, 'denied_stream_drained')
    assert native('ssh_session_opened') == 0
    assert remote('ssh_password_auth_attempt_rejected') == 0
    assert snapshot() == baseline
    click('取消')
def input_line(text, count_expected):
    wait(lambda: native('terminal_input_ready') >= count_expected, 'terminal_input_ready')
    run(['xdotool', 'mousemove', '--window', xid, '650', '380', 'click', '1'])
    run(['xdotool', 'type', '--clearmodifiers', '--delay', '5', text])
    key('Return')
    wait(lambda: remote('ssh_input_line_echoed') == count_expected, 'terminal_echo')
try:
    with peer_log.open('wb') as output:
        peer = subprocess.Popen([os.environ.get('PULSE_SSH_FIXTURE_EXECUTABLE', str(binaries / 'xtask')),
            'serve-workspace-ssh-fixture', str(Path(os.environ['XDG_DATA_HOME']) / 'dev.pulseterm.workspace-preview'),
            'key-interactive'], stdout=output, stderr=output, start_new_session=True)
    wait(lambda: remote('ssh_gui_seeded_listening') == 1, 'seeded', 30)
    with app_log.open('wb') as output:
        app = subprocess.Popen([str(binaries / 'pulseterm-workspace')], stdout=output, stderr=output, start_new_session=True)
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
    stage = 'unlock_is_not_joint_authentication_authority'
    click('管理保险库')
    fill('主密码', 'fixed-target-master')
    click('解锁保险库')
    wait(lambda: native('vault_front_confirmed') == 1, 'unlock_front')
    click('重新读取状态')
    wait(lambda: native('vault_state_available') == 1, 'unlocked')
    key('Escape')
    hosts = [host for doc in documents().values() if isinstance(doc, dict) for host in doc.get('hosts', [])]
    host = next(host for host in hosts if host.get('label') == 'GUI_HOST')
    address = f"{host['username']}@{host['host']}:{host['port']}"
    baseline = snapshot()
    assert remote('ssh_key_joint_partial_success') == 0
    checks.append(stage)
    stage = 'normal_private_key_partial_success_never_prompts_or_falls_back'
    connect(False)
    refused(1)
    assert remote('ssh_interactive_challenge_round_1') == 0
    checks.append(stage)
    stage = 'explicit_joint_rejected_first_factor_never_prompts'
    policy.write_text('reject')
    connect()
    refused(2)
    assert remote('ssh_key_joint_key_rejected') == 1
    assert remote('ssh_interactive_challenge_round_1') == 0
    checks.append(stage)
    stage = 'explicit_joint_full_key_success_does_not_skip_mfa_policy'
    policy.write_text('complete')
    connect()
    refused(3)
    assert remote('ssh_key_joint_complete_key_success') == 1
    assert remote('ssh_key_joint_shell_requested') == 0
    checks.append(stage)
    policy.write_text('partial')
    stage = 'escape_after_first_factor_cancels_and_drains'
    connect()
    questions()
    key('Escape')
    wait(lambda: remote('ssh_key_joint_stream_exited') == 4, 'escape_drained')
    assert native('ssh_session_opened') == 0 and snapshot() == baseline
    checks.append(stage)
    stage = 'wrong_second_factor_rejects_without_fallback_or_root_session'
    connect()
    questions()
    first('000000')
    refused(5)
    checks.append(stage)
    stage = 'saved_private_key_and_two_round_mfa_registers_one_root_terminal'
    connect()
    authenticate()
    wait(lambda: native('ssh_session_opened') == 1, 'saved_opened')
    input_line('JOINT_ONE', 1)
    assert snapshot() == baseline
    checks.append(stage)
    stage = 'refresh_recovers_frozen_session_without_repeating_factors'
    key("ctrl+shift+r")
    wait(lambda: native("document_confirmed") == 2, "refreshed_original_window")
    input_line('JOINT_REFRESH', 2)
    assert remote('ssh_interactive_authenticated') == 1
    checks.append(stage)
    stage = 'temporary_private_key_plus_mfa_does_not_save_metadata'
    connect(temporary=True)
    authenticate()
    wait(lambda: native('ssh_session_opened') == 2, 'temporary_opened')
    click('暂不保存')
    input_line('JOINT_TEMP', 3)
    assert snapshot() == baseline
    assert remote('ssh_password_auth_attempt_rejected') == 0
    assert remote('ssh_key_joint_premature_mfa_rejected') == 0
    run(['import', '-window', xid, str(evidence / 'workspace-key-interactive.png')])
    checks.append(stage)
    stage = 'close_waits_for_all_seven_real_connections_and_native_workers'
    key('alt+F4')
    assert app.wait(timeout=8) == 0
    wait(lambda: remote('ssh_key_joint_stream_exited') == 7, 'all_streams_drained', 3)
    assert native('native_drained') == 1 and native('exited') == 1
    assert remote('ssh_interactive_authenticated') == 2
    assert 'workspace.terminal_failure.' not in app_log.read_text()
    checks.append(stage)
    (evidence / 'workspace-key-interactive-result.json').write_text(json.dumps({
        'kind': 'production-linux-webview-explicit-private-key-and-mfa-real-loopback-ssh',
        'passed': True, 'checks': checks, 'successful_joint_sessions': 2, 'native_streams_exited': 7,
        'limitations': ['synthetic encrypted Ed25519 fixture; no external enterprise MFA',
            'direct saved and temporary targets only; no CLI, jump, Agent or password joint flow',
            'no Windows, macOS or assistive technology acceptance']}, indent=2) + '\n')
except BaseException as error:
    (evidence / 'workspace-key-interactive-failure.json').write_text(json.dumps({
        'stage': stage, 'error': type(error).__name__, 'checks': checks}, indent=2) + '\n')
    if xid and app is not None and app.poll() is None:
        run(['import', '-window', xid, str(evidence / 'workspace-key-interactive-failure.png')], False)
    raise
finally:
    stop(app)
    stop(peer)
