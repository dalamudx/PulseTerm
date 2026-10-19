#!/usr/bin/env python3
"""Explicit global scroll settings over real PTYs and production WebView."""
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import termios
import time
import tomllib
import tty

OUT = Path('target/probes/workspace-scroll-settings')
RESULT = OUT / 'result.json'


def worker(directory):
    base = Path(directory) / ('scroll-' + str(os.getppid()))
    base.mkdir()
    received = bytearray()
    old = termios.tcgetattr(0)
    deadline = time.monotonic() + 150

    def publish():
        pending = base / 'input.pending'
        pending.write_text(json.dumps({'hex': received.hex()}))
        pending.replace(base / 'input.json')

    try:
        tty.setraw(0)
        publish()
        os.write(1, ''.join(f'SCROLL_ROW_{row:03d}\r\n' for row in range(160)).encode())
        (base / 'ready').touch()
        emitted = 0
        while not (base / 'finish').exists():
            assert time.monotonic() < deadline, 'scroll.worker_timeout'
            if select.select([0], [], [], .02)[0]:
                data = os.read(0, 256)
                assert data, 'scroll.worker_eof'
                received.extend(data)
                assert len(received) <= 16, 'scroll.unexpected_input'
                publish()
            if (base / ('emit-' + str(emitted + 1))).exists():
                emitted += 1
                os.write(1, (f'SCROLL_LIVE_{emitted}\r\n' * 12 + '\a').encode())
                (base / ('emitted-' + str(emitted))).touch()
    finally:
        termios.tcsetattr(0, termios.TCSANOW, old)
    publish()
    (base / 'finished').touch()


