#!/usr/bin/env python3
"""One real SSH registration, cancellation, recent projection and process reopen."""
import base64
import hashlib
import json
import os
import signal
import sqlite3
import subprocess
import time
import sys
from pathlib import Path

evidence=Path('target/probes')
directory=Path(os.environ['XDG_DATA_HOME'])/'dev.pulseterm.workspace-preview'
database=directory/'state.db'
recent_key='catalogue.recent-ssh-hosts.v1'
app=None
peer=None
xid=''
stream=None
peer_stream=None
cli_clients=[]
host_key=''
graph_key=''
prefix='ssh-agent-cli' if os.environ.get('PULSE_CLI_AGENT')=='1' else 'ssh-agent-route' if os.environ.get('PULSE_SSH_AGENT_ROUTE')=='1' else 'ssh-agent' if os.environ.get('PULSE_SSH_AGENT')=='1' else 'ssh-route' if os.environ.get('PULSE_SSH_ROUTE')=='1' else 'ssh-jump-config' if os.environ.get('PULSE_SSH_JUMPS')=='1' else 'temporary-ssh' if os.environ.get('PULSE_TEMPORARY_SSH')=='1' else 'ssh-profile' if os.environ.get('PULSE_SSH_PROFILES')=='1' else 'host-endpoint' if os.environ.get('PULSE_HOST_ENDPOINTS')=='1' else 'fuzzy-search' if os.environ.get('PULSE_FUZZY_SEARCH')=='1' else 'quick-connect' if os.environ.get('PULSE_QUICK_CONNECT')=='1' else 'recent-host'
if os.environ.get("PULSE_CLI_ROUTE") == "1": prefix = "ssh-agent-cli-route"
log=evidence/f'{prefix}-native-1.log'
peer_log=evidence/f'{prefix}-peer.log'
def run(args,check=True,timeout=3):return subprocess.run(args,check=check,capture_output=True,text=True,timeout=timeout)
def key(*values):run(['/usr/bin/xdotool','key','--clearmodifiers',*values])
def wait(predicate,label,timeout=8):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        if predicate():return
        if app is not None and app.poll() is not None:raise RuntimeError('recent.exited.'+label)
        time.sleep(.08)
    raise RuntimeError('recent.timeout.'+label)
def accessible(text,action=None):
    assert app is not None
    args=[str(evidence/'x11-status'),str(app.pid),text]
    if action:args.append(action)
    return run(args,check=False)
def reveal(text,action=None):
    if accessible(text,action).returncode==0:return
    sidebar=text in ('管理 SSH Agent 身份','搜索主机树','快速连接','最近连接','树视图','管理保险库','管理 SSH 主机','SSH 配置','重新加载工作区') or text.startswith('选择主机 ')
    for direction in ('5','4'):
        for _ in range(20):
            run(['/usr/bin/xdotool','mousemove','--window',xid,'90' if sidebar else '650','400','click',direction]);time.sleep(.05)
            if accessible(text,action).returncode==0:return
    wait(lambda:accessible(text,action).returncode==0,text)
def click(text):reveal(text,'activate')
def fill(text,value):
    reveal(text,'focus');key('ctrl+a');run(['/usr/bin/xdotool','type','--clearmodifiers','--delay','1',value])
def recent():
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        row=db.execute('SELECT value FROM app_state WHERE key=?',[recent_key]).fetchone()
        return json.loads(row[0]) if row else None
def route_trust():
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        rows=db.execute('SELECT id,created_at,hlc,deleted,host,port,key,ordinal FROM known_hosts ORDER BY ordinal').fetchall()
        state=db.execute('SELECT singleton,schema_version,revision,node_id,clock FROM known_hosts_state').fetchone()
        assert state is not None
        return rows,state

def sources():
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        result={}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            if prefix in {'ssh-agent','ssh-agent-cli'} and table in {'agent_identity_catalog','agent_identities'}:continue
            quoted=table.replace('"','""')
            excluded=[recent_key,host_key,graph_key] if prefix in {'host-endpoint','ssh-profile','temporary-ssh','ssh-jump-config'} else [recent_key]
            rows=db.execute(f'SELECT * FROM "{quoted}"'+(' WHERE key NOT IN ('+','.join('?' for _ in excluded)+')' if table=='app_state' else ''),excluded if table=='app_state' else []).fetchall()
            result[table]=hashlib.sha256(repr(sorted(rows,key=repr)).encode()).hexdigest()
        return result

def reclaim(group):
    def active():
        for entry in Path('/proc').iterdir():
            if not entry.name.isdecimal():continue
            try:stat=(entry/'stat').read_text().rsplit(') ',1)[1].split()
            except (OSError,IndexError):continue
            if len(stat)>2 and stat[0]!='Z' and int(stat[2])==group:return True
        return False
    for sig in (signal.SIGTERM,signal.SIGKILL):
        if not active():return
        try:os.killpg(group,sig)
        except ProcessLookupError:return
        for _ in range(30):
            if not active():return
            time.sleep(.05)
    assert not active(),('remaining_group',group)
def start(index):
    global app,stream,log,xid
    log=evidence/f'{prefix}-native-{index}.log';stream=log.open('wb')
    app=subprocess.Popen(['target/debug/pulseterm-workspace'],stdout=stream,stderr=subprocess.STDOUT,start_new_session=True)
    wait(lambda:'workspace.profiles_listed' in log.read_text(),'profiles')
    xid=run(['/usr/bin/xdotool','search','--onlyvisible','--pid',str(app.pid),'--class','PulseTerm']).stdout.splitlines()[0]
    run(['/usr/bin/xdotool','windowactivate','--sync',xid]);time.sleep(.15)
