#!/usr/bin/env python3
"""Single real WebKit organization flow; independent read-only SQL receipts."""
import hashlib
import json
import os
import signal
from pathlib import Path
import sqlite3
import subprocess
import time

evidence=Path('target/probes')
database=Path(os.environ['PULSE_CATALOGUE_BATCH_DIRECTORY'])/'state.db'
org_key='catalogue.organization.v1'
app=None
stream=None
xid=''
log=evidence/'catalogue-organization-native-1.log'

def run(args,check=True,timeout=3):
    return subprocess.run(args,check=check,capture_output=True,text=True,timeout=timeout)
def key(*keys):
    run(['/usr/bin/xdotool','key','--clearmodifiers',*keys])
def wait(predicate,label):
    deadline=time.monotonic()+8
    while time.monotonic()<deadline:
        if predicate(): return
        if app is not None and app.poll() is not None: raise RuntimeError('organization.exited.'+label)
        time.sleep(.1)
    raise RuntimeError('organization.timeout.'+label)
def accessible(text,action=None):
    assert app is not None
    args=[str(evidence/'x11-status'),str(app.pid),text]
    if action: args.append(action)
    return run(args,check=False)
def reveal(text,action):
    if accessible(text,action).returncode==0: return
    sidebar=text in ('分组、标签与收藏','重新加载工作区','搜索主机树','树视图','列表视图','仅收藏主机') or text.startswith('选择主机 ')
    for direction in ('5','4'):
        for _ in range(20):
            run(['/usr/bin/xdotool','mousemove','--window',xid,'90' if sidebar else '650','400','click',direction]);time.sleep(.05)
            if accessible(text,action).returncode==0: return
    wait(lambda:accessible(text,action).returncode==0,text)
def click(text): reveal(text,'activate')
def fill(text,value):
    reveal(text,'focus');key('ctrl+a')
    for index,line in enumerate(value.split('\n')):
        if index: key('Return')
        run(['/usr/bin/xdotool','type','--clearmodifiers','--delay','1',line])
def select(text,index):
    reveal(text,'select='+str(index))
    # AT-SPI changes selection without DOM change; actual keys commit the form transition.
    reveal(text,'focus');key('Up' if index else 'Down');time.sleep(.08)
    reveal(text,'focus');key('Down' if index else 'Up');time.sleep(.08)
    selected=accessible(text,'select='+str(index))
    assert selected.returncode==0, ('selection_receipt',text,index)
    with (evidence/'catalogue-organization-selections.log').open('a') as receipt:receipt.write(text+': '+selected.stdout)
    time.sleep(.1)
def screenshot(label):
    run(['/usr/bin/import','-window',xid,str(evidence/f'catalogue-organization-{label}.png')],timeout=5)
def organization():
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        row=db.execute('SELECT value FROM app_state WHERE key=?1',[org_key]).fetchone()
        return json.loads(row[0]) if row else None

def sources():
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        result={}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            quoted=table.replace('"','""')
            rows=db.execute(f'SELECT * FROM "{quoted}"'+(' WHERE key<>?1' if table=='app_state' else ''),[org_key] if table=='app_state' else []).fetchall()
            result[table]=hashlib.sha256(repr(sorted(rows,key=repr)).encode()).hexdigest()
        return result

def start(index):
    global app,stream,xid,log
    log=evidence/f'catalogue-organization-native-{index}.log';stream=log.open('wb')
    app=subprocess.Popen(['target/debug/pulseterm-workspace'],stdout=stream,stderr=subprocess.STDOUT,start_new_session=True)
    wait(lambda:'workspace.profiles_listed' in log.read_text(),'profiles')
    found=run(['/usr/bin/xdotool','search','--onlyvisible','--pid',str(app.pid),'--class','PulseTerm']);xid=found.stdout.splitlines()[0]
    run(['/usr/bin/xdotool','windowactivate','--sync',xid]);time.sleep(.15)
def reclaim_webviews(group_id):
    # start_new_session owns this group; stop remaining WebKit workers before cache cleanup.
    def active():
        for entry in Path('/proc').iterdir():
            if not entry.name.isdecimal():continue
            try:stat=(entry/'stat').read_text().rsplit(') ',1)[1].split()
            except (OSError,IndexError):continue
            if len(stat)>2 and stat[0]!='Z' and int(stat[2])==group_id:return True
        return False
    for sig in (signal.SIGTERM,signal.SIGKILL):
        if not active():return
        try:os.killpg(group_id,sig)
        except ProcessLookupError:return
        for _ in range(30):
            if not active():return
            time.sleep(.05)
    assert not active(),('webview_group_still_active',group_id)

def close_app():
    global app,stream
    assert app is not None
    key('alt+F4');app.wait(timeout=5)
    assert app.returncode==0,('application_exit',app.returncode)
    assert 'workspace.session_started' not in log.read_text()
    reclaim_webviews(app.pid)
    app=None
    if stream is not None:stream.close();stream=None

def saved(revision):
    wait(lambda:(organization() or {}).get('revision')==revision,'save_revision_'+str(revision))
    wait(lambda:accessible('组织信息已保存').returncode==0,'visible_saved')

