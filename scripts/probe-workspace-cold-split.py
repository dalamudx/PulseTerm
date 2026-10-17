"""Real production cold split: two independent native consents and two PTYs."""
import json
import os
import re
import subprocess
import time


def wait(predicate, tag, seconds=10):
    deadline = time.monotonic() + seconds
    while True:
        value = predicate()
        if value:
            return value
        assert time.monotonic() < deadline, 'cold.split_timeout.' + tag
        time.sleep(.05)


def verify(mode, client, pulse, root, env, runtime, evidence, owned_guis, xdo, identity):
    _, direction, scenario = mode.split('-')
    chain = mode.startswith("chain-")
    prefix = 'cli-cold-' + mode
    pid_file = root / 'unexpected-pty'
    app = wait(lambda: (owned_guis() or [None])[0], 'gui')
    assert owned_guis() == [app]

    def window():
        try:
            matches = [item for item in xdo('search', '--onlyvisible', '--pid', app).splitlines()
                       if xdo('getwindowname', item) == 'PulseTerm · 工作区预览']
            return matches[0] if len(matches) == 1 else None
        except subprocess.CalledProcessError:
            return None

    xid = wait(window, 'window')
    xdo('windowactivate', '--sync', xid)

    def accessible(name, action=None):
        command = ['target/probes/x11-status', str(app), name]
        if action:
            command.append(action)
        result = subprocess.run(command, text=True, capture_output=True, timeout=3)
        return result.returncode == 0

    def consent(title):
        wait(lambda: accessible(title), title)
        subprocess.run(["/usr/bin/import", "-window", xid, str(evidence / (prefix + "-" + ("base" if "首个" in title or "本地终端" in title else "split") + "-confirmation.png"))], check=True, timeout=3)

    def click(name):
        assert accessible(name, 'focus'), 'cold.split_button_missing.' + name
        xdo('key', '--clearmodifiers', 'space')

    def pids():
        return [int(line) for line in pid_file.read_text().splitlines()] if pid_file.exists() else []

    consent("允许 CLI 创建本地终端？" if chain else "为冷分屏创建首个终端？")
    assert not pids(), 'cold.split_native_before_base_consent'
    socket, token = runtime / 'control.sock', runtime / 'control.token'
    original = identity(socket), identity(token), token.read_bytes()
    if scenario == 'cancel':
        click('取消')
    else:
        click("创建终端" if chain else "创建首个终端")
        consent('允许 CLI 创建分屏终端？')
        assert len(pids()) == 1, 'cold.split_second_native_before_consent'
        if scenario == 'partial':
            click('取消')
        elif scenario == 'disconnect':
            client.terminate()
        elif scenario == 'refresh':
            xdo('key', '--clearmodifiers', 'ctrl+shift+r')
        else:
            click('创建分屏')
    output, error = client.communicate(timeout=15)
    (evidence / (prefix + '-client.log')).write_text(output + error)
    accepted = scenario == 'approve'
    assert (client.returncode == 0) == accepted, 'cold.split_cli_ack_mismatch'
    if chain:
        lines = output.splitlines()
        completed = 2 if accepted else (0 if scenario == "cancel" else 1)
        assert len(lines) == (3 if accepted else completed), 'chain_later_step_after_failure'
        assert all(re.fullmatch(r'session [1-9][0-9]*', line) for line in lines[:completed])
        if accepted:
            assert len(set(lines[:2])) == 2, 'chain_duplicate_session'
            offers = json.loads(lines[2])
            assert set(offers) == {'version', 'profiles'} and offers['version'] == 1
            assert 'sh' in offers['profiles'] and set(offers['profiles']) <= {'bash', 'zsh', 'fish', 'sh', 'pwsh'}
    elif accepted:
        assert re.fullmatch(r'session [1-9][0-9]*\n', output)
    count = 2 if accepted else (0 if scenario == 'cancel' else 1)
    wait(lambda: len(pids()) == count, 'pty_count')
    native = pids()
    assert len(set(native)) == count
    for pid in native:
        os.kill(pid, 0)
    if accepted:
        # Coordinates are actual terminal clicks in the fixed 900x480 window.
        # No uncertain input is retried: each uniquely marked write is sent once.
        points = [(420, 320), (780, 320)] if direction == 'row' else [(600, 200), (600, 410)]
        for turn in range(2):
            if turn:
                xdo('key', '--clearmodifiers', 'ctrl+shift+r')
            time.sleep(1)
            for index, point in enumerate(points):
                text = f'cold-split-{direction}-pane-{index}-turn-{turn}'
                xdo('mousemove', '--window', xid, *point, 'click', 1)
                xdo('type', '--clearmodifiers', '--delay', 1, text)
                xdo('key', '--clearmodifiers', 'Return')
                wait(lambda: any((root / f'unexpected-pty.{pid}.input').exists() and text in (root / f'unexpected-pty.{pid}.input').read_text().splitlines() for pid in native), 'input.' + text)
            assert pids() == native, 'cold.split_refresh_redialed'
        inputs = [(root / f'unexpected-pty.{pid}.input').read_text().splitlines() for pid in native]
        expected = [[f'cold-split-{direction}-pane-{index}-turn-{turn}' for turn in range(2)] for index in range(2)]
        assert sorted(inputs) == sorted(expected), 'cold.split_input_not_independent'
        (evidence / (prefix + '-input.json')).write_text(json.dumps(inputs, ensure_ascii=False, indent=2)+'\n')
        subprocess.run(['/usr/bin/import', '-window', xid, str(evidence / (prefix+'-terminal-refreshed.png'))], check=True, timeout=3)
    assert pids() == native
    assert (identity(socket), identity(token), token.read_bytes()) == original
    assert owned_guis() == [app]
    xdo("windowactivate", "--sync", xid)
    xdo("key", "--clearmodifiers", "alt+F4")

    def exited():
        waited, status = os.waitpid(app, os.WNOHANG)
        if waited:
            assert os.waitstatus_to_exitcode(status) == 0
            return True
        return False

    wait(exited, 'gui_exit')
    assert not socket.exists() and not token.exists()
    assert all(not os.path.exists(f'/proc/{pid}') for pid in native), 'cold.split_native_not_reaped'
    (evidence / (prefix+'-result.json')).write_text(json.dumps(dict(mode=mode, direction=direction, scenario=scenario, accepted=accepted, pty_count=count, native_pids=native, two_explicit_confirmations=scenario!='cancel', refresh_same_ptys=accepted, independent_inputs=accepted, actual_gui_exit=True, native_exit=True, endpoints_drained=True), indent=2)+'\n')
