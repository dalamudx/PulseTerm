#!/usr/bin/env python3
"""Production cold CLI saved-ID stimulus; unlock never substitutes for approval."""
import ctypes
from collections.abc import Callable
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time

assert ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) == 0
root = Path(tempfile.mkdtemp(prefix='c.', dir=os.environ['TMPDIR']))
assert root.is_relative_to(Path('/data/temp'))
root.chmod(0o700)
evidence = Path('target/probes')
env = os.environ.copy()
password_interactive_mode = os.environ.get("PULSE_CLI_PASSWORD_INTERACTIVE") == "1"
agent_interactive_mode = os.environ.get("PULSE_CLI_AGENT_INTERACTIVE") == "1"
key_interactive_mode = os.environ.get("PULSE_CLI_KEY_INTERACTIVE") == "1"
interactive_mode = password_interactive_mode or key_interactive_mode or agent_interactive_mode or os.environ.get("PULSE_CLI_INTERACTIVE") == "1"
binaries = Path(os.environ.get("CARGO_TARGET_DIR", "target")) / "debug"
route_password_interactive_mode = os.environ.get("PULSE_ROUTE_PASSWORD_INTERACTIVE") == "1"
route_agent_interactive_mode = os.environ.get("PULSE_ROUTE_AGENT_INTERACTIVE") == "1"
route_key_interactive_mode = os.environ.get("PULSE_ROUTE_KEY_INTERACTIVE") == "1"
route_interactive_mode = route_password_interactive_mode or route_key_interactive_mode or route_agent_interactive_mode or os.environ.get("PULSE_ROUTE_INTERACTIVE") == "1"
route_mode = route_interactive_mode or os.environ.get("PULSE_COLD_SSH_ROUTE") == "1"
agent_mode = route_agent_interactive_mode or (route_mode and not route_interactive_mode) or os.environ.get("PULSE_COLD_SSH_AGENT") == "1"
multiwindow_mode = os.environ.get("PULSE_SSH_MULTIWINDOW") == "1"
alias_mode = os.environ.get("PULSE_SAVED_ALIAS") == "1"
alias_case = os.environ.get("PULSE_SAVED_ALIAS_CASE", "")
alias_warm = os.environ.get("PULSE_SAVED_ALIAS_WARM") == "1"
scope_mode = os.environ.get("PULSE_SSH_SCOPE_PROBE") == "1"
if scope_mode and (route_mode or agent_mode or interactive_mode or multiwindow_mode or alias_mode):
    raise RuntimeError("SSH settings fixture cannot mix authentication modes")
if alias_case not in {"", "ambiguous", "missing-default"}: raise RuntimeError("invalid owned alias case")
close_cycles = int(os.environ.get('PULSE_COLD_CLOSE_CYCLES', '0'))
close_state = os.environ.get('PULSE_COLD_CLOSE_STATE', 'locked')
close_phase = os.environ.get('PULSE_COLD_CLOSE_PHASE', 'settled')
if close_phase not in {'settled', 'cancel-unobserved', 'pending-dialog'}:
    raise RuntimeError('invalid owned close phase')
if close_phase != 'settled' and not close_cycles:
    raise RuntimeError('close phase requires bounded close cycles')
if not 0 <= close_cycles <= 32 or close_state not in {'locked', 'unlocked'}:
    raise RuntimeError('invalid owned close diagnostic')
if close_cycles and (alias_case or alias_warm): raise RuntimeError('close diagnostic cannot run alias cases')
prefix = ('cli-saved-alias-route' if route_mode else 'cli-saved-alias-agent' if agent_mode else 'cli-saved-alias-password') + ('-'+alias_case if alias_case else '-warm' if alias_warm else '-cold') if alias_mode else 'cli-cold-agent-route' if route_mode else 'cli-cold-agent' if agent_mode else 'cli-cold-ssh'
if multiwindow_mode:
    if alias_mode or close_cycles: raise RuntimeError("multiwindow fixture cannot run alias/close diagnostics")
    prefix = "cli-multiwindow-ssh-" + ("route" if route_mode else "agent" if agent_mode else "password")
if interactive_mode:
    if route_mode or agent_mode or multiwindow_mode or alias_mode or close_cycles:
        raise RuntimeError("interactive fixture cannot mix authentication modes")
    prefix = "cli-password-interactive" if password_interactive_mode else "cli-agent-interactive" if agent_interactive_mode else "cli-key-interactive" if key_interactive_mode else "cli-interactive"