def close():
    global app,stream
    assert app is not None
    key('alt+F4');app.wait(timeout=6);assert app.returncode==0
    assert 'workspace.exited' in log.read_text()
    reclaim(app.pid);app=None
    if stream is not None:stream.close();stream=None
def agent_catalogue():
    with sqlite3.connect('file:'+str(database)+'?mode=ro',uri=True) as db:
        tables={r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        if 'agent_identities' not in tables:
            assert 'agent_identity_catalog' not in tables
            return 0,[]
        revision=db.execute('SELECT revision FROM agent_identity_catalog WHERE slot=1').fetchone()
        assert revision is not None
        return revision[0],db.execute('SELECT id,label,username,algorithm,fingerprint,deleted FROM agent_identities ORDER BY id').fetchall()

def exercise_agent():
    assert host is not None
    username=host['username'];original=sources();assert agent_catalogue()==(0,[])
    def discover(name):
        click('管理 SSH Agent 身份');click('读取本机 Agent 公钥')
        reveal('选择 Agent 公钥','focus');key('End','Tab')
        fill('身份名称',name);fill('登录用户名',username)
    discover('GUI_AGENT');endpoint_screenshot('discovery-cancel');click('完成')
    assert agent_catalogue()==(0,[]) and sources()==original and recent() is None
    assert 'fixture.agent_selected_signed_once' not in peer_log.read_text()
    discover('GUI_AGENT');click('保存所选 Agent 身份');wait(lambda:agent_catalogue()[0]==1,'agent_saved_once')
    rows=agent_catalogue()[1];assert len(rows)==1 and rows[0][1:4]==('GUI_AGENT',username,'ssh-ed25519') and rows[0][5]==0
    endpoint_screenshot('saved');fill('身份名称','GUI_AGENT_RENAMED');click('保存名称与用户名')
    wait(lambda:agent_catalogue()[0]==2,'agent_renamed_once')
    click('删除此 Agent 身份');click('取消删除');assert agent_catalogue()[0]==2
    click('完成');assert sources()==original and recent() is None
    assert 'fixture.agent_selected_signed_once' not in peer_log.read_text()
    if prefix=="ssh-agent-cli":
        from probe_cli_agent_flow import locked_request
        locked_request(sys.modules[__name__])
    click('管理保险库');fill('主密码','fixed-target-master');click('解锁保险库')
    wait(lambda:'workspace.vault_front_confirmed' in log.read_text(),'agent_unlock');click('关闭')
    if prefix=="ssh-agent-cli":
        from probe_cli_agent_flow import exercise
        exercise(sys.modules[__name__], original)
        return
    def choose():
        click(target);reveal('选择保存的SSH身份','focus');key('End','Tab')
        wait(lambda:'GUI_AGENT_RENAMED' in accessible('选择保存的SSH身份','description').stdout,'agent_exact_identity_selected')
    choose();click('取消');assert 'fixture.agent_selected_signed_once' not in peer_log.read_text()
    choose();click('连接');wait(lambda:'workspace.ssh_session_opened' in log.read_text(),'agent_registered')
    wait(lambda:'fixture.ssh_window_request.' in peer_log.read_text() and 'workspace.terminal_input_ready' in log.read_text(),'agent_pty_input_ready')
    assert peer_log.read_text().splitlines().count('fixture.agent_selected_signed_once')==1
    assert peer_log.read_text().splitlines().count('fixture.ssh_public_key_authenticated')==1
    assert 'fixture.ssh_password_authenticated' not in peer_log.read_text()
    wait(lambda:len((recent() or {}).get('entries',[]))==1,'agent_recent_after_registration')
    final_recent=recent();assert final_recent is not None and final_recent['entries'][0]['host']==host_id
    run(['/usr/bin/xdotool','mousemove','--window',xid,'450','210','click','1'])
    run(['/usr/bin/xdotool','type','--clearmodifiers','A']);key('Return')
    wait(lambda:'fixture.agent_input_A_confirmed' in peer_log.read_text(),'agent_input_line')
    endpoint_screenshot('terminal');saved=agent_catalogue()
    click('重新加载工作区');wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'agent_refresh')
    assert agent_catalogue()==saved and sources()==original
    assert peer_log.read_text().splitlines().count('fixture.agent_selected_signed_once')==1
    endpoint_screenshot('refreshed');close()
    wait(lambda:'fixture.ssh_authenticated_stream_exited' in peer_log.read_text(),'agent_native_exit')
    start(2);click('管理 SSH Agent 身份');reveal('GUI_AGENT_RENAMED');endpoint_screenshot('reopened');click('完成')
    click('最近连接');choose();click('取消');close()
    assert agent_catalogue()==saved and sources()==original and recent()==final_recent
    assert peer_log.read_text().splitlines().count('fixture.agent_selected_signed_once')==1
    assert 'workspace.ssh_session_opened' not in log.read_text()
    result={'version':1,'passed':True,'scope':'Linux real WebView, bounded two-key Agent socket and one selected Ed25519 authentication','discovery_cancel_zero_write_zero_sign':True,'explicit_save_and_single_revision_update':True,'delete_cancel_zero_write':True,'connection_cancel_zero_sign':True,'selected_key_signed_once_no_fallback':True,'terminal_registration_recent':True,'refresh_without_new_authentication':True,'independent_reopen_exact_saved_identity':True,'native_exit_confirmed':True,'sources_sha256':original}
    (evidence/(prefix+'-result.json')).write_text(json.dumps(result,indent=2)+'\n')
    print('workspace.agent_metadata_cancel_explicit_save_one_selected_signature_terminal_refresh_reopen_native_exit_confirmed')

def entrance_catalogue():
    with sqlite3.connect('file:'+str(database)+'?mode=ro',uri=True) as db:
        raw=db.execute('SELECT value FROM app_state WHERE key=?',[host_key]).fetchone()
        assert raw is not None
        return json.loads(raw[0])
