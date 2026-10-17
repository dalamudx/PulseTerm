#!/usr/bin/env python3
"""Production Linux WebView plus synthetic real SSH interactive authentication."""
import json
import hashlib
import sqlite3
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

fixture = Path(sys.argv[1])
binaries = Path(os.environ.get('CARGO_TARGET_DIR', 'target')) / 'debug'
evidence = Path('target/probes')
app_log = evidence / 'workspace-temporary-interactive-native.log'
peer_log = evidence / 'workspace-temporary-interactive-peer.log'
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
def database():
    return sqlite3.connect('file:'+str(Path(os.environ['XDG_DATA_HOME'])/'dev.pulseterm.workspace-preview'/'state.db')+'?mode=ro', uri=True)
def documents():
    with database() as db:
        result = {}
        for name, raw in db.execute('SELECT key,value FROM app_state'):
            try:
                result[name] = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                pass
        return result
def snapshot(metadata=False):
    allowed = {'catalogue.recent-ssh-hosts.v1'}
    if metadata:
        for name, doc in documents().items():
            if isinstance(doc,dict) and ('hosts' in doc or ('legacyRevision' in doc and 'profiles' in doc)):
                allowed.add(name)
    with database() as db:
        result = {}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            quoted = table.replace('"','""')
            rows = db.execute('SELECT * FROM "'+quoted+'"').fetchall()
            if metadata and table == 'app_state':
                rows = [row for row in rows if row[0] not in allowed]
            result[table] = hashlib.sha256(repr(sorted(rows,key=repr)).encode()).hexdigest()
        return result
def temporary_connect():
    click('快速连接')
    fill('快速连接地址', address)
    click('核对地址')
    click('临时连接')
    wait(lambda: accessible('选择保存的SSH身份','focus'), 'temporary_identity_ready')
    key('End')
    click('连接')
    wait(lambda: accessible('Account <literal>','focus'), 'temporary_first_question')
def first_round(code='123456'):
    fill('Account <literal>','user')
    fill('First verification code',code)
    click('提交本轮回答')
def authenticate():
    first_round()
    fill('Second verification code','654321')
    click('提交本轮回答')

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

    hosts = [host for doc in documents().values() if isinstance(doc,dict) for host in doc.get('hosts',[])]
    host = next(host for host in hosts if host.get('label') == 'GUI_HOST')
    address = f"{host['username']}@{host['host']}:{host['port']}"
    baseline = snapshot()
    credentials = snapshot(True)
    stage = 'temporary_escape_revokes_question_and_drains_without_write'
    temporary_connect()
    key('Escape')
    wait(lambda: remote('ssh_interactive_stream_exited') == 1, 'temporary_cancel_drained', 3)
    assert snapshot() == baseline and remote('ssh_interactive_authenticated') == 0
    checks.append(stage)
    stage = 'wrong_answer_rejects_without_write_or_password_fallback'
    temporary_connect()
    first_round('000000')
    wait(lambda: remote('ssh_interactive_stream_exited') == 2, 'wrong_answer_drained', 3)
    click('取消')
    assert snapshot() == baseline and remote('ssh_interactive_authenticated') == 0
    assert remote('ssh_password_auth_attempt_rejected') == 0
    checks.append(stage)
    stage = 'two_round_temporary_authentication_skip_save_and_refresh'
    temporary_connect()
    authenticate()
    wait(lambda: native('ssh_session_opened') == 1, 'first_temporary_registered')
    wait(lambda: accessible('暂不保存'), 'save_is_separate')
    assert snapshot() == baseline
    click('暂不保存')
    assert snapshot() == baseline
    wait(lambda: native('terminal_input_ready') >= 1, 'temporary_terminal_input')
    key('ctrl+shift+r')
    wait(lambda: native('document_confirmed') == 2 and native('terminal_input_ready') >= 2, 'temporary_refresh')
    assert remote('ssh_interactive_authenticated') == 1 and native('ssh_session_opened') == 1
    assert snapshot() == baseline
    checks.append(stage)
    stage = 'explicit_save_reuses_exact_existing_target_without_credential_write'
    temporary_connect()
    authenticate()
    wait(lambda: native('ssh_session_opened') == 2, 'second_temporary_registered')
    wait(lambda: accessible('保存此主机'), 'second_save_is_separate')
    assert snapshot() == baseline
    click('保存此主机')
    wait(lambda: any(isinstance(doc,dict) and any(row.get('host') == host['id'] for row in doc.get('entries',[])) for name,doc in documents().items() if name == 'catalogue.recent-ssh-hosts.v1'), 'explicit_save_recent')
    assert snapshot(True) == credentials
    assert remote('ssh_interactive_authenticated') == 2 and remote('ssh_password_auth_attempt_rejected') == 0
    run(['import','-window',xid,str(evidence/'workspace-temporary-interactive.png')])
    checks.append(stage)
    stage = 'window_exit_waits_for_all_four_native_streams'
    key('alt+F4')
    assert app.wait(timeout=8) == 0
    wait(lambda: remote('ssh_interactive_stream_exited') == 4 and remote('ssh_authenticated_stream_exited') == 2, 'all_temporary_streams_drained', 3)
    assert native('native_drained') == 1 and native('exited') == 1
    assert 'workspace.terminal_failure.' not in app_log.read_text()
    checks.append(stage)
    (evidence/'workspace-temporary-interactive-result.json').write_text(json.dumps({'kind':'production-linux-webview-temporary-mfa-synthetic-real-ssh','passed':True,'checks':checks,'authenticated':2,'native_streams_exited':4,'limitations':['save reuses exact existing target metadata','isolated russh peer; no external PAM or enterprise MFA','no macOS or Windows acceptance']},indent=2)+'\n')

except BaseException as error:
    (evidence / 'workspace-temporary-interactive-failure.json').write_text(json.dumps({
        'stage': stage, 'error': type(error).__name__, 'checks': checks}, indent=2) + '\n')
    if xid and app is not None and app.poll() is None:
        run(['import', '-window', xid, str(evidence / 'workspace-temporary-interactive-failure.png')], False)
    raise
finally:
    stop(app)
    stop(peer)
