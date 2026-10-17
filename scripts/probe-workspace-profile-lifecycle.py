#!/usr/bin/env python3
"""Actual window tombstones/defaults, surviving PTY and explicit restoration."""
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
log = evidence / 'workspace-profile-lifecycle-native.log'
stage = 'startup'
checks = []
children = []
revisions = []

def current_app():
    if app is None:
        raise RuntimeError('workspace.profile_lifecycle.no_application')
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
            raise RuntimeError('workspace.profile_lifecycle.exited.' + label)
        time.sleep(0.1)
    raise RuntimeError('workspace.profile_lifecycle.timeout.' + label)

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
        raise RuntimeError('workspace.profile_lifecycle.database_identity')
    return paths[0]

def default_document(value):
    try:
        doc = json.loads(value)
        return isinstance(doc, dict) and set(doc) == {'schemaVersion', 'revision', 'shell'} and doc['schemaVersion'] == 1
    except (ValueError, TypeError):
        return False

def state():
    with sqlite3.connect(f'file:{database()}?mode=ro', uri=True) as db:
        return {key: json.loads(value) for key, value in db.execute("SELECT key,value FROM app_state") if key == "profiles.automatic.unix.v1" or default_document(value)}

def default_record():
    rows = state()
    values = [value for value in rows.values() if set(value) == {'schemaVersion','revision','shell'}]
    if len(values) != 1:
        raise RuntimeError('workspace.profile_lifecycle.default_record')
    return values[0]

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
    log = evidence / f'workspace-profile-lifecycle-{phase}-native.log'
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

def manager(status):
    click('管理')
    wait(lambda: accessible(status), 'default_status')
    wait(lambda: accessible('没有已删除项。') or accessible('恢复 POSIX sh'), 'deleted_list')

def screenshot(name):
    time.sleep(0.25)
    run(['/usr/bin/import','-window',xid,str(evidence/name)])

def close_app():
    key('alt+F4')
    code = current_app().wait(timeout=8)
    if code != 0 or count('exited') != 1:
        raise RuntimeError('workspace.profile_lifecycle.exit_failed')
    if 'workspace.terminal_failure.' in log.read_text():
        raise RuntimeError('workspace.profile_lifecycle.terminal_failure')

def refresh():
    before = count('document_confirmed')
    key('ctrl+shift+r')
    wait(lambda: count('document_confirmed') == before + 1, 'new_page')

def commit(action):
    before = count('profile_lifecycle_committed')
    click('确认' + action)
    wait(lambda: count('profile_lifecycle_committed') == before + 1, 'native_commit')
    message = 'Profile 已删除，已有终端继续运行。' if action == '删除' else 'Profile 已恢复，未启动终端。'
    wait(lambda: accessible(message), 'commit_visible')
    expected = '已保存的默认 Profile 不可用，请恢复显示或重新选择。' if action == '删除' else '当前默认：POSIX sh'
    wait(lambda: accessible(expected), 'fresh_default_status')
    record = default_record()
    assert record['shell'] == 'sh'
    revisions.append(record['revision'])
    return record

def interrupted(number, _frame):
    raise SystemExit(128 + number)