if route_interactive_mode:
    if interactive_mode or multiwindow_mode or alias_mode or close_cycles:
        raise RuntimeError("route MFA fixture cannot mix other diagnostic modes")
    prefix = "route-password-interactive" if route_password_interactive_mode else "route-agent-interactive" if route_agent_interactive_mode else "route-key-interactive" if route_key_interactive_mode else "route-interactive"
if scope_mode: prefix = "workspace-ssh-settings"
for name, sub in [('HOME','home'),('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime')]:
    path = root / sub
    path.mkdir(mode=0o700)
    env[name] = str(path)
bundle = root / 'bundle'
bundle.mkdir(mode=0o700)
for name in ['pulse','pulseterm-workspace']:
    shutil.copy2(binaries / name, bundle / name)
pulse = bundle / 'pulse'
gui = (bundle / 'pulseterm-workspace').resolve()
runtime = Path(env['XDG_RUNTIME_DIR']) / 'pulseterm-workspace-cli'
assert len(os.fsencode(runtime / 'control.sock')) < 108
directory = Path(env['XDG_DATA_HOME']) / 'dev.pulseterm.workspace-preview'
database = directory / 'state.db'
peer_log = evidence / (prefix+'-peer.log')
peer_output = peer_log.open('wb')
peer = None
seed_root = None
client = None
xid = None
diagnostic_children = []

def run(args, check=True, timeout=3):
    return subprocess.run(list(map(str,args)), env=env, capture_output=True, text=True, check=check, timeout=timeout)

def wait(predicate, tag, timeout: float = 8):
    deadline = time.monotonic() + timeout
    while True:
        result = predicate()
        if result:
            return result
        assert time.monotonic() < deadline, 'cold.ssh_timeout.' + tag
        time.sleep(.05)

def xdo(*args):
    return run(['/usr/bin/xdotool',*args]).stdout.strip()

def guis():
    owned = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if (proc / 'exe').resolve() == gui and ('XDG_RUNTIME_DIR='+env['XDG_RUNTIME_DIR']).encode() in (proc / 'environ').read_bytes().split(b'\0'):
                owned.append(int(proc.name))
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            pass
    return owned

def accessible(name, action=None):
    args = [os.environ.get('PULSE_NATIVE_HELPER', 'target/probes/x11-status'),str(guis()[0]),name]
    if action:
        args.append(action)
    return run(args,check=False,timeout=2)

def click(name):
    selected = wait(lambda: (value if (value:=accessible(name,'focus')).returncode == 0 else None), 'control.'+name)
    assert selected.returncode == 0
    xdo('key','--clearmodifiers','space')

def shot(tag):
    run(['/usr/bin/import','-limit','thread','1','-window',xid,evidence / (prefix+'-'+tag+'.png')], timeout=3)

def event_count(name):
    return peer_log.read_text().splitlines().count(name)

def auth_count():
    return event_count('fixture.ssh_interactive_authenticated' if interactive_mode else 'fixture.ssh_public_key_authenticated' if agent_mode else 'fixture.ssh_password_authenticated')

def signature_count():
    return event_count('fixture.agent_selected_signed_once')

def quiet():
    return signature_count() == 0 and all(event_count(name) == 0 for name in ("fixture.ssh_interactive_authenticated", "fixture.ssh_password_authenticated", "fixture.ssh_public_key_authenticated", "fixture.route.outer.authenticated", "fixture.route.middle.authenticated"))

def pending_is_quiet(before):
    if not agent_interactive_mode:
        return quiet()
    return (auth_count(), signature_count()) == before and all(event_count(name) == 0 for name in (
        'fixture.agent_unexpected_sign_request', 'fixture.ssh_password_authenticated', 'fixture.ssh_public_key_authenticated'))

def source_hashes():
    with sqlite3.connect('file:'+str(database)+'?mode=ro', uri=True) as db:
        result = {}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            quoted = table.replace('"','""')
            sql = 'SELECT * FROM "'+quoted+'"'
            if table == 'app_state':
                rows = db.execute(sql+' WHERE key != ?', ('catalogue.recent-ssh-hosts.v1',)).fetchall()
            else:
                rows = db.execute(sql).fetchall()
            result[table] = hashlib.sha256(repr(sorted(rows,key=repr)).encode()).hexdigest()
        return result

def route_pins():
    with sqlite3.connect('file:'+str(database)+'?mode=ro',uri=True) as db:
        return (db.execute('SELECT id,created_at,hlc,deleted,host,port,key,ordinal FROM known_hosts ORDER BY ordinal').fetchall(), db.execute('SELECT singleton,schema_version,revision,node_id,clock FROM known_hosts_state').fetchone())

