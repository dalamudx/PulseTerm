#!/usr/bin/env python3
"""Production WebView with an isolated test server preserving password partial success."""
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
evidence = Path('target/probes/workspace-password-interactive')
evidence.mkdir(parents=True, exist_ok=True)
app_log, peer_log = evidence / 'native.log', evidence / 'peer.log'
helper = fixture / 'probe-x11-status'
policy = fixture / 'password-policy'
os.environ['PULSE_SSH_PASSWORD_MFA_POLICY'] = str(policy)
policy.write_text('partial')
app = peer = None
xid = ''
stage = 'startup'
checks = []

def run(args, check=True):
    return subprocess.run(args, capture_output=True, check=check, timeout=3)
def count(path, event):
    return path.read_text().splitlines().count(event) if path.exists() else 0
def native(event):
    return count(app_log, 'workspace.' + event)
def remote(event):
    return count(peer_log, 'fixture.' + event)
def wait(predicate, name, seconds=8):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        if app is not None and app.poll() is not None and native('exited') != 1:
            raise RuntimeError('password_joint.app_exited.' + name)
        if peer is not None and peer.poll() is not None:
            raise RuntimeError('password_joint.peer_exited.' + name)
        time.sleep(.1)
    raise RuntimeError('password_joint.timeout.' + name)
def accessible(name, action=None):
    if app is None:
        return False
    return run([str(helper), str(app.pid), name] + ([action] if action else []), False).returncode == 0
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
def database(writable=False):
    path = Path(os.environ['XDG_DATA_HOME']) / 'dev.pulseterm.workspace-preview/state.db'
    return sqlite3.connect('file:' + str(path) + ('?mode=rw' if writable else '?mode=ro'), uri=True)
def snapshot():
    with database() as db:
        tables = db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").fetchall()
        result = {}
        for (table,) in tables:
            rows = db.execute('SELECT * FROM "' + table.replace('"', '""') + '"').fetchall()
            if table == 'app_state':
                rows = [row for row in rows if row[0] != 'catalogue.recent-ssh-hosts.v1']
            result[table] = sorted(rows, key=repr)
        return result
failure = '连接未成功，请检查已解锁的保险库、身份凭据和主机；未自动重试。'
def connect(joint=True):
    click('连接')
    wait(lambda: accessible('选择保存的SSH身份', 'focus'), 'identity')
    key('Home')
    if joint:
        key('Down')
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
    wait(lambda: remote('ssh_password_joint_stream_exited') == streams, 'drained')
    assert native('ssh_session_opened') == 0 and snapshot() == baseline
    click('取消')
def input_line(text, expected):
    wait(lambda: native('terminal_input_ready') >= expected, 'terminal_ready')
    run(['xdotool', 'mousemove', '--window', xid, '650', '380', 'click', '1'])
    run(['xdotool', 'type', '--clearmodifiers', '--delay', '5', text])
    key('Return')
    wait(lambda: remote('ssh_input_line_echoed') == expected, 'terminal_echo')
