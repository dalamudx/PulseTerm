#!/usr/bin/env python3
"""Native WebView history stop over actual 100000 retained PTY rows; no mocked IPC."""
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import termios
import time
import tty

OUT = Path('target/probes/workspace-history-stop')
RESULT = OUT / 'result.json'


def worker(directory):
    fixture = Path(directory)
    received = bytearray()
    old = termios.tcgetattr(0)
    deadline = time.monotonic() + 100
    try:
        tty.setraw(0)
        for start in range(0, 100100, 1000):
            data = ''.join(f'HS{index:06d}\r\n' for index in range(start, min(start + 1000, 100100))).encode()
            while data:
                count = os.write(1, data)
                assert count > 0
                data = data[count:]
        os.write(1, b'HS_OUTPUT_READY\r\n')
        (fixture / 'history-stop-emitted').touch()
        while not (fixture / 'history-stop-finish').exists():
            assert time.monotonic() < deadline, 'history_stop.worker_deadline'
            if select.select([0], [], [], .02)[0]:
                received.extend(os.read(0, 256))
            temporary = fixture / 'history-stop-input.pending'
            temporary.write_text(json.dumps({'hex': received.hex()}))
            temporary.replace(fixture / 'history-stop-input.json')
    finally:
        termios.tcsetattr(0, termios.TCSANOW, old)
    (fixture / 'history-stop-input.json').write_text(json.dumps({'hex': received.hex()}))
    (fixture / 'history-stop-finished').touch()