def route_prompt(index):
    button = '信任并连接此跳' if index < 2 else '确认连接此跳'
    expected = '第 '+str(index+1)+'/3 跳'
    wait(lambda:expected in accessible(button,'description').stdout,'route_prompt_'+str(index))
    description = accessible(button,'description').stdout
    label = ('GUI_JUMP_OUTER','GUI_JUMP_MIDDLE','GUI_HOST')[index]
    identity_label = 'GUI_IDENTITY' if index == 0 else 'GUI_ROUTE_AGENT'
    row = route_targets[label]
    fingerprint = descriptor('fixture.route.'+('outer','middle')[index]+'.fingerprint.') if index < 2 else descriptor('fixture.ssh_gui_fingerprint.')
    address = str(row['username'])+'@'+row['host']+':'+str(row['port'])
    (evidence/(prefix+'-peer-'+str(index)+'-description.txt')).write_text(description)
    assert label in description and identity_label in description and fingerprint in description and address in description
    shot('peer-'+str(index))
    return button

def verify_route_pins():
    assert route_mode and original_pins is not None and original_pin_state is not None
    pins,state = route_pins()
    assert pins[:len(original_pins)] == original_pins and len(pins) == len(original_pins)+2
    assert state[:2] == original_pin_state[:2] and state[2] == original_pin_state[2]+2 and state[3] == original_pin_state[3] and state[4] != original_pin_state[4]
    added = pins[len(original_pins):]
    targets = {(route_targets[label]['host'],route_targets[label]['port']) for label in ('GUI_JUMP_OUTER','GUI_JUMP_MIDDLE')}
    assert {(r[4],r[5]) for r in added} == targets and all(r[3] == 0 for r in added)
    for index,label in enumerate(('GUI_JUMP_OUTER','GUI_JUMP_MIDDLE')):
        row = route_targets[label]
        pin = next(r for r in added if (r[4],r[5]) == (row['host'],row['port']))
        words = pin[6].split(); assert words[0] == 'ssh-ed25519'
        fingerprint = 'SHA256:'+base64.b64encode(hashlib.sha256(base64.b64decode(words[1],validate=True)).digest()).decode().rstrip('=')
        description = (evidence/(prefix+'-peer-'+str(index)+'-description.txt')).read_text()
        assert fingerprint in description and fingerprint == descriptor('fixture.route.'+('outer','middle')[index]+'.fingerprint.')

def startup_stack(app, suffix):
    if not shutil.which('gdb') or app not in guis():
        return
    with (evidence / (prefix + '-' + suffix + '.gdb.log')).open('wb') as stream:
        debugger = subprocess.Popen(['gdb', '--batch', '--nx',
            '-iex', 'set debuginfod enabled off', '-iex', 'set print frame-arguments none',
            '-iex', 'set print thread-events off', '-ex', 'thread apply all bt 12',
            '-ex', 'thread 1', '-ex', 'bt 48', '-ex', 'detach', '-p', str(app)],
            env=env, stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.STDOUT)
        diagnostic_children.append((debugger, stream))
        try:
            debugger.wait(timeout=15)
        except subprocess.TimeoutExpired:
            debugger.kill()
            debugger.wait(timeout=3)