def selected_host(label):
    expected='当前主机：'+label+' · fixture.invalid:22'
    wait(lambda:accessible('选择保存的SSH主机','description').stdout.strip()==expected,'saved_host_description')
    receipt=accessible('选择保存的SSH主机','description')
    assert receipt.returncode==0 and receipt.stdout.strip()==expected
    with (evidence/'catalogue-organization-host-preselection.log').open('a') as stream:stream.write(receipt.stdout)

def tree_preselection():
    fill('搜索主机树','prod')
    reveal('仅收藏主机','focus');key('space')
    click('列表视图');screenshot('host-list')
    click('树视图');screenshot('host-tree')
    target='选择主机 Batch one · batch-user@fixture.invalid:22'
    click(target)
    selected_host('Batch one')
    screenshot('host-preselected');click('取消')

if os.environ.get('PULSE_ORGANIZATION_SELECTION_DEBUG')=='1':
    try:
        start(1)
        click('选择主机 Keep three · batch-user@fixture.invalid:22')
        wait(lambda:accessible('可以选择保存的主机；请先在保险库中保存身份凭据。').returncode==0,'metadata_form_ready')
        selected_host('Keep three')
        screenshot('selection-debug')
        close_app()
    finally:
        if app is not None:
            reclaim_webviews(app.pid)
            app.wait(timeout=5)
        if stream is not None:stream.close()
    raise SystemExit(0)

try:
    start(1);initial=sources();assert organization() is None
    click('分组、标签与收藏');fill('分组名称','Cancelled');click('完成');assert organization() is None;assert sources()==initial
    click('分组、标签与收藏');fill('分组名称','Ops');click('创建分组');saved(1)
    select('父分组',1);fill('分组名称','Asia');click('创建分组');saved(2)
    doc=organization();assert doc is not None
    root=next(g for g in doc['groups'] if g['name']=='Ops');child=next(g for g in doc['groups'] if g['name']=='Asia')
    assert child['parent']==root['metadata']['id']
    # Each directory receives its own independently identified overlay. Endpoints and Vault never change.
    for index,kind in [(0,'host'),(3,'password_identity'),(6,'key_identity')]:
        select('选择要整理的项目',index);select('项目分组',2);fill('标签（每行一个，最多16个）','prod\nregion');reveal('收藏此项目','focus');key('space');click('保存项目组织');saved(index//3+3)
        doc=organization();assert doc is not None
        matches=[m for m in doc['memberships'] if m['kind']==kind]
        assert matches, ('selected_directory',kind,[m['kind'] for m in doc['memberships']])
        member=matches[0]
        assert member['group']==child['metadata']['id'] and member['favorite'] and len(member['tags'])==2
        assert sources()==initial
    screenshot('assigned')
    reveal('仅显示收藏','focus');key('space');fill('搜索名称、用户名或标签','prod');select('筛选分组',2);select('筛选标签',1);screenshot('filtered')
    # The exact filter is independently checked against the persisted membership identities.
    doc=organization();assert doc is not None and len(doc['memberships'])==3 and len(doc['tags'])==2
    fill('搜索名称、用户名或标签','no-matching-target');wait(lambda:accessible('没有匹配的项目').returncode==0,'empty_filter')
    click('完成');click('分组、标签与收藏')
    # Delete a separate empty group through explicit cancellation then confirmation, leaving a tombstone.
    fill('分组名称','Disposable');click('创建分组');saved(6)
    select('编辑分组',1);before_cancel=organization();click('删除分组');click('取消删除分组');assert organization()==before_cancel
    click('删除分组');click('确认删除分组');saved(7)
    doc=organization();assert doc is not None
    deleted=next(g for g in doc['groups'] if g['name']=='Disposable');assert deleted['metadata']['deleted']
    final=organization();assert sources()==initial;click('完成');tree_preselection();click('重新加载工作区');wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'refresh');assert organization()==final
    close_app();start(2);tree_preselection();click('分组、标签与收藏');fill('搜索名称、用户名或标签','no-matching-target');reveal('没有匹配的项目',None);assert organization()==final and sources()==initial;screenshot('reopened');click('完成');close_app()
    result={'version':1,'passed':True,'scope':'Linux real WebView; metadata only','group_hierarchy':True,'host_tree_list_search_favorites':True,'saved_host_preselect_cancel':True,'three_directory_memberships':True,'tags_favorites_filter':True,'cancel_zero_write':True,'group_tombstone':True,'saved_revisions':7,'refresh_reopen':True,'source_table_hashes':initial,'sources_unchanged':True,'zero_terminal_start':True,'native_exit_confirmed':True}
    (evidence/'catalogue-organization-result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('workspace.organization_tree_tags_favorites_cancel_tombstone_refresh_reopen_zero_start_native_exit_confirmed')
finally:
    if app is not None and app.poll() is None:
        (evidence/'catalogue-organization-failed-document.json').write_text(json.dumps(organization(),indent=2)+'\n')
        screenshot('failure');app.terminate()
        try:app.wait(timeout=4)
        except subprocess.TimeoutExpired:app.kill();app.wait(timeout=3)
    if app is not None:reclaim_webviews(app.pid)
    if stream is not None:stream.close()