signal.signal(signal.SIGTERM, interrupted)
try:
    launch()
    initial = other_tables()
    stage = 'cancel_delete_zero_write_zero_session'
    manager('尚未设置默认 Profile。')
    before = state()
    click('删除 POSIX sh')
    wait(lambda: accessible('确认删除'), 'confirmation')
    screenshot('workspace-profile-lifecycle-confirm.png')
    click('取消')
    wait(lambda: accessible('没有已删除项。'), 'canceled')
    assert state() == before and count('profile_lifecycle_committed') == 0 and count('session_opened') == 0
    checks.append(stage)
    stage = 'default_saved_and_one_explicit_session'
    wait(lambda: accessible('默认 Profile','focus'),'select_focus')
    key('Home','End','Return')
    saved = count('profile_default_saved')
    click('保存默认选择')
    wait(lambda: count('profile_default_saved') == saved+1 and accessible('默认选择已保存'),'default_saved')
    revisions.append(default_record()['revision'])
    click('启动默认 Profile')
    wait(lambda: count('terminal_input_ready') == 1 and (fixture/'pids').exists(),'session_started')
    children = [int(p) for p in (fixture/'pids').read_text().splitlines()]
    assert len(children) == 1 and count('session_opened') == 1
    checks.append(stage)
    stage = 'delete_default_keeps_live_session_and_input'
    manager('当前默认：POSIX sh')
    click('删除 POSIX sh')
    wait(lambda: accessible('确认删除'),'default_confirmation')
    wait(lambda: accessible('删除后不再显示或发现此项，已有终端继续运行。它仍是默认 Profile，删除后默认启动不可用。'), 'default_warning')
    screenshot('workspace-profile-lifecycle-default-confirm.png')
    deleted = commit('删除')
    assert state()['profiles.automatic.unix.v1']['deleted'] == ['sh']
    assert not accessible('启动默认 Profile','activate')
    assert count('session_opened') == 1 and (fixture/'pids').read_text().splitlines() == [str(children[0])]
    os.kill(children[0],0)
    screenshot('workspace-profile-lifecycle-deleted.png')
    click('完成')
    run(['/usr/bin/xdotool','mousemove','--window',xid,'420','235','click','1'])
    run(['/usr/bin/xdotool','type','--clearmodifiers','--delay','8','LIVE_AFTER_DELETE'])
    key('Return')
    wait(lambda: (fixture/'input').exists() and (fixture/'input').read_text() == 'LIVE_AFTER_DELETE\n','live_input')
    checks.append(stage)
    stage = 'refresh_preserves_tombstone_and_original_pty'
    refresh()
    wait(lambda: count('terminal_input_ready') == 2,'session_recaptured')
    assert default_record() == deleted and count('session_opened') == 1
    os.kill(children[0],0)
    manager('已保存的默认 Profile 不可用，请恢复显示或重新选择。')
    assert accessible('恢复 POSIX sh') and not accessible('删除 POSIX sh')
    checks.append(stage)
    stage = 'cancel_and_refresh_restore_zero_write'
    before = state(); committed = count('profile_lifecycle_committed')
    click('恢复 POSIX sh'); wait(lambda: accessible('确认恢复'),'restore_confirm'); click('取消')
    wait(lambda: accessible('恢复 POSIX sh'),'restore_canceled')
    assert state() == before
    click('恢复 POSIX sh'); wait(lambda: accessible('确认恢复'),'restore_before_refresh')
    refresh(); wait(lambda: count('terminal_input_ready') == 3,'third_capture')
    manager('已保存的默认 Profile 不可用，请恢复显示或重新选择。')
    assert state() == before and count('profile_lifecycle_committed') == committed and not accessible('确认恢复')
    checks.append(stage)
    click('完成'); close_app()
    for pid in children:
        assert not (Path('/proc')/str(pid)).exists()
    assert initial == other_tables()
    stage = 'independent_reopen_explicit_restore_without_launch'
    launch()
    manager('已保存的默认 Profile 不可用，请恢复显示或重新选择。')
    assert default_record() == deleted and count('session_opened') == 0
    click('恢复 POSIX sh'); wait(lambda: accessible('确认恢复'),'independent_restore')
    restored = commit('恢复')
    assert restored['revision'] > deleted['revision']
    assert state()['profiles.automatic.unix.v1']['deleted'] == []
    assert accessible('显示 POSIX sh') and count('session_opened') == 0
    assert (fixture/'pids').read_text().splitlines() == [str(children[0])]
    screenshot('workspace-profile-lifecycle-restored.png')
    checks.append(stage)
    assert initial == other_tables()
    click('完成'); close_app()
    checks.append('two_normal_exits_original_native_reaped_other_tables_unchanged')
    (evidence/'workspace-profile-lifecycle-result.json').write_text(json.dumps({'checks':checks,'revisions':revisions,'other_tables':initial},indent=2)+'\n')
    print('workspace.profile_delete_restore_cancel_default_unavailable_live_pty_refresh_reopen_no_launch_native_exit_confirmed')
except BaseException as error:
    (evidence/'workspace-profile-lifecycle-failure.json').write_text(json.dumps({'stage':stage,'error':str(error),'checks':checks},indent=2)+'\n')
    if app is not None and app.poll() is None and xid:
        screenshot('workspace-profile-lifecycle-failure.png')
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
            os.kill(pid, signal.SIGKILL)
