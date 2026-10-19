#!/usr/bin/env python3
"""System terminal palette acceptance, hosted by the existing native portal fixture."""
import json
import os
from pathlib import Path
import re
import select
import signal
import sys
import termios
import time
import tty


def worker(directory):
    base = Path(directory) / ('palette-' + str(os.getpid()))
    base.mkdir(mode=0o700)
    received = bytearray()
    old = termios.tcgetattr(0)
    deadline = time.monotonic() + 180

    def publish(name, value):
        pending = base / (name + '.pending')
        pending.write_text(json.dumps(value) + '\n')
        pending.replace(base / (name + '.json'))

    def stop(_number, _frame):
        raise SystemExit(0)

    signal.signal(signal.SIGHUP, stop)
    signal.signal(signal.SIGTERM, stop)
    try:
        tty.setraw(0)
        os.write(1, b'\x1b[0mPALETTE_READY_ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n')
        publish('ready', dict(pid=os.getpid()))
        round_number = 0
        while not (base / 'finish').exists():
            assert time.monotonic() < deadline, 'palette.worker_timeout'
            if (base / ('query-' + str(round_number + 1))).exists():
                round_number += 1
                os.write(1, b'\x1b]10;?\x07\x1b]11;?\x07')
            if select.select([0], [], [], .02)[0]:
                chunk = os.read(0, 2048)
                assert chunk, 'palette.worker_eof'
                received.extend(chunk)
                assert len(received) <= 4096, 'palette.unexpected_input_budget'
                publish('input', dict(pid=os.getpid(), round=round_number, hex=received.hex()))
    finally:
        termios.tcsetattr(0, termios.TCSANOW, old)
        publish('finished', dict(pid=os.getpid(), hex=received.hex()))