def launch(tag):
    global client, xid
    assert not guis() and not (runtime / 'control.sock').exists()
    auth_before = auth_count()
    signatures_before = event_count('fixture.agent_selected_signed_once')
    arguments = [str(pulse), 'open', '--host', 'GUI_HOST'] if alias_mode else [str(pulse), 'ssh', host, '--identity', identity]
    if interactive_mode: arguments.append("--password-interactive" if password_interactive_mode else "--agent-interactive" if agent_interactive_mode else "--key-interactive" if key_interactive_mode else "--interactive")
    client = subprocess.Popen(arguments,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    app = wait(lambda: (guis() or [None])[0], 'cold_spawn')
    assert guis() == [app]
    def window():
        found = run(['/usr/bin/xdotool','search','--onlyvisible','--pid',app],check=False).stdout.splitlines()
        return next((value for value in found if xdo('getwindowname',value)=='PulseTerm · 工作区预览'),None)
    try:
        xid = wait(window, 'window')
    except AssertionError:
        owned_windows = run(['/usr/bin/xdotool', 'search', '--pid', app], check=False).stdout.splitlines()
        names = {value: run(['/usr/bin/xdotool', 'getwindowname', value], check=False).stdout.strip() for value in owned_windows}
        (evidence / (prefix + '-window-failure.json')).write_text(json.dumps({
            'stage': 'cold_window_before_invitation', 'tag': tag, 'owned_gui_pid': app,
            'cli_exit_code': client.poll(), 'owned_windows': names,
            'control_endpoint_present': (runtime / 'control.sock').exists(),
        }, indent=2) + '\n')
        run(['/usr/bin/import', '-window', 'root', evidence / (prefix + '-window-failure.png')], check=False)
        startup_stack(app, 'window-failure')
        raise
    xdo('windowactivate','--sync',xid)
    try:
        wait(lambda: accessible('解锁保险库').returncode == 0, 'locked_invitation')
    except AssertionError:
        run(['xdotool','getwindowname',xid],check=False)
        try:
            run(['/usr/bin/import','-window','root',evidence/(prefix+'-invitation-failure.png')],check=False)
        except (subprocess.TimeoutExpired,OSError):
            pass
        (evidence / (prefix + '-invitation-failure.json')).write_text(json.dumps({
            'stage': 'cold_invitation_not_ready', 'tag': tag, 'owned_gui_pid': app,
            'cli_exit_code': client.poll(), 'owned_window': xid,
            'control_endpoint_present': (runtime / 'control.sock').exists(),
        }, indent=2) + '\n')
        startup_stack(app, 'invitation-failure')
        raise
    assert client.poll() is None
    if agent_interactive_mode:
        assert auth_count() == auth_before
        assert event_count('fixture.agent_selected_signed_once') == signatures_before
        assert event_count('fixture.agent_unexpected_sign_request') == 0
    else:
        assert quiet()
    shot(tag+'-locked')
    return app

def client_result(tag, accepted):
    assert client is not None, 'cold.cli_client_missing'
    output,error = client.communicate(timeout=30 if accepted else 10)
    (evidence / (prefix+'-'+tag+'-client.log')).write_text(output+error)
    assert (client.returncode == 0) == accepted
    if accepted:
        assert output.startswith('session ') and output.endswith('\n')
    else:
        assert not output

def capture_owned_close_failure():
    for app in guis():
        log = evidence/('native-close-'+close_state+'-'+close_phase+'-'+str(app)+'-failure.gdb.log')
        with log.open('wb') as stream:
            debugger = subprocess.Popen(['/usr/bin/gdb', '--batch', '--nx',
                '-iex', 'set debuginfod enabled off', '-iex', 'set print frame-arguments none',
                '-iex', 'set print thread-events off',
                '-ex', 'echo OWNED_GUI_STACK_CAPTURE_BEGIN\\n',
                '-ex', 'thread apply all bt 12', '-ex', 'info sharedlibrary',
                '-ex', 'detach', '-ex', 'echo OWNED_GUI_STACK_CAPTURE_END\\n',
                '-p', str(app)], env=env, stdin=subprocess.DEVNULL,
                stdout=stream, stderr=subprocess.STDOUT)
            diagnostic_children.append((debugger,stream))
            try:
                debugger.wait(timeout=30)
            except subprocess.TimeoutExpired:
                debugger.kill(); debugger.wait(timeout=5)

def close_gui(app, before_reap: Callable[[], None] | None = None):
    if os.environ.get('PULSE_COLD_CLOSE_GDB') == '1':
        log = evidence/(prefix+'-gui-close-'+str(app)+'.gdb.log')
        stream = log.open('wb')
        debugger = subprocess.Popen(['/usr/bin/gdb', '--batch', '--nx', '-iex', 'set debuginfod enabled off', '-iex', 'set print frame-arguments none', '-iex', 'set auto-solib-add '+('on' if close_cycles else 'off'), '-ex', 'set print thread-events off', '-ex', 'handle SIGPIPE nostop noprint pass', '-ex', 'echo OWNED_GUI_DEBUGGER_READY\n', '-ex', 'continue', '-ex', 'thread apply all bt 12', '-ex', 'info sharedlibrary', '-ex', 'detach', '-p', str(app)], env=env, stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.STDOUT)
        diagnostic_children.append((debugger,stream))
        wait(lambda: 'OWNED_GUI_DEBUGGER_READY' in log.read_text(errors='replace') or debugger.poll() is not None, 'owned_debugger_attach', 30)
        assert debugger.poll() is None, log.read_text(errors='replace')
    xdo("windowactivate", "--sync", xid)
    xdo("key", "--clearmodifiers", "alt+F4")
    deadline = time.monotonic() + 12
    if before_reap is not None:
        before_reap()
    def exited():
        waited,status = os.waitpid(app,os.WNOHANG)
        if waited:
            code = os.waitstatus_to_exitcode(status)
            (evidence/(prefix+'-gui-exit-'+str(app)+'.txt')).write_text(str(code)+'\n')
            assert code == 0, ('gui_native_exit', app, code)
            return True
        return False
    wait(exited,'gui_native_exit',max(0, deadline-time.monotonic()))
    assert not guis() and not (runtime/'control.sock').exists() and not (runtime/'control.token').exists()

def unlock_without_confirming():
    assert client is not None, 'cold.cli_client_missing'
    before = (auth_count(), signature_count())
    click('解锁保险库')
    wait(lambda: accessible('主密码').returncode == 0, 'master_password_field')
    value = accessible('主密码','focus')
    assert value.returncode == 0
    xdo('key','--clearmodifiers','ctrl+a')
    xdo('type','--clearmodifiers','fixed-target-master')
    click('解锁保险库')
    wait(lambda: accessible('已解锁 · 不代表允许 SSH 使用凭据').returncode == 0, 'vault_available')
    assert pending_is_quiet(before) and client.poll() is None
    click('关闭')
    wait(lambda: accessible('查看目标').returncode == 0,'distinct_click')
    assert pending_is_quiet(before) and client.poll() is None
    shot('unlocked-still-unapproved')

try:
    seed_root = Path(subprocess.check_output(['mktemp', '-d', '/data/temp/pulseterm-workspace-ssh-session.XXXXXX'], text=True).strip())
    marker = seed_root / 'owned-workspace-ssh-session.fixture'
    marker.write_bytes(b'owned-fixture-v1'); marker.chmod(0o600)
    data = seed_root / 'data'; data.mkdir(mode=0o700)
    env['XDG_DATA_HOME'] = str(data)
    if agent_mode or agent_interactive_mode:
        env["SSH_AUTH_SOCK"] = str(seed_root / "agent.sock")
    directory = data / 'dev.pulseterm.workspace-preview'
    database = directory / 'state.db'
    if key_interactive_mode or agent_interactive_mode or route_key_interactive_mode or route_agent_interactive_mode:
        policy = seed_root / "owned-joint-policy"
        policy.write_text("partial")
        env["PULSE_SSH_KEY_MFA_POLICY"] = str(policy)
        if route_key_interactive_mode or route_agent_interactive_mode:
            for hop in ("outer", "middle"):
                policy.with_suffix("."+hop).write_text("partial")
    if password_interactive_mode:
        policy = seed_root / "owned-password-policy"
        policy.write_text("partial")
        env["PULSE_SSH_PASSWORD_MFA_POLICY"] = str(policy)
    if route_password_interactive_mode:
        policy = seed_root / "owned-route-password-policy"
        policy.write_text("partial")
        env["PULSE_SSH_PASSWORD_MFA_POLICY"] = str(policy)
        for hop in ("outer", "middle"):
            policy.with_suffix("."+hop).write_text("partial")
    mode = "password-interactive-route" if route_password_interactive_mode else "password-interactive" if password_interactive_mode else "agent-interactive-route" if route_agent_interactive_mode else "agent-interactive" if agent_interactive_mode else "key-interactive-route" if route_key_interactive_mode else "key-interactive" if key_interactive_mode else "interactive-route" if route_interactive_mode else "interactive" if interactive_mode else "agent-route" if route_mode else "cold-agent" if agent_mode else "normal"
    peer = subprocess.Popen([os.environ.get("PULSE_SSH_FIXTURE_EXECUTABLE", str(binaries / "xtask")),"serve-workspace-ssh-fixture",str(directory),mode],env=env,stdout=peer_output,stderr=subprocess.STDOUT,start_new_session=True)
    wait(lambda:'fixture.ssh_gui_seeded_listening' in peer_log.read_text(),'fixture',30)
    def descriptor(prefix):
        return next(line.removeprefix(prefix) for line in peer_log.read_text().splitlines() if line.startswith(prefix))
    host = descriptor('fixture.ssh_gui_host_id.')
    identity = descriptor('fixture.ssh_gui_identity_id.')
    if scope_mode:
        (directory / 'scope-scroll').mkdir(mode=0o700)
    if key_interactive_mode or route_key_interactive_mode:
        with sqlite3.connect('file:'+str(database)+'?mode=ro', uri=True) as db:
            rows = db.execute("SELECT id FROM key_identities WHERE deleted=0 AND label='GUI_KEY_IDENTITY'").fetchall()
            assert len(rows) == 1, 'fixture.unique_saved_key_required'
            identity = rows[0][0]
    if route_mode:
        with sqlite3.connect('file:'+str(database)+'?mode=ro',uri=True) as db:
            if not route_interactive_mode or route_agent_interactive_mode:
                rows = db.execute("SELECT id FROM agent_identities WHERE deleted=0 AND label='GUI_ROUTE_AGENT'").fetchall()
                assert len(rows) == 1
                identity = rows[0][0]
            documents = []
            for (value,) in db.execute("SELECT value FROM app_state"):
                try: documents.append(json.loads(value))
                except (ValueError,TypeError): continue
            catalogue = next(doc for doc in documents if isinstance(doc,dict) and any(row.get('id')==host for row in doc.get('hosts',[])))
            route_targets = {row['label']:row for row in catalogue['hosts']}
    original_pins,original_pin_state = route_pins() if route_mode else (None,None)
    initial = None if scope_mode else source_hashes()
    if scope_mode:
        import runpy
        runpy.run_path("scripts/probe-workspace-ssh-settings.py")["verify"](globals())
        sys.exit(0)
    assert initial is not None, "fixture.source_snapshot_required"
    if route_password_interactive_mode:
        import runpy
        runpy.run_path("scripts/probe-workspace-route-password-interactive.py")["verify"](globals())
        sys.exit(0)
    if route_interactive_mode:
        prefix = "route-password-interactive" if route_password_interactive_mode else "route-agent-interactive" if route_agent_interactive_mode else "route-key-interactive" if route_key_interactive_mode else "route-interactive"
        import runpy
        runpy.run_path("scripts/probe-workspace-route-interactive.py")["verify"](globals())
        sys.exit(0)
    if interactive_mode:
        prefix = "cli-password-interactive" if password_interactive_mode else "cli-agent-interactive" if agent_interactive_mode else "cli-key-interactive" if key_interactive_mode else "cli-interactive"
        import runpy
        runpy.run_path("scripts/probe-workspace-cli-interactive.py")["verify"](globals())
        sys.exit(0)
    if multiwindow_mode:
        import runpy
        runpy.run_path("scripts/probe-workspace-multiwindow-ssh.py")["verify"](globals())
        print("workspace.cli_multiwindow_ssh_" + ("route" if route_mode else "agent" if agent_mode else "password") + "_independent_vault_approval_refresh_native_exit_confirmed")
        sys.exit(0)
    if close_cycles:
        cycle_receipts = []
        debugger_requested = os.environ.get('PULSE_COLD_CLOSE_GDB') == '1'
        diagnostic_kind = 'gdb' if debugger_requested else 'normal'
        if close_phase != 'settled': diagnostic_kind += '-' + close_phase
        receipt_path = evidence/('native-close-'+close_state+'-'+diagnostic_kind+'-result.json')
        def save_close_receipt():
            receipt_path.write_text(json.dumps({'version':1,'scope':'Linux actual owned GUI cancellation-close diagnostic','state':close_state,'close_phase':close_phase,'gdb_attached':debugger_requested,'planned_cycles':close_cycles,'completed_cycles':len(cycle_receipts),'passed':len(cycle_receipts)==close_cycles,'historical_sigsegv_resolved':False,'cycles':cycle_receipts},indent=2)+'\n')
        save_close_receipt()
        for cycle in range(close_cycles):
            tag = 'native-close-'+close_state+'-'+diagnostic_kind+'-'+str(cycle+1)
            app = launch(tag)
            if close_state == 'unlocked': unlock_without_confirming()
            assert client is not None and client.poll() is None and quiet() and source_hashes() == initial
            if close_phase == 'settled':
                click('取消'); client_result(tag, False)
                assert quiet() and source_hashes() == initial
                close_gui(app)
            else:
                if close_phase == 'cancel-unobserved': click('取消')
                close_gui(app, lambda: client_result(tag, False))
                assert quiet() and source_hashes() == initial
            cycle_receipts.append({'cycle':cycle+1,'pid':app,'zero_auth_and_signatures':True,'source_hashes_unchanged':True,'gui_exit':0,'endpoints_drained':True})
            save_close_receipt()
            print('workspace.native_close_cycle.'+tag+'.exit0_zero_auth_zero_write',flush=True)
        sys.exit(0)
    app = launch('cancel');click('取消');client_result('cancel',False)
    assert quiet() and source_hashes()==initial
    close_gui(app)
    app = launch('unlocked-cancel');unlock_without_confirming();click('取消');client_result('unlocked-cancel',False)
    assert quiet() and source_hashes()==initial
    if alias_mode and not alias_case:
        client = subprocess.Popen([str(pulse),'open','--host','MISSING_ALIAS'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        client_result('warm-missing-alias', False)
        assert quiet() and source_hashes()==initial and accessible('允许 CLI 连接保存的 SSH 主机？').returncode != 0
        client = subprocess.Popen([str(pulse),'open','--host','GUI_HOST'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        wait(lambda:accessible('允许 CLI 连接保存的 SSH 主机？').returncode==0,'warm_exact_target_cancel')
        shot('warm-exact-target-cancel');click('取消');client_result('warm-target-cancel', False)
        assert quiet() and source_hashes()==initial
    close_gui(app)
    if alias_case:
        app = launch('rejected-source');unlock_without_confirming();click('查看目标')
        client_result('rejected-source', False)
        assert quiet() and source_hashes()==initial and accessible('允许 CLI 连接保存的 SSH 主机？').returncode != 0
        close_gui(app)
        (evidence/(prefix+'-result.json')).write_text(json.dumps({'version':1,'passed':True,'scope':'Linux saved alias fail-closed real cold WebView','case':alias_case,'zero_authentication_and_signatures':True,'source_hashes_unchanged':True,'independent_processes':3,'actual_gui_exit_and_endpoints_drained':True},indent=2)+'\n')
        print('workspace.cli_saved_alias_invalid_source_zero_auth_zero_writes_native_exit_confirmed')
        sys.exit(0)
    if route_mode:
        app = launch('first-hop-cancel');unlock_without_confirming();click('查看目标')
        wait(lambda:accessible('允许 CLI 连接保存的 SSH 主机？').returncode==0,'exact_target_cancel')
        click('连接');route_prompt(0);shot('first-hop-cancel');click('取消');client_result('first-hop-cancel',False)
        assert original_pins is not None and original_pin_state is not None
        assert quiet() and source_hashes()==initial and route_pins()==(original_pins,original_pin_state)
        close_gui(app)
    if alias_warm:
        app = launch('warm-context');unlock_without_confirming();click('取消');client_result('warm-context', False)
        assert quiet() and source_hashes()==initial
        client = subprocess.Popen([str(pulse),'open','--host','GUI_HOST'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    else:
        app = launch('approved');unlock_without_confirming();click('查看目标')
    wait(lambda:accessible('允许 CLI 连接保存的 SSH 主机？').returncode==0,'exact_target')
    shot('exact-target');assert quiet()
    click("连接")
    if route_mode:
        for index in range(3):
            button = route_prompt(index)
            click(button)
    client_result("approved", True)
    wait(lambda:auth_count()==1 and 'fixture.ssh_window_request.' in peer_log.read_text(),'authenticated_terminal')
    for turn in range(2):
        if turn:
            xdo("key", "--clearmodifiers", "ctrl+shift+r")
        # CLI success is native input admission. After refresh wait for the visible terminal.
        time.sleep(.5)
        xdo('mousemove','--window',xid,'500','220','click','1')
        text = 'cold-ssh-echo-'+str(turn)
        xdo('type','--clearmodifiers',text);xdo('key','--clearmodifiers','Return')
        wait(lambda:peer_log.read_text().splitlines().count('fixture.ssh_input_line_echoed')==turn+1,'input_'+str(turn))
        assert auth_count()==1
    if agent_mode:
        assert signature_count() == (2 if route_mode else 1) and event_count("fixture.ssh_password_authenticated") == 0
        assert "fixture.agent_connection_failed" not in peer_log.read_text()
    time.sleep(.3);shot('terminal-refreshed')
    if route_mode:
        assert event_count('fixture.route.outer.authenticated') == 1 and event_count('fixture.route.middle.agent_authenticated') == 1
        after = source_hashes()
        assert set(after) == set(initial)
        for table,digest in initial.items():
            if table not in {'known_hosts','known_hosts_state'}: assert after[table] == digest,(table,'cold route source changed')
        verify_route_pins()
    else:
        assert source_hashes()==initial
    exited_before = {name:event_count("fixture.route."+name+".connection_exited") for name in ("outer","middle")} if route_mode else {}
    forwarded_before = {name:event_count("fixture.route."+name+".forward_exited") for name in ("outer","middle")} if route_mode else {}
    close_gui(app)
    wait(lambda:'fixture.ssh_authenticated_stream_exited' in peer_log.read_text(),'peer_stream_exit')
    if route_mode:
        wait(lambda:all(event_count('fixture.route.'+name+'.connection_exited')>count for name,count in exited_before.items()) and all(event_count('fixture.route.'+name+'.forward_exited')>count for name,count in forwarded_before.items()),'route_parent_streams_exit')
    receipt = {'version':1,'passed':True,'scope':'Linux real WebView and selected-key software Agent, production cold CLI saved IDs' if agent_mode else 'Linux real WebView and password peer, production cold CLI saved IDs','identity_kind':'agent' if agent_mode else 'password','locked_cancel_zero_auth_and_writes':True,'unlock_does_not_approve':True,'distinct_click_then_exact_target_approval':True,'one_authentication':True,'selected_agent_signatures':signature_count(),'password_authentication_count':event_count('fixture.ssh_password_authenticated'),'input_before_and_after_refresh':True,'refresh_no_redial':True,'independent_processes':4 if route_mode else 3,'actual_gui_exit_and_endpoints_drained':True,'peer_stream_exit':True,'source_hashes':initial}
    if alias_mode:
        receipt.update({'scope':'Linux real WebView saved alias '+('warm' if alias_warm else 'cold')+' CLI with '+('mixed Agent route' if route_mode else 'selected-key Agent' if agent_mode else 'password peer'),'alias':'GUI_HOST','exact_saved_default_identity':True,'warm_missing_alias_zero_auth_and_writes':True,'warm_target_cancel_zero_auth_and_writes':True,'warm_original_window_approval':alias_warm})
    if route_mode:
        receipt.pop('one_authentication')
        receipt["leaf_password_authentication_count"] = receipt.pop("password_authentication_count")
        receipt["outer_password_authentication_count"] = event_count("fixture.route.outer.authenticated")
        receipt["middle_agent_authentication_count"] = event_count("fixture.route.middle.agent_authenticated")
        receipt["leaf_agent_authentication_count"] = auth_count()
        receipt.update({'scope':'Linux '+('warm' if alias_warm else 'cold')+' CLI actual three-peer password/Agent route in WebView','first_hop_cancel_zero_auth_and_writes':True,'one_authentication_per_hop':True,'exact_per_hop_target_identity_fingerprint':True,'two_exact_first_pins':True,'parent_connection_and_forward_streams_exited':True})
    (evidence/(prefix+'-result.json')).write_text(json.dumps(receipt,indent=2)+'\n')
    print('workspace.cli_cold_ssh_unlock_distinct_click_target_approval_refresh_native_exit_confirmed')
finally:
    if xid and guis():
        if close_cycles and os.environ.get('PULSE_COLD_CLOSE_FAILURE_GDB') == '1':
            try:
                capture_owned_close_failure()
            except (subprocess.TimeoutExpired, OSError):
                pass # Stack capture failure must not bypass owned process cleanup.
        try:
            run(["/usr/bin/import","-window",xid,evidence/(prefix+"-failure.png")],check=False)
        except (subprocess.TimeoutExpired, OSError):
            pass # A failed screenshot must never bypass process/fixture cleanup.
    if client:
        if client.poll() is None:
            client.terminate()
        try:
            output, error = client.communicate(timeout=3)
            if output or error:
                (evidence/(prefix+'-final-client.log')).write_text(output+error)
        except subprocess.TimeoutExpired:
            client.kill();client.communicate(timeout=3)
    for pid in guis():
        try:
            os.kill(pid,signal.SIGTERM)
            end = time.monotonic()+3
            while not os.waitpid(pid,os.WNOHANG)[0]:
                if time.monotonic()>=end:
                    os.kill(pid,signal.SIGKILL);os.waitpid(pid,0);break
                time.sleep(.025)
        except (ProcessLookupError,ChildProcessError):
            pass
    for debugger,stream in diagnostic_children:
        if debugger.poll() is None:
            debugger.terminate()
        try: debugger.wait(timeout=3)
        except subprocess.TimeoutExpired:
            debugger.kill()
            debugger.wait(timeout=3)
        stream.close()
    if peer:
        if peer.poll() is None:
            peer.terminate()
        peer.wait(timeout=3)
    peer_output.close()
    end=time.monotonic()+5
    while True:
        owned=[]
        for proc in Path('/proc').iterdir():
            if not proc.name.isdigit() or int(proc.name)==os.getpid():continue
            try:
                if ('XDG_RUNTIME_DIR='+env['XDG_RUNTIME_DIR']).encode() in (proc/'environ').read_bytes().split(b'\0'):owned.append(int(proc.name))
            except (FileNotFoundError,PermissionError,ProcessLookupError):pass
        for pid in owned:
            try:os.kill(pid,signal.SIGTERM if time.monotonic()<end else signal.SIGKILL)
            except ProcessLookupError:pass
        try:
            if os.waitpid(-1,os.WNOHANG)[0]:continue
            assert time.monotonic()<end+3,'cold.ssh_cleanup_timeout'
            time.sleep(.025)
        except ChildProcessError:
            assert not owned
            break
    if seed_root is not None:
        assert seed_root.parent == Path('/data/temp') and seed_root.name.startswith('pulseterm-workspace-ssh-session.')
        shutil.rmtree(seed_root)
        assert not seed_root.exists()
    assert root.parent==Path(os.environ['TMPDIR']) and root.name.startswith('c.')
    shutil.rmtree(root)
