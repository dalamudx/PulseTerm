#!/usr/bin/env python3
"""Real default Profile selection, persistence, unavailable default and explicit startup."""
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
evidence = Path('target/probes')
app = None
xid = ''
phase = 0
log = evidence / 'workspace-profile-default-native.log'
stage = 'startup'
checks = []
children = []

def current_app():
    if app is None:
        raise RuntimeError('workspace.profile_default.no_application')
    return app

def run(args, check=True, timeout=3):
    return subprocess.run(args, capture_output=True, check=check, timeout=timeout)

def key(*keys):
    run(['/usr/bin/xdotool', 'key', '--clearmodifiers', *keys])

def count(name):
    return log.read_text().splitlines().count('workspace.' + name)

def wait(predicate, label):
    end = time.monotonic() + 8
    while time.monotonic() < end:
        if predicate():
            return
        if current_app().poll() is not None:
            raise RuntimeError('workspace.profile_default.exited.' + label)
        time.sleep(0.1)
    raise RuntimeError('workspace.profile_default.timeout.' + label)

def accessible(text, action=None):
    args = [str(evidence / 'x11-status'), str(current_app().pid), text]
    if action:
        args.append(action)
    return run(args, check=False, timeout=2).returncode == 0

def click(text):
    wait(lambda: accessible(text, 'activate'), text)

def database():
    paths = list((fixture / 'data').rglob('state.db'))
    if len(paths) != 1:
        raise RuntimeError('workspace.profile_default.database_identity')
    return paths[0]

def default_document(value):
    try:
        doc = json.loads(value)
        return isinstance(doc, dict) and set(doc) == {'schemaVersion', 'revision', 'shell'} and doc['schemaVersion'] == 1
    except (ValueError, TypeError):
        return False

def record():
    with sqlite3.connect(f'file:{database()}?mode=ro', uri=True) as db:
        rows = [value for (value,) in db.execute("SELECT value FROM app_state") if default_document(value)]
        if len(rows) > 1:
            raise RuntimeError("workspace.profile_default.ambiguous_record")
        row = (rows[0],) if rows else None
        return None if row is None else json.loads(row[0])

def other_tables():
    with sqlite3.connect(f'file:{database()}?mode=ro', uri=True) as db:
        result = {}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            quoted = table.replace('"', '""')
            if table == 'app_state':
                rows = [row for row in db.execute("SELECT * FROM app_state") if row[0] != "profiles.automatic.unix.v1" and not default_document(row[1])]
            else:
                rows = db.execute(f'SELECT * FROM "{quoted}"').fetchall()
            result[table] = hashlib.sha256(repr(sorted(rows, key=repr)).encode()).hexdigest()
        return result

def launch():
    global app, xid, phase, log
    phase += 1
    log = evidence / f'workspace-profile-default-{phase}-native.log'
    with log.open('wb') as output:
        app = subprocess.Popen(['target/debug/pulseterm-workspace'], stdout=output, stderr=output)
    wait(lambda: count('sessions_listed') == 1, 'first_page')
    def window():
        global xid
        found = run(['/usr/bin/xdotool','search','--onlyvisible','--all','--pid',str(current_app().pid),'--name','^PulseTerm · 工作区预览$'],check=False).stdout.split()
        if len(found) != 1:
            return False
        xid = found[0].decode()
        return run(['/usr/bin/xdotool','getwindowpid',xid]).stdout.strip() == str(current_app().pid).encode()
    wait(window,'window_identity')
    run(['/usr/bin/xdotool','windowactivate','--sync',xid])
    wait(lambda: accessible('POSIX sh'), 'profiles_rendered')

def open_manager(status):
    click('管理')
    wait(lambda: accessible(status), 'default_status')

def choose_sh():
    wait(lambda: accessible('默认 Profile','focus'),'select_focus')
    key('Home','End','Return')  # private Linux fixture: last discovered identity is POSIX sh

def save_default(expected):
    before = count('profile_default_saved')
    click('保存默认选择')
    wait(lambda: count('profile_default_saved') == before + 1, 'native_save')
    wait(lambda: accessible('默认选择已保存'), 'save_visible')
    value = record()
    if not value or value['shell'] != expected:
        raise RuntimeError('workspace.profile_default.record_mismatch')
    if count('session_opened') or (fixture / 'pids').exists():
        raise RuntimeError('workspace.profile_default.save_launched')
    return value

def screenshot(name):
    run(['/usr/bin/import','-window',xid,str(evidence/name)])