signal.signal(signal.SIGTERM, lambda number, frame: sys.exit(128 + number))
try:
    provenance = json.loads((evidence / 'server-provenance.json').read_text())
    assert provenance['fixtureOnly'] is True and provenance['productionDependencyModified'] is False
    assert provenance['partialSuccessMethod'] == 'password'
    server_binary = Path(os.environ['PULSE_SSH_FIXTURE_EXECUTABLE'])
    assert hashlib.sha256(server_binary.read_bytes()).hexdigest() == provenance['binarySha256']
    with peer_log.open('wb') as output:
        peer = subprocess.Popen([os.environ['PULSE_SSH_FIXTURE_EXECUTABLE'], 'serve-workspace-ssh-fixture',
            str(Path(os.environ['XDG_DATA_HOME']) / 'dev.pulseterm.workspace-preview'),
            'password-interactive'], stdout=output, stderr=output, start_new_session=True)
    wait(lambda: remote('ssh_gui_seeded_listening') == 1, 'seeded', 30)
    with app_log.open('wb') as output:
        app = subprocess.Popen([str(binaries / 'pulseterm-workspace')], stdout=output, stderr=output, start_new_session=True)
    def window():
        global xid
        if app is None:
            return False
        windows = run(['xdotool', 'search', '--onlyvisible', '--all', '--pid', str(app.pid),
            '--name', '^PulseTerm · 工作区预览$'], False).stdout.split()
        if len(windows) != 1:
            return False
        xid = windows[0].decode()
        return run(['xdotool', 'getwindowpid', xid]).stdout.strip() == str(app.pid).encode()
    wait(window, 'owned_window', 30)
    wait(lambda: native('sessions_listed') == 1, 'page', 30)
    run(['xdotool', 'windowactivate', '--sync', xid])
    run(['xdotool', 'windowsize', '--sync', xid, '1280', '800'])
    stage = 'unlock_does_not_authenticate'
    click('管理保险库')
    fill('主密码', 'fixed-target-master')
    click('解锁保险库')
    wait(lambda: native('vault_front_confirmed') == 1, 'unlock_front')
    click('重新读取状态')
    wait(lambda: native('vault_state_available') == 1, 'unlocked')
    key('Escape')
    baseline = snapshot()
    with database() as db:
        identity, label = db.execute('SELECT id,label FROM identities WHERE deleted=0').fetchone()
    assert remote('ssh_password_joint_auth_attempt') == 0
    checks.append(stage)
    stage = 'ordinary_password_partial_success_never_upgrades'
    connect(False)
    refused(1)
    assert remote('ssh_interactive_round_one') == 0
    checks.append(stage)
    for mode, event, streams in [('reject', 'rejected', 2), ('complete', 'complete_success', 3), ('partial-no-mfa', 'missing_mfa', 4)]:
        stage = 'explicit_joint_' + mode
        policy.write_text(mode)
        connect()
        refused(streams)
        assert remote('ssh_password_joint_' + event) == 1
        assert remote('ssh_interactive_round_one') == 0
        assert remote('ssh_password_joint_shell_requested') == 0
        checks.append(stage)
    policy.write_text('partial')
    stage = 'cancel_after_password_factor_drains'
    connect()
    questions()
    key('Escape')
    wait(lambda: remote('ssh_password_joint_stream_exited') == 5, 'cancel_drained')
    assert native('ssh_session_opened') == 0 and snapshot() == baseline
    checks.append(stage)
    stage = 'wrong_mfa_does_not_fallback_or_register'
    connect()
    questions()
    first('000000')
    refused(6)
    checks.append(stage)
    stage = 'changed_password_source_sends_no_second_answer'
    connect()
    questions()
    first()
    wait(lambda: accessible('Second verification code'), 'second_round')
    before = remote('ssh_interactive_second_answer_received')
    with database(True) as db:
        db.execute('UPDATE identities SET label=? WHERE id=?', (label + ' changed', identity))
    fill('Second verification code', '654321')
    click('提交本轮回答')
    wait(lambda: accessible(failure), 'source_denied')
    wait(lambda: remote('ssh_password_joint_stream_exited') == 7, 'source_drained')
    assert remote('ssh_interactive_second_answer_received') == before
    with database(True) as db:
        db.execute('UPDATE identities SET label=? WHERE id=?', (label, identity))
    assert snapshot() == baseline
    click('取消')
    checks.append(stage)
    stage = 'saved_password_two_rounds_register_one_session'
    connect()
    authenticate()
    wait(lambda: native('ssh_session_opened') == 1, 'first_registered')
    input_line('PASSWORD_JOINT_ONE', 1)
    assert snapshot() == baseline
    run(['import', '-window', xid, str(evidence / 'connected.png')])
    checks.append(stage)
    stage = 'refresh_recovers_without_repeating_factors'
    key('ctrl+shift+r')
    wait(lambda: native('document_confirmed') == 2, 'refresh')
    input_line('PASSWORD_JOINT_REFRESH', 2)
    assert remote('ssh_interactive_authenticated') == 1
    checks.append(stage)
    stage = 'second_explicit_joint_after_refresh'
    connect()
    authenticate()
    wait(lambda: native('ssh_session_opened') == 2, 'second_registered')
    input_line('PASSWORD_JOINT_TWO', 3)
    assert snapshot() == baseline
    assert remote('ssh_password_joint_premature_mfa_rejected') == 0
    run(['import', '-window', xid, str(evidence / 'refreshed.png')])
    checks.append(stage)
    stage = 'all_nine_connections_and_native_workers_exit'
    key('alt+F4')
    assert app.wait(timeout=8) == 0
    wait(lambda: remote('ssh_password_joint_stream_exited') == 9, 'all_streams', 3)
    assert native('native_drained') == native('exited') == 1
    assert remote('ssh_interactive_authenticated') == 2
    assert remote('ssh_password_joint_auth_attempt') == 9
    assert remote('ssh_password_joint_shell_requested') == 2
    assert 'workspace.terminal_failure.' not in app_log.read_text()
    checks.append(stage)
    (evidence / 'result.json').write_text(json.dumps({'kind': 'production-linux-webview-saved-password-and-mfa-real-ssh',
        'version': 1, 'passed': True, 'checks': checks, 'successful_joint_sessions': 2, 'native_streams_exited': 9,
        'fixture_only_server_partial_success_correction': True, 'production_dependency_modified': False,
        'limitations': ['synthetic two-round MFA; no enterprise PAM',
        'saved direct GUI only; temporary, CLI, routes and other platforms remain open']}, indent=2) + '\n')
except BaseException as error:
    (evidence / 'failure.json').write_text(json.dumps({'stage': stage, 'error': type(error).__name__, 'checks': checks}, indent=2) + '\n')
    if xid and app is not None and app.poll() is None:
        run(['import', '-window', xid, str(evidence / 'failure.png')], False)
    raise
finally:
    stop(app)
    stop(peer)