def machine_graph():
    with sqlite3.connect('file:'+str(database)+'?mode=ro',uri=True) as db:
        raw=db.execute('SELECT value FROM app_state WHERE key=?',[graph_key]).fetchone()
        return json.loads(raw[0]) if raw else None
def typed(text,value):
    fill(text,value);wait(lambda:accessible(value).returncode==0,'typed_'+text)
def endpoint_screenshot(name):
    run(['/usr/bin/import','-window',xid,str(evidence/(prefix+'-'+name+'.png'))],timeout=5)
def exercise_route():
    assert host is not None
    expected_port=str(host['port'])
    baseline=sources(); original=entrance_catalogue(); original_graph=machine_graph()
    original_pins,original_pin_state=route_trust()
    click('管理保险库');fill('主密码','fixed-target-master');key('Tab','Return')
    wait(lambda:'workspace.vault_front_confirmed' in log.read_text(),'route_unlock');click('关闭')
    cli_route = None
    if prefix == 'ssh-agent-cli-route':
        import probe_cli_route_flow as cli_route
        cli_process = cli_route.begin(sys.modules[__name__], 'cancel-first-hop')
    else:
        click(target);click('连接')
    reveal('信任并连接此跳');endpoint_screenshot('cancel-prompt');click('取消')
    wait(lambda:'fixture.route.outer.connection_exited' in peer_log.read_text(),'route_cancel_stream_exit')
    if cli_route: cli_route.finished(sys.modules[__name__], cli_process, False)
    assert sources()==baseline and recent() is None
    assert '.authenticated' not in peer_log.read_text() and 'fixture.ssh_password_authenticated' not in peer_log.read_text()
    if cli_route:
        cli_process = cli_route.begin(sys.modules[__name__], 'connect')
    else:
        click(target);click('连接')
    for index in range(3):
        button='信任并连接此跳' if index<2 else '确认连接此跳'
        reveal(button);expected='第 '+str(index+1)+'/3 跳'
        receipt=accessible(button,'description');(evidence/(prefix+'-peer-'+str(index)+'-description-debug.json')).write_text(json.dumps({'exit':receipt.returncode,'stdout':receipt.stdout,'stderr':receipt.stderr},ensure_ascii=False))
        wait(lambda:expected in accessible(button,'description').stdout,'route_prompt_'+str(index))
        description=accessible(button,'description').stdout
        (evidence/(prefix+'-peer-'+str(index)+'-description.txt')).write_text(description)
        identity_label='GUI_ROUTE_AGENT' if prefix in {'ssh-agent-route','ssh-agent-cli-route'} and index>0 else 'GUI_IDENTITY'
        assert ('GUI_JUMP_OUTER','GUI_JUMP_MIDDLE','GUI_HOST')[index] in description and identity_label in description and 'SHA256:' in description
        endpoint_screenshot('peer-'+str(index));click(button)
    wait(lambda:'workspace.ssh_session_opened' in log.read_text(),'route_registered')
    if cli_route: cli_route.finished(sys.modules[__name__], cli_process, True)
    wait(lambda:'fixture.ssh_window_request.' in peer_log.read_text() and 'workspace.terminal_input_ready' in log.read_text(),'route_pty_input_ready')
    run(['/usr/bin/xdotool','mousemove','--window',xid,'450','210','click','1'])
    run(['/usr/bin/xdotool','type','--clearmodifiers','R']);key('Return')
    wait(lambda:'fixture.route_input_R_confirmed' in peer_log.read_text(),'route_input_line')
    time.sleep(.2) # Let the real renderer consume the echoed line before the screenshot.
    assert peer_log.read_text().splitlines().count('fixture.route.outer.authenticated')==1
    assert peer_log.read_text().splitlines().count('fixture.route.middle.authenticated')==1
    if prefix in {'ssh-agent-route','ssh-agent-cli-route'}:
        assert peer_log.read_text().splitlines().count('fixture.agent_selected_signed_once')==2
        assert peer_log.read_text().splitlines().count('fixture.route.middle.agent_authenticated')==1
        assert peer_log.read_text().splitlines().count('fixture.ssh_public_key_authenticated')==1
        assert 'fixture.ssh_password_authenticated' not in peer_log.read_text()
        assert 'fixture.ssh_password_auth_attempt_rejected' not in peer_log.read_text()
    else:assert peer_log.read_text().splitlines().count('fixture.ssh_password_authenticated')==1
    assert entrance_catalogue()==original and machine_graph()==original_graph
    endpoint_screenshot('terminal')
    click('重新加载工作区');wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'route_refresh')
    assert log.read_text().splitlines().count('workspace.ssh_session_opened')==1
    exited_before={name:peer_log.read_text().splitlines().count('fixture.route.'+name+'.connection_exited') for name in ('outer','middle')}
    close()
    wait(lambda:all(peer_log.read_text().splitlines().count('fixture.route.'+name+'.connection_exited')>count for name,count in exited_before.items()) and 'fixture.ssh_authenticated_stream_exited' in peer_log.read_text(),'route_parents_exited')
    after=sources();assert entrance_catalogue()==original and machine_graph()==original_graph
    pins,pin_state=route_trust()
    assert pins[:len(original_pins)]==original_pins and len(pins)==len(original_pins)+2
    assert pin_state[:2]==original_pin_state[:2] and pin_state[2]==original_pin_state[2]+2 and pin_state[3]==original_pin_state[3] and pin_state[4]!=original_pin_state[4]
    added=pins[len(original_pins):]
    expected_targets={(row['host'],row['port']) for row in original['hosts'] if row['label'].startswith('GUI_JUMP_')}
    assert {(row[4],row[5]) for row in added}==expected_targets and all(row[3]==0 and row[6] is not None for row in added)
    for index,label in enumerate(('GUI_JUMP_OUTER','GUI_JUMP_MIDDLE')):
        entrance=next(row for row in original['hosts'] if row['label']==label)
        pin=next(row for row in added if (row[4],row[5])==(entrance['host'],entrance['port']))
        words=pin[6].split();assert words[0]=='ssh-ed25519'
        fingerprint='SHA256:'+base64.b64encode(hashlib.sha256(base64.b64decode(words[1],validate=True)).digest()).decode().rstrip('=')
        peer_description=(evidence/(prefix+'-peer-'+str(index)+'-description.txt')).read_text()
        expected_address=str(entrance['username'])+'@'+str(entrance['host'])+':'+str(entrance['port'])
        assert fingerprint in peer_description and expected_address in peer_description
    for table,digest in baseline.items():
        if table not in {'known_hosts','known_hosts_state'}:assert after[table]==digest,(table,'route source changed')
    start(2);click('最近连接');click(target)
    wait(lambda:'GUI_HOST' in accessible('选择保存的SSH主机','description').stdout and expected_port in accessible('选择保存的SSH主机','description').stdout,'route_reopened_original_selection')
    selection=accessible('选择保存的SSH主机','description').stdout
    (evidence/(prefix+'-reopened-selection.txt')).write_text(selection)
    endpoint_screenshot('reopened');click('取消');assert sources()==after
    assert 'workspace.ssh_session_opened' not in log.read_text();close()
    if prefix in {'ssh-agent-route','ssh-agent-cli-route'}:assert peer_log.read_text().splitlines().count('fixture.agent_selected_signed_once')==2
    result={'version':1,'passed':True,'scope':'Linux real WebView, two nested exact-target forwarding peers and terminal leaf','cancel_first_peer_zero_writes_and_zero_auth':True,'three_explicit_actual_peer_approvals':True,'two_first_pins_one_existing_pin':True,'only_leaf_pty_shell':True,'one_authentication_per_hop':True,'root_session_refresh_without_redial':True,'source_invariance_except_two_first_pins':True,'recent_reopen_original_entry':True,'normal_shutdown_parent_native_exit_confirmed':True}
    if prefix in {'ssh-agent-route','ssh-agent-cli-route'}:result['two_selected_agent_signatures_no_password_fallback']=True
    (evidence/(prefix+'-result.json')).write_text(json.dumps(result,indent=2)+'\n')
    print('workspace.route_two_jumps_actual_peer_confirm_cancel_zero_auth_first_pins_refresh_reopen_native_exit_confirmed')

