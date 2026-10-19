#!/usr/bin/env python3
"""Production WebView with an isolated test server preserving password partial success."""
import base64
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

os.environ["MAGICK_THREAD_LIMIT"] = "1"
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
def sidebar_click(name):
    deadline = time.monotonic() + 8
    for direction in ('5', '4'):
        for _ in range(20):
            if time.monotonic() >= deadline:
                raise RuntimeError('password_joint.timeout.sidebar.' + name)
            if accessible(name, 'activate'):
                return
            run(['xdotool', 'mousemove', '--window', xid, '90', '400', 'click', direction])
    raise RuntimeError('password_joint.missing.sidebar.' + name)
def pointer_click(name):
    wait(lambda: accessible(name, 'bounds'), 'bounds.' + name)
    assert app is not None
    rectangle = run([str(helper), str(app.pid), name, 'bounds']).stdout
    left, top, width, height = map(int, rectangle.decode().split())
    run(['xdotool', 'mousemove', str(left + width // 2), str(top + height // 2), 'click', '1'])
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
def catalogue(key):
    with database() as db:
        row = db.execute('SELECT value FROM app_state WHERE key=?', (key,)).fetchone()
        assert row is not None
        return json.loads(row[0])
def stable_sources(excluded):
    state = snapshot()
    state['app_state'] = [row for row in state['app_state'] if row[0] not in excluded]
    return state
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
(evidence / 'result.json').write_text(json.dumps({'version': 3, 'passed': False, 'stage': 'not_started'}, indent=2) + '\n')
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
        hosts = []
        for _, raw in db.execute('SELECT key,value FROM app_state'):
            try:
                document = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                continue
            if isinstance(document, dict):
                hosts.extend(document.get('hosts', []))
    host = next(row for row in hosts if row.get('label') == 'GUI_HOST')
    address = f"{host['username']}@{host['host']}:{host['port']}"
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
    def temporary_connect():
        click('快速连接')
        fill('快速连接地址', address)
        click('核对地址')
        click('临时连接')
        wait(lambda: accessible('选择保存的SSH身份', 'focus'), 'temporary_identity')
        key('Home')
        key('Down')
        click('连接')
        questions()

    stage = 'temporary_password_joint_cancel_drains_without_write'
    before_cancel = remote('ssh_password_joint_stream_exited')
    before_success_exit = remote('ssh_authenticated_stream_exited')
    temporary_connect()
    key('Escape')
    wait(lambda: remote('ssh_password_joint_stream_exited') == before_cancel + 1, 'temporary_cancel_drained')
    assert remote('ssh_authenticated_stream_exited') == before_success_exit
    assert native('ssh_session_opened') == 2 and snapshot() == baseline
    checks.append(stage)
    stage = 'temporary_joint_skip_save_and_refresh_without_reauthentication'
    temporary_connect()
    authenticate()
    wait(lambda: native('ssh_session_opened') == 3, 'temporary_registered')
    click('暂不保存')
    assert snapshot() == baseline
    input_line('PASSWORD_TEMPORARY', 4)
    key('ctrl+shift+r')
    wait(lambda: native('document_confirmed') == 3, 'temporary_refresh')
    input_line('PASSWORD_TEMPORARY_REFRESH', 5)
    assert remote('ssh_interactive_authenticated') == 3
    checks.append(stage)
    stage = 'temporary_joint_explicit_save_reuses_exact_existing_target'
    temporary_connect()
    authenticate()
    wait(lambda: native('ssh_session_opened') == 4, 'temporary_second_registered')
    assert snapshot() == baseline
    click('保存此主机')
    wait(lambda: not accessible('暂不保存'), 'explicit_save_finished')
    assert snapshot() == baseline
    input_line('PASSWORD_TEMPORARY_SAVED', 6)
    assert remote('ssh_interactive_authenticated') == 4
    run(['import', '-window', xid, str(evidence / 'temporary.png')])
    checks.append(stage)
    stage = 'new_temporary_target_fingerprint_cancel_zero_auth_zero_write'
    address = f"{host['username']}@localhost:{host['port']}"
    with database() as db:
        documents = []
        for state_key, raw in db.execute('SELECT key,value FROM app_state'):
            try:
                documents.append((state_key, json.loads(raw)))
            except (ValueError, UnicodeDecodeError):
                continue
        host_key = next(key for key, doc in documents if isinstance(doc, dict)
            and any(row.get('id') == host['id'] for row in doc.get('hosts', [])))
        graph_key = next(key for key, doc in documents if isinstance(doc, dict) and 'legacyRevision' in doc and 'profiles' in doc)
        public_key = db.execute('SELECT key FROM known_hosts WHERE deleted=0 ORDER BY ordinal').fetchone()[0]
    original_catalogue, original_graph = catalogue(host_key), catalogue(graph_key)
    words = public_key.split()
    assert words[0] == 'ssh-ed25519'
    fingerprint = 'SHA256:' + base64.b64encode(hashlib.sha256(base64.b64decode(words[1], validate=True)).digest()).decode().rstrip('=')
    click('快速连接')
    fill('快速连接地址', address)
    click('核对地址')
    click('临时连接')
    wait(lambda: accessible('选择保存的SSH身份', 'focus'), 'new_target_identity')
    key('Home', 'Down')
    click('核对所选主机指纹')
    wait(lambda: accessible('首次见到此密钥。请通过独立可信渠道核对，保存仅添加应用指纹记录，不启动连接。'), 'first_use')
    assert accessible(fingerprint) and accessible(f"localhost:{host['port']}")
    run(['import', '-window', xid, str(evidence / 'first-use-fingerprint.png')])
    key('Escape')
    wait(lambda: accessible('确认临时SSH目标', 'focus'), 'returned_after_cancel')
    wait(lambda: remote('ssh_password_joint_stream_exited') == 9, 'fingerprint_cancel_drained')
    assert snapshot() == baseline and remote('ssh_password_joint_auth_attempt') == 12
    checks.append(stage)
    stage = 'explicit_fingerprint_save_never_authenticates_or_saves_host'
    click('核对所选主机指纹')
    wait(lambda: accessible(fingerprint), 'second_first_use')
    click('保存指纹')
    wait(lambda: accessible('已保存此指纹 · 没有启动 SSH 连接'), 'fingerprint_saved')
    click('完成')
    assert catalogue(host_key) == original_catalogue and catalogue(graph_key) == original_graph
    assert remote('ssh_password_joint_auth_attempt') == 12 and native('ssh_session_opened') == 4
    immutable = stable_sources({host_key, graph_key})
    checks.append(stage)
    stage = 'new_joint_session_explicit_save_creates_one_host_and_recent_id'
    click('连接')
    authenticate()
    wait(lambda: native('ssh_session_opened') == 5, 'new_target_registered')
    assert catalogue(host_key) == original_catalogue and catalogue(graph_key) == original_graph
    pointer_click('临时连接保存名称')
    key('ctrl+a')
    run(['xdotool', 'type', '--clearmodifiers', '--delay', '5', 'PASSWORD_NEW_HOST'])
    run(['import', '-window', xid, str(evidence / 'new-host-named.png')])
    pointer_click('保存此主机')
    wait(lambda: not accessible('暂不保存'), 'new_target_save_finished')
    final_catalogue, final_graph = catalogue(host_key), catalogue(graph_key)
    previous_ids = {row['id'] for row in original_catalogue['hosts']}
    created = [row for row in final_catalogue['hosts'] if row['id'] not in previous_ids]
    (evidence / 'new-host-state.json').write_text(json.dumps({'created': [
        {field: row[field] for field in ('id', 'label', 'host', 'port', 'username')}
        for row in created], 'revision': final_catalogue['revision']}, indent=2) + '\n')
    assert len(created) == 1
    saved = created[0]
    assert saved['label'] == 'PASSWORD_NEW_HOST' and saved['host'] == 'localhost'
    assert saved['port'] == host['port'] and saved['username'] == host['username']
    assert final_catalogue['revision'] == original_catalogue['revision'] + 1
    assert [row for row in final_catalogue['hosts'] if row['id'] in previous_ids] == original_catalogue['hosts']
    for collection in ('hosts', 'endpoints', 'profiles'):
        assert len(final_graph[collection]) == len(original_graph[collection]) + 1
        assert all(row in final_graph[collection] for row in original_graph[collection])
    assert stable_sources({host_key, graph_key}) == immutable
    wait(lambda: catalogue('catalogue.recent-ssh-hosts.v1')['entries'][0]['host'] == saved['id'], 'saved_recent_id')
    input_line('PASSWORD_NEW_HOST_SAVED', 7)
    key('ctrl+shift+r')
    wait(lambda: native('document_confirmed') == 4, 'new_host_refresh')
    input_line('PASSWORD_NEW_HOST_REFRESH', 8)
    assert remote('ssh_interactive_authenticated') == 5
    assert catalogue(host_key) == final_catalogue and catalogue(graph_key) == final_graph
    run(['import', '-window', xid, str(evidence / 'new-host-saved.png')])
    checks.append(stage)
    stage = 'new_host_persists_and_reconnect_requires_fresh_joint_authentication'
    gui_pids = [app.pid]
    key('alt+F4')
    assert app.wait(timeout=8) == 0
    wait(lambda: remote('ssh_password_joint_stream_exited') == 15, 'first_window_drained', 3)
    before_sessions = native('sessions_listed')
    with app_log.open('ab') as output:
        app = subprocess.Popen([str(binaries / 'pulseterm-workspace')], stdout=output, stderr=output, start_new_session=True)
    gui_pids.append(app.pid)
    wait(window, 'reopened_owned_window', 30)
    wait(lambda: native('sessions_listed') > before_sessions, 'reopened_page', 30)
    run(['xdotool', 'windowactivate', '--sync', xid])
    run(['xdotool', 'windowsize', '--sync', xid, '1280', '800'])
    assert catalogue(host_key) == final_catalogue and catalogue(graph_key) == final_graph
    assert native('ssh_session_opened') == 5 and remote('ssh_interactive_authenticated') == 5
    sidebar_click("管理保险库")
    fill('主密码', 'fixed-target-master')
    click('解锁保险库')
    wait(lambda: native('vault_front_confirmed') == 2, 'reopened_unlock')
    key('Escape')
    sidebar_click("连接")
    wait(lambda: accessible('选择保存的SSH主机', 'focus'), 'reopened_hosts')
    key('End')
    description = run([str(helper), str(app.pid), '选择保存的SSH主机', 'description']).stdout.decode()
    assert 'PASSWORD_NEW_HOST' in description and 'localhost' in description
    wait(lambda: accessible('选择保存的SSH身份', 'focus'), 'reopened_identity')
    key('Home', 'Down')
    click('连接')
    authenticate()
    wait(lambda: native('ssh_session_opened') == 6, 'saved_new_host_registered')
    input_line('PASSWORD_NEW_HOST_REOPENED', 9)
    assert catalogue(host_key) == final_catalogue and catalogue(graph_key) == final_graph
    assert stable_sources({host_key, graph_key}) == immutable
    run(['import', '-window', xid, str(evidence / 'new-host-reopened.png')])
    checks.append(stage)
    stage = 'all_sixteen_connections_and_two_windows_exit'
    key('alt+F4')
    assert app.wait(timeout=8) == 0
    assert remote('ssh_password_joint_stream_exited') == 16
    assert native('native_drained') == native('exited') == 2
    assert remote('ssh_interactive_authenticated') == 6
    assert remote('ssh_password_joint_auth_attempt') == 14
    assert remote('ssh_password_joint_shell_requested') == 6
    assert 'workspace.terminal_failure.' not in app_log.read_text()
    groups = run(['ps', '-eo', 'pgid=,stat=']).stdout.decode().splitlines()
    assert not any(int(fields[0]) in gui_pids and not fields[1].startswith('Z')
        for line in groups if len(fields := line.split()) == 2)
    checks.append(stage)
    (evidence / 'result.json').write_text(json.dumps({'kind': 'production-linux-webview-saved-and-new-temporary-password-mfa-real-ssh',
        'version': 3, 'passed': True, 'checks': checks, 'successful_joint_sessions': 6, 'native_streams_exited': 16,
        'password_auth_attempts': 14, 'gui_pids': gui_pids, 'gui_exit_codes': [0, 0], 'final_input_lines': 9,
        'fixture_only_server_partial_success_correction': True, 'production_dependency_modified': False,
        'limitations': ['synthetic two-round MFA; no enterprise PAM',
        'new host save checked with password MFA only; no system or hardware Agent; Linux only']}, indent=2) + '\n')
except BaseException as error:
    (evidence / 'failure.json').write_text(json.dumps({'stage': stage, 'error': type(error).__name__, 'checks': checks}, indent=2) + '\n')
    if xid and app is not None and app.poll() is None:
        run(['import', '-window', xid, str(evidence / 'failure.png')], False)
    raise
finally:
    stop(app)
    stop(peer)
