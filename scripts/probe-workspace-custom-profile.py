#!/usr/bin/env python3
"""Actual Linux WebView saved-UUID Profile creation, reopen, GUI/CLI use and drain."""
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import time

fixture = Path(sys.argv[1])
evidence = Path('target/probes')
binaries = Path(os.environ.get('CARGO_TARGET_DIR', 'target')) / 'debug'
app = None
clients = []
xid = ''
phase = 0
checks = []
stage = 'startup'
log = evidence / 'workspace-custom-profile-native.log'

def run(args, check=True, timeout=3):
    return subprocess.run(args, capture_output=True, check=check, timeout=timeout)

def count(name):
    return log.read_text().splitlines().count('workspace.' + name)

def pids():
    file = fixture / 'pids'
    return [int(value) for value in file.read_text().splitlines()] if file.exists() else []

def wait(predicate, label):
    end = time.monotonic() + 8
    while time.monotonic() < end:
        if predicate():
            return
        if app is not None and app.poll() is not None:
            raise RuntimeError('custom_profile.exited.' + label)
        time.sleep(0.1)
    raise RuntimeError('custom_profile.timeout.' + label)

def key(*keys):
    run(['/usr/bin/xdotool', 'key', '--clearmodifiers', *keys])

def accessible(text, action=None):
    args = [str(evidence / 'x11-status'), str(app.pid), text]
    if action:
        args.append(action)
    return run(args, check=False, timeout=2).returncode == 0

def reveal(text, action=None):
    if accessible(text, action):
        return
    for direction, steps in [('5', 5), ('5', 5), ('5', 5), ('4', 18)]:
        run(['/usr/bin/xdotool', 'mousemove', '--window', xid, '700', '450', 'click', '--repeat', str(steps), '--delay', '30', direction])
        time.sleep(0.08)
        if accessible(text, action):
            return
    wait(lambda: accessible(text, action), text)

def click(text):
    reveal(text, 'activate')

def fill(label, value):
    reveal(label, 'focus')
    key('ctrl+a')
    run(['/usr/bin/xdotool', 'type', '--clearmodifiers', '--delay', '2', value])

def profiles():
    files = list((fixture / 'data').rglob('state.db'))
    if len(files) != 1:
        return []
    with sqlite3.connect(f'file:{files[0]}?mode=ro', uri=True) as db:
        row = db.execute("SELECT value FROM app_state WHERE key='profiles.custom.v1'").fetchone()
    return json.loads(row[0])['profiles'] if row else []

def launch():
    global app, xid, phase, log
    phase += 1
    log = evidence / f'workspace-custom-profile-{phase}-native.log'
    with log.open('wb') as output:
        app = subprocess.Popen([str(binaries / 'pulseterm-workspace')], stdout=output, stderr=output)
    wait(lambda: count('sessions_listed') == 1, 'first_page')
    def selected():
        global xid
        found = run(['/usr/bin/xdotool', 'search', '--onlyvisible', '--all', '--pid', str(app.pid), '--name', '^PulseTerm · 工作区预览$'], check=False).stdout.split()
        if len(found) != 1:
            return False
        xid = found[0].decode()
        return run(['/usr/bin/xdotool', 'getwindowpid', xid]).stdout.strip() == str(app.pid).encode()
    wait(selected, 'window_identity')
    run(['/usr/bin/xdotool', 'windowactivate', '--sync', xid])

def close_app():
    key('alt+F4')
    assert app.wait(timeout=8) == 0 and count('exited') == 1
    assert 'workspace.terminal_failure.' not in log.read_text()
    for pid in pids():
        assert not (Path('/proc') / str(pid)).exists()

def verify_receipt(pid):
    receipt = fixture / f'receipt.{pid}'
    wait(receipt.exists, 'native_literal_receipt')
    assert receipt.read_bytes().split(b'\0') == [str(fixture / 'work').encode(), b'PulseTerm', b'']