def exercise_temporary():
    assert host is not None
    original=entrance_catalogue();graph=machine_graph();assert graph is not None
    address=f"{host['username']}@{host['host']}:{host['port']}"
    quick_address(address);click('临时连接');reveal('确认临时SSH目标');click('取消')
    assert entrance_catalogue()==original and machine_graph()==graph and sources()==initial and recent() is None
    assert 'fixture.ssh_password_authenticated' not in peer_log.read_text()
    click('管理保险库');fill('主密码','fixed-target-master');key('Tab','Return')
    wait(lambda:'workspace.vault_front_confirmed' in log.read_text(),'temporary_unlock');click('关闭')
    quick_address(address);click('临时连接');click('连接')
    wait(lambda:log.read_text().splitlines().count('workspace.ssh_session_opened')==1,'temporary_first_registered')
    reveal('保存此主机');assert entrance_catalogue()==original and machine_graph()==graph and recent() is None
    click('暂不保存');assert entrance_catalogue()==original and machine_graph()==graph and recent() is None
    assert 'workspace.ssh_session_closed' not in log.read_text()
    quick_address(address);click('临时连接');click('连接')
    wait(lambda:log.read_text().splitlines().count('workspace.ssh_session_opened')==2,'temporary_second_registered')
    typed('临时连接保存名称','Temporary Saved');click('保存此主机')
    wait(lambda:entrance_catalogue()['revision']==original['revision']+1,'temporary_saved_revision')
    final=entrance_catalogue();old_ids={row['id'] for row in original['hosts']}
    added=[row for row in final['hosts'] if row['id'] not in old_ids];assert len(added)==1
    saved=added[0];assert saved['label']=='Temporary Saved' and saved['host']==host['host'] and saved['port']==host['port'] and saved['username']==host['username']
    assert [row for row in final['hosts'] if row['id'] in old_ids]==original['hosts']
    wait(lambda:len((recent() or {}).get('entries',[]))==1,'temporary_saved_recent')
    final_recent=recent();assert final_recent is not None and final_recent['entries'][0]['host']==saved['id']
    assert sources()==initial and peer_log.read_text().splitlines().count('fixture.ssh_password_authenticated')==2
    run(['/usr/bin/import','-window',xid,str(evidence/'temporary-ssh-visible.png')],timeout=5)
    saved_target=f"选择主机 {saved['label']} · {saved['username']}@{saved['host']}:{saved['port']}"
    click('最近连接');click(saved_target);click('取消')
    click('重新加载工作区');wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'temporary_refresh')
    assert entrance_catalogue()==final and recent()==final_recent and sources()==initial
    assert peer_log.read_text().splitlines().count('fixture.ssh_password_authenticated')==2
    close();start(2);click('最近连接');click(saved_target)
    description=accessible('选择保存的SSH主机','description');assert description.returncode==0 and 'Temporary Saved' in description.stdout
    click('取消');assert entrance_catalogue()==final and recent()==final_recent and sources()==initial
    assert 'workspace.ssh_session_opened' not in log.read_text()
    run(['/usr/bin/import','-window',xid,str(evidence/'temporary-ssh-reopened.png')],timeout=5);close()
    result={'version':1,'passed':True,'scope':'Linux real WebView with fixed russh peer; temporary cancel, successful skip-save and explicit save','cancel_zero_write':True,'temporary_not_persisted':True,'skip_save_keeps_session':True,'registered_only_single_revision_save':True,'original_records_preserved':True,'saved_recent_binding':True,'two_explicit_authentications':True,'credential_trust_organization_hashes_unchanged':True,'refresh_reopen':True,'native_exit_confirmed':True}
    (evidence/'temporary-ssh-result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('workspace.temporary_target_cancel_connect_skip_save_explicit_saved_recent_refresh_reopen_native_exit_confirmed')

def exercise_jumps():
    assert host is not None
    seeded=entrance_catalogue();click('管理 SSH 主机')
    reveal('选择 SSH 主机或添加主机','focus');key('Home')
    typed('显示名称','Jump fixture');typed('主机地址','jump.invalid');typed('端口','2202');typed('用户名','fixed-user');click('添加')
    wait(lambda:entrance_catalogue()['revision']==seeded['revision']+1,'jump_fixture_second_profile')
    click('完成')
    original=entrance_catalogue();before=machine_graph();assert before is not None
    assert len([p for p in before['profiles'] if not p['metadata']['deleted']])==2
    leaf=next(p for p in before['profiles'] if p['endpoint']==host_id and not p['metadata']['deleted'])
    via=next(p for p in before['profiles'] if p['metadata']['id']!=leaf['metadata']['id'] and not p['metadata']['deleted'])
    def select_jump(index):
        # Actual browser keys commit the option and leave no native menu overlay.
        assert index in (0,1)
        reveal('配置SSH跳板','focus');key('Home')
        if index:key('Down')
        key('Tab')
    click('SSH 配置');select_jump(1);typed('配置名称','Cancelled Jump');click('完成SSH配置')
    assert machine_graph()==before and entrance_catalogue()==original and sources()==initial and recent() is None
    click('SSH 配置');select_jump(1);typed('配置名称','Routed Saved');endpoint_screenshot('editing');click('保存SSH配置')
    wait(lambda:entrance_catalogue()['revision']==original['revision']+1,'jump_saved')
    routed=machine_graph();assert routed is not None and routed['version']==2
    saved=next(p for p in routed['profiles'] if p['metadata']['id']==leaf['metadata']['id'])
    assert saved['name']=='Routed Saved' and saved['via']==via['metadata']['id'] and saved['username']==leaf['username'] and saved['identity']==leaf['identity']
    assert routed['hosts']==before['hosts'] and routed['endpoints']==before['endpoints']
    assert entrance_catalogue()['hosts']==original['hosts'] and sources()==initial and recent() is None
    endpoint_screenshot('saved');click('完成SSH配置');click('重新加载工作区')
    wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'jump_refresh')
    click('SSH 配置');reveal('Routed Saved');assert machine_graph()==routed;endpoint_screenshot('refreshed');click('完成SSH配置')
    close();start(2);click('SSH 配置');reveal('Routed Saved');assert machine_graph()==routed and sources()==initial
    endpoint_screenshot('reopened');select_jump(0);click('保存SSH配置')
    wait(lambda:entrance_catalogue()['revision']==original['revision']+2,'jump_detached')
    detached=machine_graph();assert detached is not None
    final=next(p for p in detached['profiles'] if p['metadata']['id']==leaf['metadata']['id'])
    assert final['via'] is None and final['name']=='Routed Saved' and final['username']==leaf['username'] and final['identity']==leaf['identity']
    assert detached['hosts']==before['hosts'] and detached['endpoints']==before['endpoints'] and entrance_catalogue()['hosts']==original['hosts']
    assert sources()==initial and recent() is None
    endpoint_screenshot('detached');click('完成SSH配置');close()
    assert 'workspace.ssh_session_opened' not in log.read_text() and 'fixture.ssh_password_authenticated' not in peer_log.read_text()
    (evidence/(prefix+'-result.json')).write_text(json.dumps({'version':1,'passed':True,'scope':'Linux real WebView jump metadata only; production jump transport unavailable','cancel_zero_write':True,'one_revision_complete_save':True,'profile_reference_exact':True,'machine_endpoint_identity_credential_trust_organization_unchanged':True,'refresh_reopen':True,'explicit_clear_one_revision':True,'zero_authentication':True,'native_exit_confirmed':True,'sources_sha256':initial},indent=2)+'\n')
    print('workspace.jump_profile_cancel_atomic_save_refresh_reopen_detach_zero_auth_native_exit_confirmed')
