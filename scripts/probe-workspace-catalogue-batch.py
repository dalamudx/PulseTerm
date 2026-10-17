#!/usr/bin/env python3
"""One real WebKit metadata flow; SQL is a read-only independent receipt."""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
fixture=Path(os.environ['PULSE_CATALOGUE_BATCH_ROOT'])
evidence=Path('target/probes')
database=Path(os.environ['PULSE_CATALOGUE_BATCH_DIRECTORY'])/'state.db'
app=None
stream=None
xid=''
log=evidence/'catalogue-batch-native-1.log'
result={'version':1,'scope':'Linux real WebView; metadata only','receipts':[]}
def run(args, check=True, timeout=3):
    return subprocess.run(args,check=check,capture_output=True,text=True,timeout=timeout)
def key(*keys):
    run(['/usr/bin/xdotool','key','--clearmodifiers',*keys])
def count(name):
    return log.read_text().splitlines().count('workspace.'+name)
def wait(predicate,label):
    end=time.monotonic()+8
    while time.monotonic()<end:
        if predicate(): return
        if app is not None and app.poll() is not None: raise RuntimeError('catalogue.exited.'+label)
        time.sleep(.1)
    raise RuntimeError('catalogue.timeout.'+label)
def accessible(text,action=None):
    assert app is not None
    args=[str(evidence/'x11-status'),str(app.pid),text]
    if action: args.append(action)
    return run(args,check=False)
def reveal(text,action):
    if accessible(text,action).returncode==0: return
    for direction in ('5','4'):
        for _ in range(20):
            run(['/usr/bin/xdotool','mousemove','--window',xid,'650','400','click',direction])
            time.sleep(.05)
            if accessible(text,action).returncode==0: return
    wait(lambda:accessible(text,action).returncode==0,text)
def click(text): reveal(text,'activate')
def focus(text): reveal(text,'focus')
def fill(text,value):
    focus(text); key('ctrl+a'); run(['/usr/bin/xdotool','type','--clearmodifiers','--delay','1',value])
def snapshot():
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        data={}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            quoted=table.replace('"','""')
            rows=db.execute(f'SELECT * FROM "{quoted}"').fetchall()
            data[table]=hashlib.sha256(repr(sorted(rows,key=repr)).encode()).hexdigest()
        return data

def rows(table):
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        if table=='hosts':
            candidates=[json.loads(value) for (value,) in db.execute("SELECT value FROM app_state") if value.startswith("{")]
            value=next(value for value in candidates if set(value)=={"schemaVersion","revision","hosts"})
            return sorted(r['label'] for r in value['hosts']),value['revision']
        return [r[0] for r in db.execute(f'SELECT label FROM {table} WHERE deleted=0 ORDER BY label')], db.execute(f'SELECT revision FROM {"identity_catalog" if table=="identities" else "key_identity_catalog"}').fetchone()[0]

def start(index):
    global app,stream,xid,log
    log=evidence/f'catalogue-batch-native-{index}.log'; stream=log.open('wb')
    app=subprocess.Popen(['target/debug/pulseterm-workspace'],stdout=stream,stderr=subprocess.STDOUT)
    wait(lambda:count('profiles_listed')==1,'profiles_ready')
    found=run(['/usr/bin/xdotool','search','--onlyvisible','--pid',str(app.pid),'--class','PulseTermWorkspace'],check=False).stdout.splitlines()
    assert found
    xid=found[-1];run(['/usr/bin/xdotool','windowactivate','--sync',xid])
def stop():
    global app,stream
    assert app is not None
    app.terminate()
    try: app.wait(timeout=4)
    except subprocess.TimeoutExpired: app.kill();app.wait(timeout=3)
    app=None
    if stream is not None: stream.close();stream=None

def open_kind(kind):
    if kind=='私钥身份': click('私钥')
    else:
        key('ctrl+shift+p');focus('搜索命令');run(['/usr/bin/xdotool','type','--clearmodifiers','host' if kind=='SSH 主机' else 'identity']);key('Return')
    focus('搜索'+kind)