def close_app():
    key('alt+F4')
    code = current_app().wait(timeout=8)
    if code != 0 or count('exited') != 1:
        raise RuntimeError('workspace.profile_default.exit_failed')
    if 'workspace.terminal_failure.' in log.read_text():
        raise RuntimeError('workspace.profile_default.terminal_failure')

def interrupted(number, _frame):
    raise SystemExit(128+number)

signal.signal(signal.SIGTERM, interrupted)
try:
    launch()
    initial = other_tables()
    stage = 'save_without_launch'
    open_manager('尚未设置默认 Profile。')
    choose_sh()
    saved = save_default('sh')
    screenshot('workspace-profile-default-saved.png')
    checks.append(stage)
    stage = 'refresh_same_choice_no_launch'
    before = count('document_confirmed')
    key('ctrl+shift+r')
    wait(lambda: count('document_confirmed') == before+1,'new_page')
    open_manager('当前默认：POSIX sh')
    assert record() == saved and count('session_opened') == 0
    checks.append(stage)
    click('完成')
    close_app()
    stage = 'independent_process_reopen'
    launch()
    open_manager('当前默认：POSIX sh')
    assert record() == saved and count('session_opened') == 0
    checks.append(stage)
    stage = 'hidden_default_unavailable'
    click('显示 POSIX sh')
    wait(lambda: accessible('已保存的默认 Profile 不可用，请恢复显示或重新选择。'),'unavailable')
    assert record() == saved
    assert not accessible('启动默认 Profile','activate')
    assert count('session_opened') == 0 and not (fixture/'pids').exists()
    screenshot('workspace-profile-default-unavailable.png')
    checks.append(stage)
    stage = 'clear_and_resave'
    wait(lambda: accessible('默认 Profile','focus'),'clear_focus')
    key('Home','Return')
    cleared = save_default(None)
    assert cleared['revision'] > saved['revision']
    click('显示 POSIX sh')
    wait(lambda: accessible('尚未设置默认 Profile。'),'restored')
    choose_sh()
    resaved = save_default('sh')
    assert resaved['revision'] > cleared['revision']
    checks.append(stage)
    stage = 'explicit_one_start_echo_and_refresh'
    click('启动默认 Profile')
    wait(lambda: count('terminal_input_ready') == 1,'input_ready')
    wait(lambda: (fixture/'pids').exists(),'native_pid')
    children = [int(p) for p in (fixture/'pids').read_text().splitlines()]
    assert len(children) == 1 and count('session_opened') == 1
    run(['/usr/bin/xdotool','mousemove','--window',xid,'420','235','click','1'])
    run(['/usr/bin/xdotool','type','--clearmodifiers','--delay','8','DEFAULT_PROFILE_INPUT'])
    key('Return')
    wait(lambda: (fixture/'input').exists() and (fixture/'input').read_text() == 'DEFAULT_PROFILE_INPUT\n','input_echo')
    before = count('document_confirmed')
    key('ctrl+shift+r')
    wait(lambda: count('document_confirmed') == before+1 and count('terminal_input_ready') == 2,'session_refresh')
    assert (fixture/'pids').read_text().splitlines() == [str(children[0])]
    os.kill(children[0],0)
    screenshot('workspace-profile-default-terminal.png')
    checks.append(stage)
    assert initial == other_tables()
    close_app()
    for pid in children:
        assert not (Path('/proc')/str(pid)).exists()
    checks.append('other_tables_unchanged_two_normal_exits_native_reaped')
    (evidence/'workspace-profile-default-result.json').write_text(json.dumps({'checks':checks,'revisions':[saved['revision'],cleared['revision'],resaved['revision']], 'other_tables':initial},indent=2)+'\n')
    print('workspace.profile_default_save_clear_unavailable_refresh_reopen_explicit_start_echo_native_exit_confirmed')
except BaseException as error:
    (evidence/'workspace-profile-default-failure.json').write_text(json.dumps({'stage':stage,'error':str(error),'checks':checks},indent=2)+'\n')
    if app is not None and app.poll() is None and xid:
        screenshot('workspace-profile-default-failure.png')
    raise
finally:
    if app is not None and app.poll() is None:
        app.terminate()
        try:
            app.wait(timeout=8)
        except subprocess.TimeoutExpired:
            app.kill(); app.wait(timeout=3)
    for pid in children:
        if (Path('/proc')/str(pid)).exists():
            os.kill(pid,signal.SIGKILL)