def exercise_profiles():
    assert host is not None
    original=entrance_catalogue();before=machine_graph();assert before is not None
    profile=next(p for p in before['profiles'] if p['endpoint']==host_id and not p['metadata']['deleted'])
    identity_id=next(line.removeprefix('fixture.ssh_gui_identity_id.') for line in peer_log.read_text().splitlines() if line.startswith('fixture.ssh_gui_identity_id.'))
    click('SSH 配置');typed('配置名称','Cancelled');click('完成SSH配置')
    assert machine_graph()==before and entrance_catalogue()==original and sources()==initial
    click('SSH 配置');typed('配置名称','Independent SSH');typed('配置登录用户名',host['username'])
    reveal('配置默认身份','focus');key('End')
    endpoint_screenshot('editing');click('保存SSH配置')
    wait(lambda:entrance_catalogue()['revision']==original['revision']+1,'profile_saved')
    final=machine_graph();assert final is not None
    saved=next(p for p in final['profiles'] if p['metadata']['id']==profile['metadata']['id'])
    assert saved['name']=='Independent SSH' and saved['username']==host['username'] and saved['identity']==identity_id
    assert final['hosts']==before['hosts'] and final['endpoints']==before['endpoints']
    assert entrance_catalogue()['hosts']==original['hosts'] and sources()==initial and recent() is None
    endpoint_screenshot('saved');click('完成SSH配置');click(target)
    wait(lambda:'GUI_IDENTITY' in accessible('选择保存的SSH身份','description').stdout,'default_preselected')
    endpoint_screenshot('default');click('取消');assert sources()==initial and machine_graph()==final
    click('重新加载工作区');wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'refresh')
    click('SSH 配置');reveal('Independent SSH');assert machine_graph()==final;endpoint_screenshot('refreshed');click('完成SSH配置')
    close();start(2);click('SSH 配置');reveal('Independent SSH');assert machine_graph()==final and sources()==initial
    endpoint_screenshot('reopened');click('完成SSH配置');click(target)
    wait(lambda:'GUI_IDENTITY' in accessible('选择保存的SSH身份','description').stdout,'reopened_default');click('取消');close()
    assert 'workspace.ssh_session_opened' not in log.read_text() and recent() is None
    (evidence/(prefix+'-result.json')).write_text(json.dumps({'version':1,'passed':True,'scope':'Linux real WebView metadata only; no new authentication','independent_profile_name':True,'explicit_identity_reference':True,'default_identity_preselected':True,'cancel_before_save_zero_write':True,'single_revision_save':True,'host_endpoint_and_credential_trust_organization_unchanged':True,'refresh_reopen':True,'native_exit_confirmed':True,'sources_sha256':initial},indent=2)+'\n')
    print('workspace.ssh_profile_independent_name_default_identity_cancel_refresh_reopen_zero_auth_native_exit_confirmed')