def verify(fixture, app, xid, native_log):
    OUT.mkdir(parents=True, exist_ok=True)
    RESULT.write_text(json.dumps({'version': 1, 'passed': False, 'nativeExitConfirmed': False}) + '\n')
    helper = os.environ['PULSE_NATIVE_HELPER']

    def run(*args, check=True):
        return subprocess.run(list(map(str, args)), check=check, capture_output=True, text=True, timeout=4)

    def xdo(*args):
        return run('/usr/bin/xdotool', *args)

    def accessible(name, action='activate'):
        return run(helper, app, name, action).stdout.strip()

    def wait(predicate, label, seconds=12):
        deadline = time.monotonic() + seconds
        while True:
            try:
                result = predicate()
                if result:
                    return result
            except subprocess.CalledProcessError:
                pass  # Readiness only; effects are invoked outside this loop.
            assert time.monotonic() < deadline, 'history_stop.timeout.' + label
            time.sleep(.02)

    def count(event):
        return native_log.read_text().splitlines().count('workspace.' + event)

    def status():
        return accessible('历史分页状态', 'readonly-text')

    def page():
        return accessible('当前页历史文本', 'readonly-text')

    def open_history():
        accessible('查看当前窗格历史与查找')
        wait(lambda: run(helper, app, '查找历史与主屏', check=False).returncode == 0, 'dialog')

    def text(query):
        accessible('查找历史与主屏', 'focus')
        xdo('key', '--clearmodifiers', 'ctrl+a')
        xdo('type', '--clearmodifiers', '--delay', '1', query)

    def find(query, expected):
        text(query)
        xdo('key', '--clearmodifiers', 'Return')
        started = time.monotonic()
        message = wait(lambda: value if expected in (value := status()) else None, 'find_' + query)
        assert query in page(), 'history_stop.matched_page_missing'
        return {'query': query, 'observedMs': (time.monotonic()-started)*1000, 'status': message}

    original_pid = int((fixture / 'native.pid').read_text())
    sessions = count('session_opened')
    xdo('mousemove', '--window', xid, '400', '300', 'click', '1')
    xdo('type', '--clearmodifiers', '--delay', '1', 'ALPHA_STOP_HISTORY')
    xdo('key', '--clearmodifiers', 'Return')
    wait(lambda: (fixture / 'history-stop-emitted').exists(), 'worker_output', 60)
    open_history()

    def retained():
        message = status()
        match = re.search(r'共保留 (\d+) 行', message)
        if match and int(match[1]) == 100000:
            return message
        accessible('最新历史')
        return None

    retained_status = wait(retained, '100000_retained', 60)
    initial = find('HS001000', '匹配历史')
    input_before = (fixture / 'input').read_bytes()
    stops = []
    for backwards in (False, True):
        text('HS_NEVER_PRESENT')
        before = page()
        xdo('key', '--clearmodifiers', 'shift+Return' if backwards else 'Return')
        started = time.monotonic()
        accessible('停止查找')
        stopped = wait(lambda: value if (value := status()) == '已停止查找，当前页已保留。' else None, 'stopped')
        elapsed = (time.monotonic()-started)*1000
        assert page() == before, 'history_stop.changed_current_page'
        assert 'enabled=1' in accessible('下一个', 'inspect'), 'history_stop.next_not_readmitted'
        assert run(helper, app, '停止查找', check=False).returncode != 0, 'history_stop.button_still_visible'
        assert status() == stopped and page() == before, 'history_stop.late_update'
        assert (fixture / 'input').read_bytes() == input_before, 'history_stop.terminal_input'
        assert json.loads((fixture / 'history-stop-input.json').read_text())['hex'] == '', 'history_stop.raw_terminal_input'
        stops.append({'backwards': backwards, 'observedStopMs': elapsed, 'pagePreserved': True})
        find('HS090000', '匹配历史')
    run('/usr/bin/import', '-window', xid, OUT / 'live.png')
    xdo('key', '--clearmodifiers', 'Escape')
    (fixture / 'history-stop-finish').touch()
    wait(lambda: (fixture / 'history-stop-finished').exists(), 'worker_done')
    assert json.loads((fixture / 'history-stop-input.json').read_text())['hex'] == '', 'history_stop.final_raw_terminal_input'
    assert (fixture / 'input').read_bytes() == input_before, 'history_stop.final_terminal_input'
    run('/usr/bin/cp', fixture / 'history-stop-input.json', OUT / 'worker-input.json')
    xdo('mousemove', '--window', xid, '400', '300', 'click', '1')
    xdo('type', '--clearmodifiers', '--delay', '1', 'ALPHA_FINISH')
    xdo('key', '--clearmodifiers', 'Return')

    def ended():
        try:
            os.kill(original_pid, 0)
            return False
        except ProcessLookupError:
            return True

    wait(ended, 'native_eof')
    open_history()
    eof = find('HS090000', '匹配历史')
    assert count('session_opened') == sessions, 'history_stop.reconnected'
    run('/usr/bin/import', '-window', xid, OUT / 'ended.png')
    xdo('key', '--clearmodifiers', 'Escape')
    RESULT.write_text(json.dumps({'version': 1, 'passed': False, 'checksPassed': True,
        'nativeExitConfirmed': False, 'nativePid': original_pid, 'retainedRows': 100000,
        'retainedStatus': retained_status, 'initial': initial, 'stops': stops, 'ended': eof,
        'zeroTerminalInputDuringSearch': True, 'sameSession': True,
        'limitations': ['Linux Xvfb software rendering; stop timings include AT-SPI observation',
                        'No native read delay injected; in-flight overlap boundaries also covered by unit tests',
                        'No IME, hardware GPU or cross-platform acceptance']}, indent=2)+'\n')


def finalize(fixture):
    receipt = json.loads(RESULT.read_text())
    assert receipt.get('checksPassed') is True
    pids = list(map(int, (fixture / 'native-pids').read_text().split()))
    assert pids == [receipt['nativePid']]
    for pid in pids:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            continue
        raise AssertionError('history_stop.native_still_alive')
    receipt.update(passed=True, nativeExitConfirmed=True)
    RESULT.write_text(json.dumps(receipt, indent=2)+'\n')
    print('workspace.history_stop_100000_forward_backward_eof_zero_input_native_exit_confirmed')


if __name__ == '__main__':
    if sys.argv[1] == 'worker':
        worker(sys.argv[2])
    elif sys.argv[1] == 'finalize':
        finalize(Path(sys.argv[2]))
    else:
        try:
            verify(Path(sys.argv[1]), sys.argv[2], sys.argv[3], Path(sys.argv[4]))
        except BaseException:
            OUT.mkdir(parents=True, exist_ok=True)
            subprocess.run(['/usr/bin/import', '-window', sys.argv[3], str(OUT / 'failure.png')], check=False, timeout=4)
            raise