try:
    initial=snapshot();start(1)
    for kind,table,operation in [('SSH 主机','hosts','hosts'),('密码身份','identities','identities'),('私钥身份','key_identities','key_identities')]:
        before=snapshot(); before_count=count(operation+'_operation_confirmed')
        open_kind(kind)
        if operation != "key_identities":
            wait(lambda:count(operation+"_operation_confirmed")==before_count+1,operation+"_read")
        before_count=count(operation+"_operation_confirmed")
        fill('搜索'+kind,'Batch ')
        run(['/usr/bin/import','-window',xid,str(evidence/f'catalogue-batch-{table}-filtered.png')],timeout=5)
        focus('选择当前搜索结果');key('space')
        run(['/usr/bin/import','-window',xid,str(evidence/f'catalogue-batch-{table}-selected.png')],timeout=5)
        click('删除所选（2）');click('取消批量删除')
        assert snapshot()==before,'cancel.changed_database'
        click('删除所选（2）');fill('搜索'+kind,'Keep');
        assert accessible('确认删除所选','activate').returncode!=0,'search.retained_confirmation'
        assert snapshot()==before,'search.changed_database'
        fill('搜索'+kind,'Batch ');focus('选择当前搜索结果');key('space');click('删除所选（2）')
        run(['/usr/bin/import','-window',xid,str(evidence/f'catalogue-batch-{table}-confirm.png')],timeout=5)
        click('确认删除所选')
        wait(lambda:rows(table)==(['Keep three'],4),operation+'_single_save')
        if operation != 'key_identities':
            wait(lambda:count(operation+'_operation_confirmed')==before_count+1,operation+'_save_receipt')
        focus('选择 Keep three · '+('batch-user@fixture.invalid:22' if table=='hosts' else 'batch-user'))
        assert 'workspace.session_started' not in log.read_text()
        run(['/usr/bin/import','-window',xid,str(evidence/f'catalogue-batch-{table}-saved.png')],timeout=5)
        key('Escape'); result['receipts'].append({'catalogue':table,'cancel_and_search_zero_write':True,'one_commit_revision':4,'survivors':['Keep three']})
    final=snapshot()
    assert initial.get('vault_state')==final.get('vault_state')
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        for table in ['identities','key_identities']:
            deleted,same=db.execute(f'SELECT count(*),min(hlc)=max(hlc) FROM {table} WHERE deleted=1').fetchone()
            assert deleted==2 and same
        credential_tables=[t for t in initial if 'credential' in t or 'secret' in t or 'vault' in t]
        assert all(initial[t]==final[t] for t in credential_tables),'credential_write'
    key('ctrl+shift+r');wait(lambda:count('document_confirmed')==2 and count('profiles_listed')==2,'refresh')
    assert snapshot()==final
    key('alt+F4');assert app is not None;app.wait(timeout=5)
    assert app.returncode == 0, ('application_exit', app.returncode, log.read_text())
    stop();start(2)
    for kind,table,_ in [('SSH 主机','hosts','hosts'),('密码身份','identities','identities'),('私钥身份','key_identities','key_identities')]:
        open_kind(kind);assert rows(table)==(['Keep three'],4)
        focus('选择 Keep three · '+('batch-user@fixture.invalid:22' if table=='hosts' else 'batch-user'))
        key('Escape')
    assert snapshot()==final
    assert 'workspace.session_started' not in log.read_text()
    key('alt+F4');assert app is not None; app.wait(timeout=5)
    assert app.returncode == 0, ('application_exit', app.returncode, log.read_text())
    result['refresh_and_process_reopen']=True; result['native_exit_confirmed']=True
    result['credentials_unchanged']=True;result['passed']=True
    (evidence/'catalogue-batch-result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print('workspace.catalogue_batch_cancel_search_single_commit_tombstone_refresh_reopen_no_launch_native_exit_confirmed')
finally:
    if app is not None and app.poll() is None:
        run(['/usr/bin/import','-window',xid,str(evidence/'catalogue-batch-failure.png')],check=False,timeout=5)
        stop()
    elif stream is not None: stream.close()