signal.signal(signal.SIGTERM, lambda number, frame: sys.exit(128 + number))
try:
    launch()
    stage = 'native_save_has_no_launch'
    click('管理')
    click('自定义 Profile')
    click('新建 Profile')
    fill('Profile 名称', 'Native Saved')
    fill('程序绝对路径', str(fixture / 'bin/custom-profile'))
    fill('工作目录（留空继承；填写绝对路径）', str(fixture / 'work'))
    click('保存配置')
    wait(lambda: len(profiles()) == 1, 'saved_record')
    saved = profiles()[0]
    saved_id = saved['id']
    assert saved['draft']['executable'] == str(fixture / 'bin/custom-profile')
    assert pids() == [] and count('session_opened') == 0
    checks.append(stage)
    stage = 'gui_explicit_saved_identity_starts_native_program'
    click('关闭编辑')
    click('打开终端')
    wait(lambda: count('terminal_input_ready') >= 1 and len(pids()) == 1, 'first_native_terminal')
    verify_receipt(pids()[0])
    run(['/usr/bin/import', '-window', xid, str(evidence / 'workspace-custom-profile-gui.png')])
    close_app()
    checks.append(stage)
    stage = 'independent_process_reopens_same_uuid_without_start'
    launch()
    assert profiles()[0]['id'] == saved_id and len(pids()) == 1 and count('session_opened') == 0
    checks.append(stage)
    stage = 'cli_directory_exposes_saved_id_without_execution_parameters'
    directory = run([str(binaries / 'pulse'), 'profiles'], timeout=10)
    assert saved_id.encode() in directory.stdout
    assert str(fixture / 'bin/custom-profile').encode() not in directory.stdout
    assert len(pids()) == 1 and count('session_opened') == 0
    checks.append(stage)
    stage = 'cli_saved_identity_requires_original_window_confirmation'
    client = subprocess.Popen([str(binaries / 'pulse'), 'open', saved_id], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    clients.append(client)
    wait(lambda: accessible('创建终端'), 'cli_original_window_confirmation')
    assert len(pids()) == 1 and count('session_opened') == 0
    click('创建终端')
    wait(lambda: count('terminal_input_ready') >= 1 and len(pids()) == 2, 'cli_native_terminal')
    stdout, stderr = client.communicate(timeout=8)
    assert client.returncode == 0, stderr.decode(errors='replace')
    verify_receipt(pids()[-1])
    checks.append(stage)
    stage = 'refresh_recovers_existing_custom_session_without_relaunch'
    ready = count('terminal_input_ready')
    key('ctrl+shift+r')
    wait(lambda: count('terminal_input_ready') > ready, 'recovered_custom_session')
    assert profiles()[0]['id'] == saved_id and len(pids()) == 2
    close_app()
    checks.append(stage)
    (evidence / 'workspace-custom-profile-result.json').write_text(json.dumps({'checks': checks, 'savedId': saved_id, 'nativeStarts': len(pids()), 'processes': phase}, indent=2) + '\n')
    print('workspace.custom_profile_saved_uuid_reopen_gui_cli_confirmation_refresh_native_exit_confirmed')
except BaseException as error:
    (evidence / 'workspace-custom-profile-failure.json').write_text(json.dumps({'stage': stage, 'error': str(error), 'checks': checks}, indent=2) + '\n')
    if app is not None and app.poll() is None and xid:
        run(['/usr/bin/import', '-window', xid, str(evidence / 'workspace-custom-profile-failure.png')], check=False)
    raise
finally:
    for client in clients:
        if client.poll() is None:
            client.terminate()
            try:
                client.wait(timeout=3)
            except subprocess.TimeoutExpired:
                client.kill()
                client.wait(timeout=3)
    if app is not None and app.poll() is None:
        app.terminate()
        try:
            app.wait(timeout=8)
        except subprocess.TimeoutExpired:
            app.kill()
            app.wait(timeout=3)
    for pid in pids():
        env = Path('/proc') / str(pid) / 'environ'
        try:
            if env.exists() and f'PROFILE_NATIVE_FIXTURE={fixture}'.encode() in env.read_bytes().split(b'\0'):
                os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