def verify(c):
    out = c['OUTPUT']
    fixture = c['fixture']
    run, wait, key, click = (c[name] for name in ('run', 'wait', 'key', 'click'))
    workers = []
    observations = []
    palette_colors = {'paper': ('1f2937', 'fafaf9'), 'midnight': ('e2e8f0', '0f172a'), 'classic': ('ffffff', '000000')}
    status = '已保存 · 用于新建本地或 SSH 终端；已有终端保持原配色'

    def app():
        return c['current_app']()

    def accessible(name, action='activate'):
        return c['access'](name, action)

    def combo(label, index):
        wait(lambda: accessible(label, 'bounds-combo'), 'combo_' + label)
        left, top, width, height = map(int, run(c['helper'], app().pid, label, 'bounds-combo').stdout.split())
        run('/usr/bin/xdotool', 'mousemove', left + width // 2, top + height // 2, 'click', '1')
        wait(lambda: 'expanded=1' in run(c['helper'], app().pid, label, 'inspect-combo').stdout, 'expanded_' + label)
        key('Home')
        for _ in range(index):
            key('Down')
        key('Return')
        wait(lambda: 'expanded=0' in run(c['helper'], app().pid, label, 'inspect-combo').stdout, 'collapsed_' + label)

    def strategy(reset=False):
        c['setting']()
        wait(lambda: accessible('搜索设置', 'focus'), 'theme_search')
        key('ctrl+a')
        run('/usr/bin/xdotool', 'type', '--clearmodifiers', '--delay', '10', 'terminal_theme')
        wait(lambda: accessible('终端配色方式', 'bounds-combo'), 'terminal_theme_field')
        before = c['config'].read_bytes() if c['config'].exists() else b''
        if not reset:
            combo('终端配色方式', 1)
            combo('系统浅色主题', 2)
            combo('系统深色主题', 1)
            assert (c['config'].read_bytes() if c['config'].exists() else b'') == before, 'palette.draft_saved'
        saves = c['count']('settings_save_confirmed')
        click('恢复继承' if reset else '保存')
        wait(lambda: c['count']('settings_save_confirmed') == saves + 1 and accessible(status, 'bounds'), 'theme_save')
        terminal = c['tomllib'].loads(c['config'].read_text()).get('terminal', {})
        if reset:
            assert 'theme' not in terminal, 'palette.reset_kept_override'
        else:
            assert terminal['theme'] == dict(mode='system', light='paper', dark='midnight'), 'palette.wrong_policy'
        key('Escape')
        wait(lambda: not accessible('搜索设置', 'focus'), 'settings_closed')

    def start():
        before = c['count']('session_opened')
        click('POSIX sh')
        wait(lambda: c['count']('session_opened') == before + 1, 'session_opened')
        wait(lambda: c['count']('terminal_input_ready') >= before + 1, 'input_ready')
        found = wait(lambda: [path for path in fixture.glob('palette-*') if path.is_dir() and path not in [item['base'] for item in workers] and (path / 'ready.json').exists()], 'worker_ready')
        assert len(found) == 1, 'palette.extra_worker'
        entry = dict(base=found[0], pid=json.loads((found[0] / 'ready.json').read_text())['pid'], round=0)
        workers.append(entry)
        return entry

    def assert_palette(entry, preset, tag):
        entry['round'] += 1
        (entry['base'] / ('query-' + str(entry['round']))).touch()
        fg, bg = palette_colors[preset]
        def response(code, color):
            rgb = '/'.join(color[index:index+2] * 2 for index in range(0, 6, 2))
            return ('\x1b]' + str(code) + ';rgb:' + rgb + '\x07').encode()
        expected = (response(10, fg) + response(11, bg)) * entry['round']
        def received():
            path = entry['base'] / 'input.json'
            if not path.exists():
                return False
            value = json.loads(path.read_text())
            (out / (tag + '-response.json')).write_text(json.dumps(value, indent=2) + '\n')
            return value['round'] == entry['round'] and bytes.fromhex(value['hex']) == expected
        wait(received, 'osc_palette_' + tag)
        image = out / (tag + '.png')
        def pixels():
            run('/usr/bin/import', '-window', c['xid'], image)
            histogram = run('/usr/bin/magick', image, '-crop', '600x60+220+150', '+repage', '-depth', '8', '-format', '%c', 'histogram:info:').stdout
            counts = {color.lower(): int(count) for count, color in re.findall(r'^\s*(\d+):.*?#([0-9a-fA-F]{6})(?:\s|$)', histogram, re.M)}
            return counts.get(bg, 0) > 10000 and counts.get(fg, 0) > 5
        wait(pixels, 'paint_' + tag)
        observations.append(dict(tag=tag, preset=preset, foreground=fg, background=bg, workerPid=entry['pid'], round=entry['round'], inputHex=expected.hex(), image=image.name))

    def refresh(entry, preset, tag):
        opened = c['count']('session_opened')
        before = c['count']('terminal_input_ready')
        key('ctrl+shift+r')
        wait(lambda: c['count']('terminal_input_ready') == before + 1, 'refresh_input')
        assert c['count']('session_opened') == opened, 'palette.refresh_created_session'
        assert_palette(entry, preset, tag)

    def finish_window():
        for entry in workers:
            if not (entry['base'] / 'finished.json').exists():
                (entry['base'] / 'finish').touch()
                wait(lambda: (entry['base'] / 'finished.json').exists() and not Path('/proc/' + str(entry['pid'])).exists(), 'worker_exit')
            expected = next(item['inputHex'] for item in reversed(observations) if item['workerPid'] == entry['pid'])
            final = json.loads((entry['base'] / 'finished.json').read_text())
            assert final['hex'] == expected, 'palette.extra_final_input'
            (out / ('input-final-' + str(entry['pid']) + '.json')).write_text(json.dumps(final, indent=2) + '\n')
        c['expected_sessions'] = c['count']('session_opened')
        c['close']()
        c['windows_exited'] += 1
        (out / ('native-window-' + str(c['windows_exited']) + '.log')).write_bytes(c['app_log'].read_bytes())

    c['launch']()
    c['xid'] = c['find_window']()
    c['paint']('light', 'ui-light')
    c['choose']('light')
    strategy()
    light = start()
    assert_palette(light, 'paper', 'light-new')
    c['change']('dark')
    c['paint']('light', 'ui-stays-light-system-dark')
    assert_palette(light, 'paper', 'light-keeps-after-system-dark')
    refresh(light, 'paper', 'light-keeps-after-refresh')
    c['checks'].append('system_light_new_session_and_frozen_after_native_dark_and_refresh')
    dark = start()
    assert_palette(dark, 'midnight', 'dark-new-with-light-ui')
    strategy(reset=True)
    assert_palette(dark, 'midnight', 'dark-keeps-after-reset')
    refresh(dark, 'midnight', 'dark-keeps-after-reset-refresh')
    fixed = start()
    assert_palette(fixed, 'classic', 'fixed-new-after-reset')
    c['checks'].append('native_system_dark_is_independent_of_light_ui_and_reset_affects_only_new_sessions')
    strategy()
    assert_palette(fixed, 'classic', 'fixed-keeps-after-system-policy-restored')
    finish_window()
    c['launch']()
    c['xid'] = c['find_window']()
    assert c['persisted']() == 'light'
    reopened = start()
    assert_palette(reopened, 'midnight', 'reopened-system-dark')
    c['change']('light')
    assert_palette(reopened, 'midnight', 'reopened-keeps-after-system-light')
    last = start()
    assert_palette(last, 'paper', 'new-after-system-light')
    finish_window()
    c['checks'].append('saved_policy_survives_independent_reopen_and_new_sessions_follow_both_native_directions')
    assert len(workers) == 5 and len({entry['pid'] for entry in workers}) == 5
    c['receipt'].update(passed=True, windowsExited=c['windows_exited'], nativeSessionsStarted=5,
        nativeExitConfirmed=True, paletteObservations=observations, exactFinalInputVerified=True,
        workerPids=[entry['pid'] for entry in workers], currentUiMode='light')


if __name__ == '__main__':
    assert len(sys.argv) == 3 and sys.argv[1] == '--worker'
    worker(sys.argv[2])
