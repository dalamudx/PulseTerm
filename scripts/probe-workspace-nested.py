#!/usr/bin/env python3
"""One real WebView path: nested layout, preview-only drag, hidden input, refresh, close."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

fixture = Path(sys.argv[1])
evidence = Path('target/probes')
cli_mode = os.environ.get('PULSETERM_NESTED_CLI') == '1'
log = evidence / ('workspace-nested-cli-native.log' if cli_mode else 'workspace-nested-native.log')
clients = []
app = None
xid = ''
stage = 'startup'
checks = []

def run(args, check=True):
    return subprocess.run(args, check=check, capture_output=True, timeout=3)

def count(name):
    return log.read_text().splitlines().count('workspace.' + name)

def wait(predicate, label):
    end = time.monotonic() + 10
    while time.monotonic() < end:
        if predicate():
            return
        if app is not None and app.poll() is not None:
            raise RuntimeError('nested.exited.' + label)
        time.sleep(0.1)
    raise RuntimeError('nested.timeout.' + label)

def key(*keys):
    run(['/usr/bin/xdotool', 'key', '--clearmodifiers', *keys])

def accessible(text, action=None):
    assert app is not None
    args = [str(evidence/'x11-status'), str(app.pid), text]
    if action:
        args.append(action)
    return run(args, check=False)

def reveal(text, action):
    if accessible(text, action).returncode == 0:
        return
    for direction, steps in [('5', 5), ('5', 5), ('4', 18)]:
        run(['/usr/bin/xdotool', 'mousemove', '--window', xid, '100', '500', 'click', '--repeat', str(steps), '--delay', '25', direction])
        time.sleep(0.1)
        if accessible(text, action).returncode == 0:
            return
    wait(lambda: accessible(text, action).returncode == 0, text)

def click(text):
    reveal(text, 'activate')

def focus(text):
    reveal(text, 'focus')

def bounds(text):
    result = accessible(text, 'bounds')
    return tuple(map(int, result.stdout.split()))

def pids():
    path = fixture/'pids'
    return [int(p) for p in path.read_text().splitlines()] if path.exists() else []

def size(pid):
    path = fixture/f'size.{pid}'
    return tuple(map(int, path.read_text().splitlines()[-1].split())) if path.exists() else (0, 0)

def inputs(pid):
    path = fixture/f'input.{pid}'
    return path.read_bytes() if path.exists() else b''

def select(session):
    click(f'会话 {session} · 会话 {session}')

def target(index):
    focus('选择已有会话')
    x, y, width, height = bounds('选择已有会话')
    run(['/usr/bin/xdotool', 'mousemove', str(x+width//2), str(y+height//2), 'click', '1'])
    time.sleep(0.15)
    key('Home', *(['Down'] * (index-1)), 'Return')
    time.sleep(0.15)

def drag(text, delta, cancel=False):
    x, y, width, height = bounds(text)
    run(['/usr/bin/xdotool', 'mousemove', str(x+width//2), str(y+height//2), 'mousedown', '1'])
    run(['/usr/bin/xdotool', 'mousemove', str(x+width//2), str(y+height//2+delta)])
    if cancel:
        key('Escape')
    else:
        time.sleep(0.2)
    return (x, y)

def input_to(session, text):
    before = [inputs(pid) for pid in pids()]
    select(session)
    run(['/usr/bin/xdotool', 'type', '--clearmodifiers', '--delay', '2', text])
    key('Return')
    pid = pids()[session-1]
    wait(lambda: inputs(pid).endswith((text+'\n').encode()), 'focused_input')
    for index, other in enumerate(pids()):
        if other != pid:
            assert inputs(other) == before[index], 'nested.hidden_input_leak'

def ready():
    reveal('最大化窗格', 'focus')

def screenshot(name):
    run(['/usr/bin/import', '-window', xid, str(evidence/name)])

def exercise_cli():
    global stage
    process = app
    assert process is not None
    for number in range(1, 3):
        click('POSIX sh')
        wait(lambda: count('terminal_input_ready') >= number and len(pids()) == number, 'cli_two_sessions')
    select(1)
    target(2)
    click('左右分屏')
    ready()
    original = pids()
    input_to(1, 'CLI_CAPTURE_FOCUS')
    grids = [size(pid) for pid in original]
    def ask():
        path = evidence/f'workspace-nested-cli-client-{len(clients)}.log'
        with path.open('wb') as output:
            client = subprocess.Popen(['target/debug/pulse','split-pane','-h','sh'],stdout=output,stderr=output)
        clients.append(client)
        return client
    stage = 'cli_cancel'
    client = ask()
    wait(lambda: accessible('创建分屏').returncode == 0, 'cli_cancel_dialog')
    key('Return')
    assert client.wait(timeout=8) != 0
    assert count('session_opened') == 2 and pids() == original
    stage = 'cli_approve_nested'
    client = ask()
    wait(lambda: accessible('创建分屏').returncode == 0, 'cli_approve_dialog')
    key('Tab','Return')
    assert client.wait(timeout=8) == 0
    wait(lambda: len(pids()) == 3, 'cli_created_one')
    ready()
    current = pids()
    assert current[:2] == original and count('session_opened') == 3
    wait(lambda: size(original[0])[0] < grids[0][0], 'cli_nested_native_grid')
    assert size(original[1]) == grids[1], 'cli.unaffected_grid_changed'
    input_to(3, 'CLI_NESTED_ONLY')
    screenshot('workspace-nested-cli-approved.png')
    stage = 'cli_maximum_denied'
    click('最大化窗格')
    reveal('还原分屏','focus')
    client = ask()
    assert client.wait(timeout=8) != 0
    assert count('session_opened') == 3 and pids() == current
    assert accessible('创建分屏').returncode != 0
    click('还原分屏')
    ready()
    input_to(1, 'CLI_ORIGINAL_SURVIVES')
    assert 'workspace.terminal_failure.' not in log.read_text()
    stage = 'cli_shutdown'
    key('alt+F4')
    process.wait(timeout=8)
    assert process.returncode == 0 and all(not Path(f'/proc/{pid}').exists() for pid in current)
    (evidence/'workspace-nested-cli-acceptance.json').write_text(json.dumps({'cancel_zero_start':True,'approved_one_nested_start':True,'old_pids_preserved':True,'unaffected_grid_preserved':True,'focused_input_isolated':True,'maximum_rejected_before_create':True,'native_exit_confirmed':True,'scope':'Linux actual CLI and WebView'},indent=2)+'\n')
    print('workspace.cli_nested_cancel_approve_original_pids_maximum_denial_native_exit_confirmed')

try:
    with log.open('wb') as output:
        app = subprocess.Popen(['target/debug/pulseterm-workspace'], stdout=output, stderr=output)
    process = app
    wait(lambda: count('sessions_listed') == 1, 'page')
    def window():
        global xid
        result = run(['/usr/bin/xdotool', 'search', '--onlyvisible', '--all', '--pid', str(process.pid), '--name', '^PulseTerm · 工作区预览$'], check=False)
        found = result.stdout.split()
        if len(found) != 1:
            return False
        xid = found[0].decode()
        return run(['/usr/bin/xdotool', 'getwindowpid', xid]).stdout.strip() == str(process.pid).encode()
    wait(window, 'window_identity')
    run(['/usr/bin/xdotool', 'windowactivate', '--sync', xid])
    if cli_mode:
        exercise_cli()
        raise SystemExit(0)
    for number in range(1, 4):
        click('POSIX sh')
        wait(lambda: count('terminal_input_ready') >= number and len(pids()) == number, 'three_sessions')
    original = pids()
    target(1)
    click('左右分屏')
    ready()
    select(3)
    target(2)
    stage = 'nested'
    click('上下分屏')
    ready()
    wait(lambda: size(original[2])[0] < size(original[0])[0] and size(original[2])[1] < 60, 'native_nested_grid')
    nested = [size(pid) for pid in original]
    checks.append({'nested_native_sizes':nested, 'pids':original})
    input_to(3, 'NESTED_TOP')
    input_to(2, 'NESTED_BOTTOM')
    stage = 'drag'
    before = count('terminal_input_ready')
    drag('调整分屏比例 split_2', 75)
    assert count('terminal_input_ready') == before, 'nested.motion_resized_native'
    run(['/usr/bin/xdotool', 'mouseup', '1'])
    ready()
    wait(lambda: size(original[2])[0] > nested[2][0], 'drag_native_commit')
    assert pids() == original and count('session_opened') == 3
    checks.append({'drag_native_sizes':[size(pid) for pid in original], 'preview_input_ready_count':before})
    settled = [size(pid) for pid in original]
    before = count('terminal_input_ready')
    drag('调整分屏比例 split_2', -45, cancel=True)
    run(['/usr/bin/xdotool', 'mouseup', '1'])
    assert [size(pid) for pid in original] == settled and count('terminal_input_ready') == before
    stage = 'keyboard_ratio'
    focus('调整分屏比例 split_2')
    key('Up')
    ready()
    wait(lambda: size(original[2])[0] < settled[2][0], 'keyboard_ratio_commit')
    input_to(3, 'KEYBOARD_RATIO')
    stage = 'maximize'
    click('最大化窗格')
    reveal('还原分屏', 'focus')
    wait(lambda: size(original[2])[1] > 80, 'maximum_grid')
    # Choosing the same owning tab retains maximum display and input stays in it.
    input_to(3, 'MAXIMUM_ONLY')
    maximum = size(original[2])
    screenshot('workspace-nested-maximized.png')
    stage = 'refresh'
    before = count('document_confirmed')
    key('ctrl+shift+r')
    wait(lambda: count('document_confirmed') == before+1, 'new_page')
    reveal('还原分屏', 'focus')
    assert pids() == original and count('session_opened') == 3 and size(original[2]) == maximum
    click('还原分屏')
    ready()
    wait(lambda: size(original[2])[1] < maximum[1], 'nested_restored')
    input_to(2, 'REFRESH_BOTTOM')
    screenshot('workspace-nested-restored.png')
    stage = 'close'
    input_to(3, 'CLOSE_FOCUS')
    click('关闭会话')
    key('Tab', 'Return')
    wait(lambda: count('session_closed') == 1, 'focused_close')
    ready()
    assert not Path(f'/proc/{original[2]}').exists(), 'nested.closed_child_alive'
    assert all(Path(f'/proc/{pid}').exists() for pid in original[:2])
    input_to(2, 'SURVIVOR_AFTER_CLOSE')
    assert count('session_opened') == 3 and 'workspace.terminal_failure.' not in log.read_text()
    stage = 'shutdown'
    key('alt+F4')
    process.wait(timeout=8)
    assert process.returncode == 0 and all(not Path(f'/proc/{pid}').exists() for pid in original)
    checks.append({'refresh_pids_unchanged':True, 'closed_focus_only':True, 'native_exit_confirmed':True})
    (evidence/'workspace-nested-acceptance.json').write_text(json.dumps({'checks':checks,'scope':'Linux actual WebView; no cross-platform acceptance'},indent=2)+'\n')
    print('workspace.nested_drag_release_keyboard_maximize_refresh_prune_native_exit_confirmed')
except Exception:
    if xid:
        screenshot('workspace-nested-failure.png')
    diagnostics=[stage]
    for name in ['最大化窗格','左右分屏','第一个窗格比例','选择已有会话','调整分屏比例 split_root']:
        observed=accessible(name,'inspect')
        diagnostics.append(f'{name}: exit={observed.returncode} {observed.stdout.decode().strip()}')
    (evidence/'workspace-nested-failure.txt').write_text('\n'.join(diagnostics)+'\n')
    raise
finally:
    for client in clients:
        if client.poll() is None:
            client.terminate()
        client.wait(timeout=3)
    if app is not None and app.poll() is None:
        app.terminate()
        try:
            app.wait(timeout=8)
        except subprocess.TimeoutExpired:
            app.kill()
            app.wait(timeout=3)
    for pid in pids():
        try:
            # Only fixture children from this task are eligible for cleanup.
            environment = Path(f'/proc/{pid}/environ').read_bytes()
            if ('PULSETERM_NESTED_FIXTURE='+str(fixture)).encode()+b'\0' in environment:
                os.kill(pid, signal.SIGKILL)
                wait(lambda: not Path(f'/proc/{pid}').exists(), 'child_cleanup')
        except (FileNotFoundError, ProcessLookupError):
            pass
