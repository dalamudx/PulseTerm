#!/usr/bin/env python3
"""Native Settings portal -> Tao/GTK -> production WebView appearance acceptance.

Runs only inside the task's private Xvfb/D-Bus session. No DOM injection or media emulation.
"""
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import time
import tomllib

TERMINAL_THEME = os.environ.get("PULSE_TERMINAL_SYSTEM_THEME") == "1"
OUTPUT = Path('target/probes/workspace-terminal-system-theme' if TERMINAL_THEME else 'target/probes/workspace-system-appearance')
OUTPUT.mkdir(parents=True, exist_ok=True)
checks = []

def run(*args):
    return subprocess.run(list(map(str, args)), check=True, capture_output=True,
                          text=True, timeout=4)

def wait(predicate, name, seconds=12):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.08)
    raise AssertionError('appearance.timeout.' + name)

def interrupted(_number, _frame):
    raise SystemExit(143)

signal.signal(signal.SIGTERM, interrupted)

with tempfile.TemporaryDirectory(prefix='pt-appearance.', dir='/data/temp') as temporary:
    fixture = Path(temporary)
    environment = os.environ.copy()
    for environment_key, name in [('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        directory = fixture / name
        directory.mkdir(mode=0o700)
        environment[environment_key] = str(directory)
    shell_dir = fixture / 'bin'
    shell_dir.mkdir(mode=0o700)
    shell = shell_dir / 'sh'
    shell.write_text('#!/bin/sh\nexit 0\n')
    if TERMINAL_THEME:
        shell.write_text('#!/bin/sh\nexec /usr/bin/python3 "$PULSE_TERMINAL_THEME_SCRIPT" --worker "$PULSE_TERMINAL_THEME_FIXTURE"\n')
        environment.update(PULSE_TERMINAL_THEME_SCRIPT=str(Path('scripts/probe-workspace-terminal-system-theme.py').resolve()), PULSE_TERMINAL_THEME_FIXTURE=str(fixture))
    shell.chmod(0o700)
    environment.update(PATH=str(shell_dir), SHELL=str(shell),
                       PULSE_APPEARANCE_FIXTURE='1')
    config = fixture / 'config/dev.pulseterm.workspace-preview/settings.toml'
    helper = os.environ['PULSE_NATIVE_HELPER']
    app: subprocess.Popen[str] | None = None
    provider: subprocess.Popen[str] | None = None
    xid = None
    windows_exited = 0
    app_log = OUTPUT / 'native.log'
    portal_log = OUTPUT / 'portal.log'
    receipt = dict(version=1, passed=False,
                   kind='linux-production-webview-controlled-desktop-settings-portal',
                   checks=checks,
                   limitations=['Isolated Settings portal provider; no GNOME/KDE desktop session acceptance',
                                'No terminal palette, external monitor or cross-platform acceptance'])
    expected_sessions = 0
    if TERMINAL_THEME:
        receipt.update(version=3, kind='native-system-terminal-theme-over-real-pty', limitations=['Isolated desktop portal, not GNOME/KDE or cross-platform', 'Local GUI and warm CLI; SSH, cold CLI, fixed Profile/group overrides remain separate'])
    def current_app():
        assert app is not None, 'appearance.app_missing'
        return app
    def count(event):
        return app_log.read_text().splitlines().count('workspace.' + event)
    def access(name, action='activate'):
        result = subprocess.run([helper, str(current_app().pid), name, action],
                                capture_output=True, text=True, timeout=4)
        return result.returncode == 0
    def click(name):
        wait(lambda: access(name), 'activate_' + name)
    def key(*names):
        run('/usr/bin/xdotool', 'key', '--clearmodifiers', *names)
    def pixels():
        run('/usr/bin/import', '-window', xid, str(fixture / 'current.png'))
        geometry = run('/usr/bin/xdotool', 'getwindowgeometry', '--shell', xid).stdout
        width_match = re.search(r'^WIDTH=(\d+)$', geometry, re.M)
        height_match = re.search(r'^HEIGHT=(\d+)$', geometry, re.M)
        assert width_match is not None and height_match is not None, 'appearance.window_geometry'
        width = int(width_match[1])
        height = int(height_match[1])
        value = run('/usr/bin/magick', str(fixture / 'current.png'), '-crop',
                    f'8x8+{width-24}+{height-24}', '+repage', '-format',
                    '%[fx:mean]', 'info:').stdout
        return float(value)
    def paint(mode, tag):
        def matches():
            value = pixels()
            return value > .7 if mode == 'light' else value < .3
        wait(matches, 'paint_' + tag)
        run('/usr/bin/import', '-window', xid, str(OUTPUT / (tag + '.png')))
    def persisted():
        return tomllib.loads(config.read_text()).get('ui', {}).get('appearance') if config.exists() else None
    def setting():
        before = count('settings_read_confirmed')
        click('设置')
        wait(lambda: count('settings_read_confirmed') > before, 'settings_read')
        wait(lambda: access('搜索设置', 'focus'), 'settings_search')
        key('ctrl+a')
        run('/usr/bin/xdotool', 'type', '--clearmodifiers', '--delay', '30', 'theme')
        wait(lambda: access('界面外观', 'bounds-combo'), 'appearance_field')
    def choose(mode):
        setting()
        previous = config.read_bytes() if config.exists() else b''
        before_paint = pixels()
        bounds = list(map(int, run(helper, current_app().pid, '界面外观', 'bounds-combo').stdout.split()))
        assert len(bounds) == 4, 'appearance.field_bounds'
        left, top, width, height = bounds
        run('/usr/bin/xdotool', 'mousemove', left + width // 2, top + height // 2, 'click', '1')
        wait(lambda: 'expanded=1' in run(helper, current_app().pid, '界面外观', 'inspect-combo').stdout,
             'appearance_menu_open')
        key('Home')
        for _ in range(['light', 'dark', 'system'].index(mode)):
            key('Down')
        key('Return')
        wait(lambda: 'expanded=0' in run(helper, current_app().pid, '界面外观', 'inspect-combo').stdout,
             'appearance_menu_closed')
        assert (config.read_bytes() if config.exists() else b'') == previous, 'appearance.draft_wrote'
        assert abs(pixels() - before_paint) < .015, 'appearance.draft_changed_workspace'
        saves = count('settings_save_confirmed')
        click('保存界面外观')
        wait(lambda: count('settings_save_confirmed') == saves + 1 and persisted() == mode,
             'save_' + mode)
        key('Escape')
    def change(mode):
        scheme = 1 if mode == 'dark' else 2
        before = portal_log.read_text().splitlines().count(f'fixture.system_appearance.changed.{scheme}')
        assert provider is not None and provider.stdin is not None, 'appearance.provider_missing'
        provider.stdin.write(mode + '\n')
        provider.stdin.flush()
        wait(lambda: portal_log.read_text().splitlines().count(
            f'fixture.system_appearance.changed.{scheme}') == before + 1, 'portal_' + mode)
    def launch():
        global app
        with app_log.open('w') as log:
            app = subprocess.Popen([os.environ['PULSE_WORKSPACE_EXECUTABLE']], env=environment,
                                   stdout=log, stderr=subprocess.STDOUT,
                                   text=True, start_new_session=True)
        def ready():
            assert current_app().poll() is None, 'appearance.app_exited_before_ready'
            return 'workspace.profiles_listed' in app_log.read_text()
        wait(ready, 'profiles', 30)
        return app
    def find_window():
        def selected():
            result = subprocess.run(['/usr/bin/xdotool', 'search', '--onlyvisible', '--all',
                '--pid', str(current_app().pid), '--name', '^PulseTerm · 工作区预览$'],
                capture_output=True, text=True, timeout=4)
            found = result.stdout.split()
            return found[0] if len(found) == 1 else None
        window = wait(selected, 'window', 20)
        assert int(run('/usr/bin/xdotool', 'getwindowpid', window).stdout) == current_app().pid
        run('/usr/bin/xdotool', 'windowactivate', '--sync', window)
        return window
    def close():
        run('/usr/bin/xdotool', 'windowactivate', '--sync', xid)
        key('alt+F4')
        assert current_app().wait(timeout=15) == 0, 'appearance.app_exit'
        assert count('exited') == 1, 'appearance.exit_barrier'
        assert count('session_opened') == expected_sessions, 'appearance.unexpected_session'
        def group_exited():
            try:
                os.killpg(current_app().pid, 0)
            except ProcessLookupError:
                return True
            return False
        wait(group_exited, 'native_process_group_exit', 8)
    try:
        with portal_log.open('w') as log:
            provider = subprocess.Popen([os.environ['PULSE_APPEARANCE_HELPER']], env=environment,
                stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT,
                text=True, start_new_session=True)
        wait(lambda: 'fixture.system_appearance.ready' in portal_log.read_text(), 'portal_ready')
        if TERMINAL_THEME:
            import runpy
            runpy.run_path(str(Path('scripts/probe-workspace-terminal-system-theme.py')))['verify'](globals())
        else:
            app = launch()
            xid = find_window()
            paint('light', 'default-light')
            choose('system')
            change('dark')
            paint('dark', 'system-dark')
            change('light')
            paint('light', 'system-light')
            checks.append('real_portal_signal_updates_production_webview_both_directions')
            change('dark')
            paint('dark', 'system-dark-before-override')
            choose('light')
            paint('light', 'explicit-light')
            change('light')
            change('dark')
            paint('light', 'explicit-light-stays')
            choose('dark')
            change('light')
            paint('dark', 'explicit-dark-stays')
            checks.append('explicit_modes_ignore_system_changes_and_drafts_do_not_apply')
            choose('system')
            paint('light', 'follow-restored')
            change('dark')
            paint('dark', 'follow-dark-again')
            before = count('document_confirmed')
            key('ctrl+shift+r')
            wait(lambda: count('document_confirmed') == before + 1, 'refresh')
            paint('dark', 'refreshed-dark')
            assert persisted() == 'system'
            checks.append('saved_follow_system_survives_refresh')
            close()
            windows_exited += 1
            (OUTPUT / 'first-native.log').write_bytes(app_log.read_bytes())
            app = launch()
            xid = find_window()
            paint('dark', 'reopened-dark')
            checks.append('independent_reopen_reads_saved_mode_and_current_native_preference')
            setting()
            saves = count('settings_save_confirmed')
            click('界面外观恢复继承')
            wait(lambda: count('settings_save_confirmed') == saves + 1 and persisted() is None,
                 'reset_override')
            key('Escape')
            paint('light', 'reset-default-light')
            change('light')
            change('dark')
            paint('light', 'reset-ignores-system')
            checks.append('reset_removes_override_and_restores_default_light')
            close()
            windows_exited += 1
            receipt.update(passed=True, windowsExited=windows_exited, nativeSessionsStarted=0,
                           nativeExitConfirmed=True)
    except BaseException as error:
        receipt.update(error=type(error).__name__ + ': ' + str(error), windowsExited=windows_exited)
        if xid is not None:
            subprocess.run(['/usr/bin/import', '-window', str(xid), str(OUTPUT / 'failure.png')],
                           capture_output=True, timeout=4)
        raise
    finally:
        for process in [app, provider]:
            if process is None:
                continue
            if process is provider and process.poll() is None:
                assert process.stdin is not None, 'appearance.provider_pipe_missing'
                process.stdin.write('quit\n')
                process.stdin.flush()
            if process.poll() is None:
                try:
                    if process is app:
                        os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        receipt['providerExited'] = provider is not None and provider.returncode == 0
        if not receipt['providerExited']:
            receipt['passed'] = False
        (OUTPUT / 'result.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n')
    assert receipt['passed'], 'appearance.final_receipt_failed'
    print('workspace.system_appearance_signal_explicit_save_refresh_reopen_reset_and_exit_confirmed')