def verify(fixture, app, xid, config, native_log):
    OUT.mkdir(parents=True, exist_ok=True)
    RESULT.write_text(json.dumps({'version': 1, 'passed': False, 'nativeExitConfirmed': False}) + '\n')
    helper = os.environ['PULSE_NATIVE_HELPER']
    returned = '回到最新输出'
    saved_status = '已保存 · 重新加载终端后应用；当前会话未关闭'
    checks = []

    def run(*args, check=True):
        return subprocess.run(list(map(str, args)), check=check, capture_output=True, text=True, timeout=4)

    def wait(predicate, label):
        deadline = time.monotonic() + 12
        while True:
            value = predicate()
            if value:
                return value
            assert time.monotonic() < deadline, 'scroll.timeout.' + label
            time.sleep(.03)

    def native(event):
        return native_log.read_text().splitlines().count('workspace.' + event)

    def visible(name):
        return run(helper, app, name, check=False).returncode == 0

    def accessible(name, action='activate'):
        return run(helper, app, name, action).stdout

    def xdo(*args):
        return run('xdotool', *args)

    def click(name):
        wait(lambda: run(helper, app, name, 'bounds', check=False).returncode == 0, 'bounds.' + name)
        left, top, width, height = map(int, accessible(name, 'bounds').split())
        xdo('mousemove', str(left + width // 2), str(top + height // 2), 'click', '1')

    def key(*keys):
        xdo('key', '--clearmodifiers', *keys)

    def settings():
        before = native('settings_read_confirmed')
        click('设置')
        wait(lambda: native('settings_read_confirmed') > before, 'settings_open')

    def edit(field, label, value=None):
        wait(lambda: run(helper, app, '搜索设置', 'focus', check=False).returncode == 0, 'search')
        key('ctrl+a')
        xdo('type', '--clearmodifiers', '--delay', '1', field)
        wait(lambda: run(helper, app, label, 'focus', check=False).returncode == 0, 'field.' + field)
        original = config.read_bytes() if config.exists() else b''
        if value is not None:
            key('space')
            assert (config.read_bytes() if config.exists() else b'') == original, 'scroll.draft_wrote_settings'
        before = native('settings_save_confirmed')
        click('保存' + label if value is not None else label + '恢复继承')
        wait(lambda: native('settings_save_confirmed') == before + 1, 'saved.' + field)
        wait(lambda: visible(saved_status), 'frontend_save_complete')
        saved = tomllib.loads(config.read_text()).get('terminal', {})
        if value is None:
            assert field not in saved, 'scroll.reset_left_override'
        else:
            assert saved[field] is value, 'scroll.wrong_persisted_value'

    def close_settings():
        key('Escape')
        wait(lambda: not visible('搜索设置'), 'settings_closed')

    def pid():
        return int((fixture / 'native.pid').read_text())

    def start_worker():
        base = fixture / ('scroll-' + str(pid()))
        xdo('mousemove', '--window', xid, '600', '350', 'click', '1')
        xdo('type', '--clearmodifiers', '--delay', '2', 'ALPHA_SCROLL')
        key('Return')
        wait(lambda: (base / 'ready').exists(), 'worker_ready')
        wait(lambda: not visible(returned), 'initial_bottom')
        return base

    def received(base):
        return json.loads((base / 'input.json').read_text())['hex']

    def scroll_up():
        xdo('mousemove', '--window', xid, '600', '350', 'click', '--repeat', '12', '--delay', '20', '4')
        wait(lambda: visible(returned), 'reading_viewport')

    def type_key(base, text, expected):
        xdo('mousemove', '--window', xid, '600', '350', 'click', '1')
        xdo('type', '--clearmodifiers', text)
        wait(lambda: received(base) == expected, 'raw_key.' + text)

    def emit(base, index):
        if visible('清除终端响铃提示'):
            click('清除终端响铃提示')
            wait(lambda: not visible('清除终端响铃提示'), 'clear_previous_output_signal')
        (base / ('emit-' + str(index))).touch()
        wait(lambda: (base / ('emitted-' + str(index))).exists(), 'emitted')
        wait(lambda: visible('清除终端响铃提示'), 'output_parsed')

    def stays_reading(label):
        deadline = time.monotonic() + 1.1
        while time.monotonic() < deadline:
            assert visible(returned), label
            time.sleep(.03)

    def refresh():
        before = native('terminal_input_ready')
        old_pid = pid()
        key('ctrl+shift+r')
        wait(lambda: native('terminal_input_ready') > before, 'refresh')
        assert pid() == old_pid, 'scroll.refresh_created_session'

    first_pid = pid()
    first = start_worker()
    scroll_up()
    emit(first, 1)
    stays_reading('scroll.default_output_moved_viewport')
    type_key(first, 'a', '61')
    wait(lambda: not visible(returned), 'default_input_bottom')
    checks.append('default_output_preserves_and_input_returns')
    settings()
    edit('scroll_on_output', '新输出时回到底部', True)
    edit('scroll_on_input', '输入时回到底部', False)
    explicit_document = config.read_bytes()
    close_settings()
    scroll_up()
    emit(first, 2)
    stays_reading('scroll.changed_mounted_surface')
    type_key(first, 'b', '6162')
    wait(lambda: not visible(returned), 'mounted_input_still_default')
    checks.append('explicit_save_leaves_mounted_surface_unchanged')
    refresh()
    scroll_up()
    type_key(first, 'c', '616263')
    assert visible(returned), 'scroll.disabled_input_moved_viewport'
    run('import', '-window', xid, OUT / 'input-disabled.png')
    emit(first, 3)
    wait(lambda: not visible(returned), 'enabled_output_bottom')
    assert config.read_bytes() == explicit_document
    checks.append('refresh_same_pty_applies_output_enabled_input_disabled')
    (first / 'finish').touch()
    wait(lambda: (first / 'finished').exists(), 'first_worker_done')
    before = native('terminal_input_ready')
    pulse = Path(os.environ['CARGO_TARGET_DIR']) / 'debug/pulse'
    with subprocess.Popen([str(pulse), 'new-tab', 'sh'], stdout=subprocess.PIPE, stderr=subprocess.PIPE) as client:
        try:
            wait(lambda: visible('创建终端') or client.poll() is not None, 'cli_request')
            assert client.poll() is None, 'scroll.cli_rejected_early'
            click('创建终端')
            client.communicate(timeout=12)
            assert client.returncode == 0
        finally:
            if client.poll() is None:
                client.terminate()
                client.communicate(timeout=3)
    wait(lambda: native('terminal_input_ready') > before and pid() != first_pid, 'cli_new_pty')
    second_pid = pid()
    second = start_worker()
    scroll_up()
    type_key(second, 'x', '78')
    assert visible(returned), 'scroll.cli_did_not_inherit_input_false'
    emit(second, 1)
    wait(lambda: not visible(returned), 'cli_inherits_output_true')
    assert config.read_bytes() == explicit_document
    checks.append('new_cli_session_inherits_explicit_values')
    settings()
    edit('scroll_on_output', '新输出时回到底部')
    edit('scroll_on_input', '输入时回到底部')
    inherited_document = config.read_bytes()
    close_settings()
    scroll_up()
    type_key(second, 'y', '7879')
    assert visible(returned), 'scroll.reset_changed_mounted_surface'
    emit(second, 2)
    wait(lambda: not visible(returned), 'mounted_output_still_explicit')
    refresh()
    scroll_up()
    emit(second, 3)
    stays_reading('scroll.inherited_output_moved_viewport')
    type_key(second, 'z', '78797a')
    wait(lambda: not visible(returned), 'inherited_input_bottom')
    assert config.read_bytes() == inherited_document
    checks.append('reset_only_removes_overrides_and_refresh_restores_defaults')
    (second / 'finish').touch()
    wait(lambda: (second / 'finished').exists(), 'second_worker_done')
    assert received(first) == '616263' and received(second) == '78797a', 'scroll.extra_pty_input'
    run('import', '-window', xid, OUT / 'restored.png')
    RESULT.write_text(json.dumps({'version': 1, 'passed': False, 'checksPassed': True,
        'outputObservation': 'same-packet BEL parsed; negative output position checked beyond the existing one-second draw deadline',
        'nativeExitConfirmed': False, 'checks': checks, 'nativePids': [first_pid, second_pid],
        'rawPtyBytesHex': ['616263', '78797a'], 'samePtyAcrossRefresh': True,
        'limitations': ['Linux Xvfb software rendering; no SSH, system IME, physical multi-monitor or cross-platform acceptance']}, indent=2) + '\n')


def finalize(fixture):
    receipt = json.loads(RESULT.read_text())
    assert receipt['checksPassed'] is True
    pids = list(map(int, (fixture / 'native-pids').read_text().split()))
    assert pids == receipt['nativePids'] and len(set(pids)) == 2
    for pid, expected in zip(pids, receipt['rawPtyBytesHex'], strict=True):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            pass
        else:
            raise AssertionError('scroll.native_still_alive')
        final = json.loads((fixture / ('scroll-' + str(pid)) / 'input.json').read_text())
        assert final['hex'] == expected
        (OUT / ('input-' + str(pid) + '.json')).write_text(json.dumps(final) + '\n')
    log = Path('target/probes/workspace-settings-native.log').read_text()
    lines = log.splitlines()
    assert lines.count('workspace.session_opened') == 2
    assert lines.count('workspace.native_drained') == lines.count('workspace.exited') == 1
    assert 'workspace.terminal_failure.' not in log
    (OUT / 'native.log').write_text(log)
    receipt.update(passed=True, nativeExitConfirmed=True, guiExitCode=0)
    RESULT.write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__':
    if sys.argv[1] == 'worker':
        worker(sys.argv[2])
    elif sys.argv[1] == 'finalize':
        finalize(Path(sys.argv[2]))
    else:
        try:
            verify(Path(sys.argv[1]), sys.argv[2], sys.argv[3], Path(sys.argv[4]), Path(sys.argv[5]))
        except BaseException:
            OUT.mkdir(parents=True, exist_ok=True)
            subprocess.run(['import', '-window', sys.argv[3], str(OUT / 'failure.png')], check=False, timeout=4)
            raise