def exercise_endpoints():
    assert host is not None
    original=entrance_catalogue();revision=original['revision'];assert machine_graph() is None
    click('管理 SSH 主机');reveal('启用多入口管理');assert machine_graph() is None
    click('完成');assert entrance_catalogue()==original and machine_graph() is None
    click('管理 SSH 主机');click('启用多入口管理')
    wait(lambda:entrance_catalogue()['revision']==revision+1,'migrated')
    migrated=machine_graph();assert migrated is not None
    primary=next(e for e in migrated['endpoints'] if e['metadata']['id']==host_id)
    machine_id=primary['host'];endpoint_screenshot('migrated')
    reveal('选择 SSH 主机或添加主机','focus');key('Home')
    typed('显示名称','Alternate SSH');typed('主机地址','second.invalid');fill('端口','2202');typed('用户名','other-user')
    machines=[m for m in migrated['hosts'] if not m['metadata']['deleted']]
    index=next(i for i,m in enumerate(machines,1) if m['metadata']['id']==machine_id)
    reveal('选择入口归属机器','select='+str(index));click('添加')
    wait(lambda:entrance_catalogue()['revision']==revision+2,'attached')
    attached=machine_graph();assert attached is not None
    secondary=next(e for e in attached['endpoints'] if e['name']=='Alternate SSH')
    assert secondary['host']==machine_id and secondary['metadata']['id']!=host_id
    assert secondary['address']=='second.invalid' and secondary['port']==2202
    assert next(r for r in entrance_catalogue()['hosts'] if r['id']==host_id)==host
    endpoint_screenshot('attached')
    typed('机器名称','Development machine');click('保存机器名称')
    wait(lambda:entrance_catalogue()['revision']==revision+3,'renamed')
    renamed=machine_graph();assert renamed is not None
    assert next(m for m in renamed['hosts'] if m['metadata']['id']==machine_id)['name']=='Development machine'
    assert next(r for r in entrance_catalogue()['hosts'] if r['id']==host_id)==host
    reveal('选择 SSH 主机或添加主机','focus');key('End');click('删除');click('确认删除主机')
    wait(lambda:entrance_catalogue()['revision']==revision+4,'removed')
    final=machine_graph();assert final is not None
    assert next(e for e in final['endpoints'] if e['metadata']['id']==secondary['metadata']['id'])['metadata']['deleted']
    assert not next(m for m in final['hosts'] if m['metadata']['id']==machine_id)['metadata']['deleted']
    assert sources()==initial and recent() is None
    click('完成');reveal('Development machine');click(target)
    wait(lambda:accessible('选择保存的SSH主机','description').returncode==0,'original_entrance')
    description=accessible('选择保存的SSH主机','description');assert 'GUI_HOST' in description.stdout and host['host'] in description.stdout
    click('取消');click('重新加载工作区')
    wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'refresh')
    reveal('Development machine');assert machine_graph()==final and sources()==initial
    endpoint_screenshot('refreshed');close();start(2);reveal('Development machine')
    click('管理 SSH 主机');reveal('机器名称');assert machine_graph()==final and sources()==initial
    assert entrance_catalogue()['revision']==revision+4
    endpoint_screenshot('reopened');click('完成');close()
    assert 'workspace.ssh_session_opened' not in log.read_text() and recent() is None
    (evidence/(prefix+'-result.json')).write_text(json.dumps({'status':'passed','platform':'linux','scope':'metadata-only','legacy_entrance_id_preserved':True,'explicit_migration':True,'machine_id':machine_id,'distinct_entrance_ids':[host_id,secondary['metadata']['id']],'independent_name':True,'remaining_machine_preserved':True,'refresh_reopen':True,'sources_sha256':initial,'authentication_count':0,'native_exit_confirmed':True},indent=2)+'\n')
    print('workspace.host_endpoint_explicit_migration_attach_rename_tombstone_refresh_reopen_zero_auth_native_exit_confirmed')
