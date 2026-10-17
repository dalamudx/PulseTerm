#!/usr/bin/env python3
"""Actual Linux WebView Profile editing, immutable session appearance and native options."""
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
stage = 'startup'
checks = []
children = []
log = evidence / 'workspace-profile-editor-native.log'

def run(args, check=True, timeout=3):
    return subprocess.run(args, capture_output=True, check=check, timeout=timeout)

def count(name):
    return log.read_text().splitlines().count('workspace.' + name)

def wait(predicate, label):
    end = time.monotonic() + 8
    while time.monotonic() < end:
        if predicate():
            return
        if app is not None and app.poll() is not None:
            raise RuntimeError('profile_editor.exited.' + label)
        time.sleep(0.1)
    raise RuntimeError('profile_editor.timeout.' + label)

def key(*keys):
    run(['/usr/bin/xdotool', 'key', '--clearmodifiers', *keys])

def accessible(text, action=None):
    assert app is not None
    args = [str(evidence/'x11-status'), str(app.pid), text]
    if action:
        args.append(action)
    return run(args, check=False, timeout=2).returncode == 0

def reveal(text, action=None):
    if accessible(text, action):
        return
    for direction, steps in [('5', 5), ('5', 5), ('5', 5), ('4', 18)]:
        run(['/usr/bin/xdotool','mousemove','--window',xid,'700','450','click','--repeat',str(steps),'--delay','30',direction])
        time.sleep(0.08)
        if accessible(text, action):
            return
    wait(lambda: accessible(text, action), text)

def click(text):
    reveal(text, 'activate')

def fill(label, value):
    reveal(label, "focus")
    key('ctrl+a')
    run(['/usr/bin/xdotool', 'type', '--clearmodifiers', '--delay', '4', value])

def database():
    files = list((fixture/'data').rglob('state.db'))
    if len(files) != 1:
        raise RuntimeError('profile_editor.database_identity')
    return files[0]

def records():
    with sqlite3.connect(f'file:{database()}?mode=ro', uri=True) as db:
        return dict(db.execute('SELECT key,value FROM app_state'))

def editor_record():
    return json.loads(records()['profiles.editor.unix.Sh.v1'])

def other_tables():
    with sqlite3.connect(f'file:{database()}?mode=ro', uri=True) as db:
        result = {}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            quoted = table.replace('"', '""')
            rows = db.execute(f'SELECT * FROM "{quoted}"').fetchall()
            if table == 'app_state':
                rows = [r for r in rows if not r[0].startswith('profiles.editor.') and r[0] != 'profiles.default.unix.v1']
            result[table] = hashlib.sha256(repr(sorted(rows, key=repr)).encode()).hexdigest()
        return result

def launch():
    global app, xid, phase, log
    phase += 1
    log = evidence/f'workspace-profile-editor-{phase}-native.log'
    with log.open('wb') as output:
        app = subprocess.Popen(['target/debug/pulseterm-workspace'], stdout=output, stderr=output)
    process = app
    wait(lambda: count('sessions_listed') == 1, 'first_page')
    def window():
        global xid
        found = run(['/usr/bin/xdotool','search','--onlyvisible','--all','--pid',str(process.pid),'--name','^PulseTerm · 工作区预览$'],check=False).stdout.split()
        if len(found) != 1:
            return False
        xid = found[0].decode()
        return run(['/usr/bin/xdotool','getwindowpid',xid]).stdout.strip() == str(process.pid).encode()
    wait(window, 'window_identity')
    run(['/usr/bin/xdotool','windowactivate','--sync',xid])

def edit():
    click('管理')
    click('编辑 POSIX sh')
    wait(lambda: accessible("工作目录（留空继承；填写绝对路径）"), "editor_ready")

def save():
    before = count('profile_editor_saved')
    click('保存配置')
    wait(lambda: count('profile_editor_saved') == before+1 and accessible('Profile 配置已保存。新终端将使用此配置。'), 'save_confirmed')
    click('关闭编辑')
    click('完成')

def pids():
    path = fixture/'pids'
    return [int(p) for p in path.read_text().splitlines()] if path.exists() else []

def size(pid):
    path = fixture/f'size.{pid}'
    rows = path.read_text().splitlines() if path.exists() else []
    return tuple(map(int,rows[-1].split())) if rows else (0,0)

def refresh():
    before = count('document_confirmed')
    key('ctrl+shift+r')
    wait(lambda: count('document_confirmed') == before+1,'refresh_page')

def screenshot(name):
    time.sleep(0.25)
    run(['/usr/bin/import','-window',xid,str(evidence/name)])

def close_app():
    assert app is not None
    key('alt+F4')
    code = app.wait(timeout=8)
    assert code == 0 and count('exited') == 1
    assert 'workspace.terminal_failure.' not in log.read_text()
    for pid in pids():
        assert not (Path('/proc')/str(pid)).exists()

def interrupted(number, _frame):
    raise SystemExit(128+number)