def exercise_fuzzy():
    assert host is not None
    fill('搜索主机树','guh');wait(lambda:accessible('guh').returncode==0,'fuzzy_query')
    click(target);wait(lambda:accessible('选择保存的SSH主机','description').returncode==0,'fuzzy_saved_form')
    assert host['label'] in accessible('选择保存的SSH主机','description').stdout
    click('取消');assert sources()==initial and recent() is None
    fill('搜索主机树','zzzzzz');reveal('没有匹配的主机')
    fill('搜索主机树','user 127');wait(lambda:accessible('user 127').returncode==0,'multiple_terms');click(target);click('取消')
    assert sources()==initial and recent() is None and 'workspace.ssh_session_opened' not in log.read_text()
    run(['/usr/bin/import','-window',xid,str(evidence/'fuzzy-search-visible.png')],timeout=5);close()
    (evidence/'fuzzy-search-result.json').write_text(json.dumps({'version':1,'passed':True,'scope':'Linux real WebView metadata search only','noncontiguous_label':True,'multiple_terms':True,'empty_result':True,'original_saved_id_preselection':True,'zero_writes_or_launch':True,'native_exit_confirmed':True},indent=2)+'\n')
    print('workspace.fuzzy_metadata_multiterm_original_target_cancel_zero_write_native_exit_confirmed')
def host_catalogue():
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        for (raw,) in db.execute('SELECT value FROM app_state'):
            doc=json.loads(raw)
            if isinstance(doc,dict) and any(row['id']==host_id for row in doc.get('hosts',[])):return doc
    raise RuntimeError('quick.host_catalogue_missing')
def quick_address(address,label=''):
    click('快速连接');reveal('输入地址后核对目标，再保存或使用已有主机。')
    fill('快速连接地址',address);wait(lambda:accessible(address).returncode==0,'quick_address_bytes')
    if label:
        fill('快速连接名称',label);wait(lambda:accessible(label).returncode==0,'quick_label_bytes')
    click('核对地址')
def exercise_quick():
    assert host is not None
    original=host_catalogue();original_revision=int(original['revision'])
    quick_address('user:password@invalid.example');reveal('请输入 user@host:port；IPv6 使用方括号。不支持密码、路径或其他协议。');click('取消快速连接')
    quick_address('new-user@[::1]:2442','Quick IPv6');reveal('保存并继续');click('取消快速连接');assert sources()==initial and recent() is None
    address=f"{host['username']}@{host['host']}:{host['port']}"
    quick_address(address);click('使用此主机');wait(lambda:accessible('选择保存的SSH主机','description').returncode==0,'quick_reuse')
    assert host['label'] in accessible('选择保存的SSH主机','description').stdout
    click('取消');assert sources()==initial
    quick_address('ssh://new-user@[::1]:2442','Quick IPv6');click('保存并继续')
    wait(lambda:int(host_catalogue()['revision'])==original_revision+1,'quick_saved_once')
    wait(lambda:accessible('选择保存的SSH主机','description').returncode==0,'quick_new_form')
    assert 'Quick IPv6' in accessible('选择保存的SSH主机','description').stdout
    click('取消');created=host_catalogue();new=next(row for row in created['hosts'] if row['id'] not in {old['id'] for old in original['hosts']})
    assert new['host']=='::1' and new['port']==2442 and new['username']=='new-user'
    after=sources();assert all(value==after[table] for table,value in initial.items() if table!='app_state')
    assert recent() is None and 'workspace.ssh_session_opened' not in log.read_text()
    run(['/usr/bin/import','-window',xid,str(evidence/'quick-connect-saved.png')],timeout=5)
    click('重新加载工作区');wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'quick_refresh');assert host_catalogue()==created
    close();start(2);quick_address('new-user@[::1]:2442');click('使用此主机')
    wait(lambda:accessible('选择保存的SSH主机','description').returncode==0,'quick_reopen')
    assert 'Quick IPv6' in accessible('选择保存的SSH主机','description').stdout
    click('取消');assert host_catalogue()==created and sources()==after and recent() is None
    assert 'workspace.ssh_session_opened' not in log.read_text()
    run(['/usr/bin/import','-window',xid,str(evidence/'quick-connect-reopened.png')],timeout=5);close()
    result={'version':1,'passed':True,'scope':'Linux real WebView; metadata preview and original saved-host connection form, no authentication run','invalid_rejection':True,'cancel_zero_write':True,'saved_target_reuse_zero_write':True,'explicit_single_revision_create':True,'ipv6_saved':True,'original_connection_form_preselection':True,'credential_and_trust_hashes_unchanged':True,'zero_authentication_or_recent_records':True,'refresh_reopen':True,'native_exit_confirmed':True}
    (evidence/'quick-connect-result.json').write_text(json.dumps(result,indent=2)+'\n');print('workspace.quick_preview_cancel_reuse_saved_ipv6_original_form_refresh_reopen_zero_start_native_exit_confirmed')