signal.signal(signal.SIGTERM,interrupted)
try:
    launch()
    original = other_tables()
    stage = 'cancel_and_navigation_are_zero_write_zero_launch'
    before = records()
    edit(); fill('工作目录（留空继承；填写绝对路径）',str(fixture/'work')); click('取消编辑'); click('完成')
    assert records() == before and pids() == [] and count('session_opened') == 0
    edit(); refresh(); wait(lambda: count('sessions_listed') == 2,'second_page_list')
    assert records() == before and pids() == [] and count('profile_editor_saved') == 0
    checks.append(stage)
    stage = 'initial_session_uses_inherited_appearance'
    click('POSIX sh'); wait(lambda: count('terminal_input_ready') == 1 and len(pids()) == 1,'initial_terminal')
    children = pids(); first = children[0]
    wait(lambda: size(first)[1] > 80,'initial_layout')
    original_size = size(first)
    checks.append(stage)
    stage = 'invalid_reserved_environment_is_rejected_without_write'
    edit(); click('添加环境变量'); fill('环境变量 1 名称','PULSE_SOCKET'); fill('环境变量 1 值','caller')
    click('保存配置'); wait(lambda: accessible('请检查参数总量、绝对路径、变量名称和外观值。'),'invalid_visible')
    assert records() == before and pids() == children
    click('取消编辑'); click('完成')
    checks.append(stage)
    stage = 'literal_arguments_cwd_environment_and_appearance_save_without_launch'
    edit(); click('添加参数'); fill('参数 1',' ; $(literal) '); click('添加参数')
    fill('工作目录（留空继承；填写绝对路径）',str(fixture/'work'))
    click('添加环境变量'); fill('环境变量 1 名称','PROFILE_VALUE'); fill('环境变量 1 值','exact = value')
    click('添加环境变量'); fill('环境变量 2 名称','EDITOR_REMOVE'); click('环境变量 2 移除继承值')
    fill('字体大小（留空继承；6–72）','18'); screenshot('workspace-profile-editor-form.png'); save()
    saved = editor_record()
    assert saved['config']['args'] == [' ; $(literal) ',''] and saved['config']['envRemove'] == ['EDITOR_REMOVE']
    assert saved['config']['appearance']['fontSize'] == 18
    assert pids() == children and count('session_opened') == 1 and size(first) == original_size
    checks.append(stage)
    stage = 'new_native_session_exact_bytes_and_larger_font_geometry'
    click('POSIX sh'); wait(lambda: count('terminal_input_ready') == 2 and len(pids()) == 2,'configured_terminal')
    children = pids(); second = children[1]
    expected = (str(fixture/'work')+'\0exact = value\0unset\0 ; $(literal) \0\0').encode()
    wait(lambda: (fixture/f'receipt.{second}').exists(),'receipt')
    assert (fixture/f'receipt.{second}').read_bytes() == expected
    wait(lambda: 0 < size(second)[1] < original_size[1],'font_geometry')
    configured_size = size(second)
    screenshot('workspace-profile-editor-terminal.png')
    checks.append(stage)
    stage = 'refresh_keeps_same_native_and_appearance_snapshot'
    refresh(); wait(lambda: count('terminal_input_ready') == 3,'configured_recapture')
    assert pids() == children and editor_record() == saved and size(second) == configured_size
    edit(); fill('字体大小（留空继承；6–72）','13'); save()
    assert pids() == children and size(second) == configured_size and editor_record()['revision'] > saved['revision']
    checks.append(stage)
    latest = editor_record()
    assert other_tables() == original
    close_app()
    stage = 'independent_process_reopen_retains_config_and_launches_only_explicitly'
    launch(); edit(); assert editor_record() == latest and count('session_opened') == 0
    screenshot('workspace-profile-editor-reopened.png'); click('取消编辑'); click('完成')
    assert pids() == children
    click('POSIX sh'); wait(lambda: count('terminal_input_ready') == 1 and len(pids()) == 3,'reopened_terminal')
    children = pids(); third = children[2]
    assert (fixture/f'receipt.{third}').read_bytes() == expected
    wait(lambda: size(third) == original_size,'restored_font_geometry')
    assert other_tables() == original
    close_app(); checks.append(stage)
    (evidence/'workspace-profile-editor-result.json').write_text(json.dumps({'checks':checks,'literal_native_bytes':len(expected),'original_size':original_size,'configured_size':configured_size,'record_revision':latest['revision'],'other_tables':original},indent=2)+'\n')
    print('workspace.profile_editor_cancel_navigation_bounds_save_no_launch_literal_native_options_appearance_refresh_reopen_native_exit_confirmed')
except BaseException as error:
    (evidence/'workspace-profile-editor-failure.json').write_text(json.dumps({'stage':stage,'error':str(error),'checks':checks},indent=2)+'\n')
    if app is not None and app.poll() is None and xid:
        screenshot('workspace-profile-editor-failure.png')
    raise
finally:
    if app is not None and app.poll() is None:
        app.terminate()
        try:
            app.wait(timeout=8)
        except subprocess.TimeoutExpired:
            app.kill(); app.wait(timeout=3)
    for pid in pids():
        env = Path('/proc')/str(pid)/'environ'
        try:
            if env.exists() and f'PULSETERM_EDITOR_FIXTURE={fixture}'.encode() in env.read_bytes().split(b'\0'):
                os.kill(pid,signal.SIGKILL)
        except ProcessLookupError:
            pass