try:
    peer_stream=peer_log.open('wb')
    peer=subprocess.Popen(['target/debug/xtask','serve-workspace-ssh-fixture',str(directory),'agent-route' if prefix in {'ssh-agent-route','ssh-agent-cli-route'} else 'agent' if prefix in {'ssh-agent','ssh-agent-cli'} else 'route' if prefix=='ssh-route' else 'normal'],stdout=peer_stream,stderr=subprocess.STDOUT,start_new_session=True)
    wait(lambda:"fixture.ssh_gui_seeded_listening" in peer_log.read_text(),"peer",timeout=30)
    host_id=next(line.removeprefix('fixture.ssh_gui_host_id.') for line in peer_log.read_text().splitlines() if line.startswith('fixture.ssh_gui_host_id.'))
    with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as db:
        host=None
        for (raw,) in db.execute('SELECT value FROM app_state'):
            doc=json.loads(raw)
            if isinstance(doc,dict):
                host=next((r for r in doc.get('hosts',[]) if r.get('id')==host_id),None)
                if host:break
    assert host is not None
    target=f"选择主机 {host['label']} · {host['username']}@{host['host']}:{host['port']}"
    if prefix in {'host-endpoint','ssh-profile','temporary-ssh','ssh-jump-config','ssh-route','ssh-agent-route','ssh-agent-cli-route'}:
        # This database belongs to this probe's marked private fixture. Model a legacy-only source.
        with sqlite3.connect(database) as db:
            for key_name,raw in db.execute('SELECT key,value FROM app_state'):
                try:doc=json.loads(raw)
                except (ValueError,TypeError):continue
                if isinstance(doc,dict) and 'legacyRevision' in doc and 'profiles' in doc:graph_key=key_name
                if isinstance(doc,dict) and any(r.get('id')==host_id for r in doc.get('hosts',[])):host_key=key_name
            assert host_key and graph_key
            if prefix=='host-endpoint':db.execute('DELETE FROM app_state WHERE key=?',[graph_key])
            if prefix=='temporary-ssh':
                # Keep the peer's real username in host; make the saved entrance a different target.
                # The probe owns this marked private fixture, and updates both projections atomically.
                legacy=json.loads(db.execute('SELECT value FROM app_state WHERE key=?',[host_key]).fetchone()[0])
                graph=json.loads(db.execute('SELECT value FROM app_state WHERE key=?',[graph_key]).fetchone()[0])
                saved_rows=[row for row in legacy['hosts'] if row['id']==host_id]
                profiles=[row for row in graph['profiles'] if row['endpoint']==host_id]
                assert len(saved_rows)==len(profiles)==1
                saved_rows[0]['username']=profiles[0]['username']='saved-other-user'
                db.execute('UPDATE app_state SET value=? WHERE key=?',[json.dumps(legacy,separators=(',',':')),host_key])
                db.execute('UPDATE app_state SET value=? WHERE key=?',[json.dumps(graph,separators=(',',':')),graph_key])
    initial=sources();assert recent() is None
    start(1)
    if prefix in {"ssh-agent","ssh-agent-cli"}:
        exercise_agent();sys.exit(0)
    if prefix in {"ssh-route", "ssh-agent-route", "ssh-agent-cli-route"}:
        exercise_route();sys.exit(0)
    if prefix=="temporary-ssh":
        exercise_temporary();sys.exit(0)
    if prefix=="ssh-jump-config":
        exercise_jumps();sys.exit(0)
    if prefix=="ssh-profile":
        exercise_profiles();sys.exit(0)
    if prefix=="host-endpoint":
        exercise_endpoints();sys.exit(0)
    if prefix=="fuzzy-search":
        exercise_fuzzy();sys.exit(0)
    if prefix=="quick-connect":
        exercise_quick();sys.exit(0)
    click('最近连接');reveal('尚无成功的 SSH 连接')
    click('树视图');click(target);wait(lambda:accessible('选择保存的SSH主机','description').returncode==0,'initial_form');click('取消');assert recent() is None
    click('管理保险库');fill('主密码','fixed-target-master');click('解锁保险库');wait(lambda:'workspace.vault_front_confirmed' in log.read_text(),'unlock');click('关闭')
    click(target);click('连接')
    wait(lambda:'workspace.ssh_session_opened' in log.read_text(),'registered')
    wait(lambda:len((recent() or {}).get('entries',[]))==1,'recorded')
    final=recent();assert final is not None and final['entries'][0]['host']==host_id
    assert sources()==initial
    click('最近连接');click(target)
    description=accessible('选择保存的SSH主机','description');assert description.returncode==0 and host['label'] in description.stdout
    click('取消');assert recent()==final
    assert peer_log.read_text().splitlines().count('fixture.ssh_password_authenticated')==1
    run(['/usr/bin/import','-window',xid,str(evidence/'recent-host-visible.png')],timeout=5)
    click('重新加载工作区');wait(lambda:log.read_text().splitlines().count('workspace.profiles_listed')==2,'refresh');assert recent()==final
    close();start(2);click('最近连接');click(target);click('取消');assert recent()==final and sources()==initial
    assert 'workspace.ssh_session_opened' not in log.read_text()
    close()
    result={'version':1,'passed':True,'scope':'Linux real WebView with fixed russh peer','cancel_zero_write':True,'successful_registration_only':True,'recent_saved_target_preselection':True,'one_authentication':True,'refresh_reopen':True,'native_exit_confirmed':True,'source_table_hashes':initial,'sources_unchanged':True}
    (evidence/'recent-host-result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('workspace.recent_saved_host_cancel_register_preselect_refresh_reopen_native_exit_confirmed')
finally:
    for process, output in cli_clients:
        if process.poll() is None:
            process.terminate()
            try:process.wait(timeout=3)
            except subprocess.TimeoutExpired:process.kill();process.wait(timeout=3)
        reclaim(process.pid);output.close()
    if app is not None:
        if app.poll() is None:
            run(['/usr/bin/import','-window',xid,str(evidence/f'{prefix}-failure.png')],check=False,timeout=5)
            app.terminate()
            try:app.wait(timeout=4)
            except subprocess.TimeoutExpired:app.kill();app.wait(timeout=3)
        reclaim(app.pid)
    if peer is not None:
        if peer.poll() is None:
            peer.terminate()
            try:peer.wait(timeout=3)
            except subprocess.TimeoutExpired:peer.kill();peer.wait(timeout=3)
        reclaim(peer.pid)
    if stream is not None:stream.close()
    if peer_stream is not None:peer_stream.close()
