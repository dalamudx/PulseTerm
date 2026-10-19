#!/usr/bin/env python3
"""Real saved SSH settings inheritance; invoked by the isolated cold-CLI fixture."""
import json
import re
import sqlite3
import subprocess
import time
import tomllib


def verify(s):
    evidence, prefix, database = s['evidence'], s['prefix'], s['database']
    wait, accessible, xdo = s['wait'], s['accessible'], s['xdo']
    events, auth = s['event_count'], s['auth_count']
    allowed = {'catalogue.organization.v1', 'catalogue.recent-ssh-hosts.v1',
               'settings.host.' + s['host'] + '.v1'}

    def source_rows():
        with sqlite3.connect('file:' + str(database) + '?mode=ro', uri=True) as db:
            result = {}
            for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
                quoted = table.replace('"', '""')
                rows = db.execute('SELECT * FROM "' + quoted + '"').fetchall()
                result[table] = sorted((row for row in rows if table != 'app_state' or row[0] not in allowed), key=repr)
            return result

    palette_observations = []
    def change_system(mode):
        before_auth = auth()
        before_input = events('fixture.ssh_scope_input_received')
        scheme = 1 if mode == 'dark' else 2
        portal_log = evidence / (prefix + '-system-portal.log')
        marker = 'fixture.system_appearance.changed.' + str(scheme)
        before = portal_log.read_text().splitlines().count(marker)
        provider = s['palette_provider']
        assert provider.poll() is None and provider.stdin is not None, 'scope.portal_unavailable'
        provider.stdin.write(mode + '\n')
        provider.stdin.flush()
        wait(lambda: portal_log.read_text().splitlines().count(marker) == before + 1, 'system_' + mode)
        assert auth() == before_auth and events('fixture.ssh_scope_input_received') == before_input, 'scope.signal_sent_input_or_authenticated'

    def palette_pixels(session, palette, tag):
        focus(session)
        fg, bg = {'paper': ('1f2937', 'fafaf9'), 'classic': ('ffffff', '000000')}[palette]
        image = evidence / (prefix + '-' + tag + '.png')
        def drawn():
            s['run'](['/usr/bin/import', '-window', s['xid'], image])
            histogram = s['run'](['/usr/bin/magick', image, '-crop', '600x60+220+150', '+repage',
                                  '-depth', '8', '-format', '%c', 'histogram:info:']).stdout
            colors = {color.lower(): int(count) for count, color in re.findall(
                r'^\s*(\d+):.*?#([0-9a-fA-F]{6})(?:\s|$)', histogram, re.M)}
            return colors.get(bg, 0) > 10000 and colors.get(fg, 0) > 5
        wait(drawn, 'system_palette_pixels_' + tag)
        palette_observations.append(dict(session=session, preset=palette, tag=tag, foreground=fg, background=bg, image=image.name))

    original = source_rows()

    def document(key):
        with sqlite3.connect('file:' + str(database) + '?mode=ro', uri=True) as db:
            row = db.execute('SELECT value FROM app_state WHERE key=?', (key,)).fetchone()
        return json.loads(row[0]) if row else None

    def key(*values):
        xdo('key', '--clearmodifiers', *values)

    def reveal(caption, action):
        if accessible(caption, action).returncode == 0:
            return
        sidebar = caption == '分组、标签与收藏'
        for direction in ('5', '4'):
            for _ in range(20):
                xdo('mousemove', '--window', s['xid'], '90' if sidebar else '650', '400', 'click', direction)
                time.sleep(.04)
                if accessible(caption, action).returncode == 0:
                    return
        wait(lambda: accessible(caption, action).returncode == 0, 'reveal_' + caption)

    def click(caption):
        reveal(caption, 'activate')

    def fill(caption, value):
        reveal(caption, 'focus')
        key('ctrl+a')
        xdo('type', '--clearmodifiers', '--delay', '1', value)

    def expanded(caption, value):
        result = accessible(caption, 'inspect-combo')
        return result.returncode == 0 and 'expanded=' + str(value) in result.stdout

    def select(caption, index):
        reveal(caption, 'focus')
        key('space')
        wait(lambda: expanded(caption, 1), 'expanded_' + caption)
        if caption == '编辑分组':
            # Exactly two fixture choices; each change replaces the form and closes its menu.
            assert index in (0, 1)
            key('End' if index else 'Home')
        else:
            key('Home')
            for _ in range(index):
                key('Down')
        key('Return')
        wait(lambda: expanded(caption, 0), 'selected_' + caption)

    def cli_connect(tag, term, first=False):
        before = auth()
        term_event = 'fixture.ssh_scope_term.' + term
        before_term = events(term_event)
        if first:
            s['app'] = s['launch']('global')
            s['unlock_without_confirming']()
            click('查看目标')
        else:
            s['client'] = subprocess.Popen([str(s['pulse']), 'ssh', s['host'], '--identity', s['identity']],
                                           env=s['env'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        wait(lambda: accessible('允许 CLI 连接保存的 SSH 主机？').returncode == 0, 'target_' + tag)
        assert auth() == before, 'scope.authentication_before_target_approval'
        click('连接')
        s['client_result'](tag, True)
        wait(lambda: auth() == before + 1, 'authenticated_' + tag)
        wait(lambda: events(term_event) == before_term + 1, 'remote_pty_term_' + tag)
        output = (evidence / (prefix + '-' + tag + '-client.log')).read_text()
        session = re.search(r'^session ([1-9][0-9]*)$', output, re.M)
        assert session is not None, 'scope.missing_native_session'
        return session.group(1)

    def terminal_ready():
        result = accessible('查看当前窗格历史与查找', 'inspect')
        return result.returncode == 0 and 'enabled=1' in result.stdout

    def focus(session):
        wait(terminal_ready, 'terminal_ready_before_selection')
        result = subprocess.run([str(s['pulse']), 'focus-tab', session], env=s['env'],
                                capture_output=True, text=True, timeout=8)
        assert result.returncode == 0, 'scope.focus_failed.' + result.stderr
        wait(terminal_ready, 'terminal_ready_after_selection')
        xdo('mousemove', '--window', s['xid'], '400', '300', 'click', '1')

    def verify_terminal(session, palette, encoding):
        focus(session)
        assert sum(events('fixture.ssh_scope_term.' + term) for term in
                   ('xterm-256color', 'xterm', 'vt100')) == len(sessions), 'scope.existing_session_requested_another_pty'
        color_event = 'fixture.ssh_scope_color.' + palette
        before = events(color_event)
        xdo('type', '--clearmodifiers', '--delay', '1', 'SCOPE_QUERY')
        key('Return')
        wait(lambda: events(color_event) == before + 1, 'remote_palette_' + palette)
        key_event = 'fixture.ssh_scope_keys.' + encoding
        before = events(key_event)
        key('BackSpace', 'Delete', 'Return')
        wait(lambda: events(key_event) == before + 1, 'remote_bytes_' + encoding)

    def asynchronous_scroll_settings(session, scroll_up, showing):
        control = s['directory'] / 'scope-scroll'
        config = s['root'] / 'config/dev.pulseterm.workspace-preview/settings.toml'
        bell = '清除终端响铃提示'
        checks = []
        wait(lambda: events('fixture.ssh_scope_async_started') == 1, 'async_output_started')

        def native_click(caption):
            reveal(caption, 'bounds')
            result = accessible(caption, 'bounds')
            assert result.returncode == 0
            left, top, width, height = map(int, result.stdout.split())
            xdo('mousemove', str(left + width // 2), str(top + height // 2), 'click', '1')

        def stays_reading(label):
            until = time.monotonic() + 1.1
            while time.monotonic() < until:
                assert showing(), label
                time.sleep(.03)

        def emit(index, reading):
            if accessible(bell).returncode == 0:
                native_click(bell)
                wait(lambda: accessible(bell).returncode != 0, 'previous_async_bell_cleared')
            received = events('fixture.ssh_scope_input_received')
            (control / ('emit-' + str(index))).touch()
            wait(lambda: events('fixture.ssh_scope_async_sent.' + str(index)) == 1, 'async_output_sent')
            wait(lambda: accessible(bell).returncode == 0, 'async_output_parsed')
            if reading:
                stays_reading('scroll.async_output_changed_reading_position')
            else:
                wait(lambda: not showing(), 'async_output_follows_latest')
            assert events('fixture.ssh_scope_input_received') == received, 'scroll.async_control_leaked_input'

        def change_settings(reset=False):
            native_click('设置')
            wait(lambda: accessible('搜索设置').returncode == 0, 'async_scroll_settings_open')
            for field, caption, value in [('scroll_on_output', '新输出时回到底部', True),
                                          ('scroll_on_input', '输入时回到底部', False)]:
                fill('搜索设置', field)
                reveal(caption, 'focus')
                before = config.read_bytes() if config.exists() else b''
                if not reset:
                    key('space')
                    assert (config.read_bytes() if config.exists() else b'') == before
                native_click(caption + '恢复继承' if reset else '保存' + caption)
                wait(lambda: (config.read_bytes() if config.exists() else b'') != before, 'async_scroll_persisted')
                wait(lambda: accessible('已保存 · 重新加载终端后应用；当前会话未关闭').returncode == 0,
                     'async_scroll_frontend_saved')
                stored = tomllib.loads(config.read_text()).get('terminal', {})
                assert field not in stored if reset else stored[field] is value
            key('Escape')
            wait(lambda: accessible('搜索设置').returncode != 0, 'async_scroll_settings_closed')

        def input_char(char, reading):
            received = events('fixture.ssh_scope_input_received')
            xdo('mousemove', '--window', s['xid'], '650', '350', 'click', '1')
            xdo('type', '--clearmodifiers', char)
            wait(lambda: events('fixture.ssh_scope_input_received') > received, 'async_scroll_partial_input_received')
            if reading:
                stays_reading('scroll.disabled_input_moved_viewport')
            else:
                wait(lambda: not showing(), 'async_scroll_input_returns_latest')

        def refresh():
            before_auth = auth()
            key('ctrl+shift+r')
            time.sleep(.5)
            wait(terminal_ready, 'async_scroll_refreshed_terminal')
            assert auth() == before_auth == 5
            assert sum(events('fixture.ssh_scope_term.' + term) for term in ('xterm-256color', 'xterm', 'vt100')) == 5

        scroll_up()
        emit(1, True)
        checks.append('default_background_output_preserves_reading')
        change_settings()
        scroll_up()
        input_char('p', False)
        scroll_up()
        emit(2, True)
        checks.append('explicit_save_preserves_mounted_input_and_output_behavior')
        refresh()
        scroll_up()
        input_char('n', True)
        emit(3, False)
        s['shot']('ssh-async-output-follow')
        checks.append('same_ssh_session_refresh_applies_input_false_output_true')
        change_settings(reset=True)
        scroll_up()
        input_char('q', True)
        emit(4, False)
        checks.append('reset_preserves_mounted_explicit_behavior')
        refresh()
        scroll_up()
        emit(5, True)
        before_echo = events('fixture.ssh_input_line_echoed')
        input_char('r', False)
        assert events('fixture.ssh_input_line_echoed') == before_echo
        key('Return')
        wait(lambda: events('fixture.ssh_input_line_echoed') == before_echo + 1, 'async_scroll_final_echo')
        wait(lambda: events('fixture.ssh_scope_async_exited') == 1, 'async_output_task_exited')
        assert events('fixture.ssh_scope_async_failed') == 0
        checks.append('reset_then_refresh_restores_defaults_and_output_task_exits')
        return {'checks': checks, 'backgroundBatches': 5, 'zeroInputDuringBackgroundOutput': True,
                'partialInputEcho': 'GUI_SSH_ECHO:pnqr', 'sameSession': session,
                'actualOutputTaskExit': True, 'limitations': ['Linux software rendering; no IME or cross-platform acceptance']}

    def scroll_interaction(session):
        returned = '回到最新输出'
        received = events('fixture.ssh_scope_input_received')

        def showing():
            return accessible(returned).returncode == 0

        def scroll_up():
            xdo('mousemove', '--window', s['xid'], '650', '350', 'click', '--repeat', '12', '--delay', '20', '4')
            wait(showing, 'ssh_scroll_reading')

        scroll_up()
        s['shot']('ssh-scroll-reading')
        bounds = accessible(returned, 'bounds')
        assert bounds.returncode == 0
        left, top, width, height = map(int, bounds.stdout.split())
        xdo('mousemove', str(left + width // 2), str(top + height // 2), 'click', '1')
        wait(lambda: not showing(), 'ssh_mouse_returns_latest')
        assert events('fixture.ssh_scope_input_received') == received, 'scroll.mouse_control_sent_input'
        scroll_up()
        assert accessible(returned, 'focus').returncode == 0
        key('space')
        wait(lambda: not showing(), 'ssh_keyboard_returns_latest')
        assert events('fixture.ssh_scope_input_received') == received, 'scroll.keyboard_control_sent_input'
        scroll_up()
        xdo('mousemove', '--window', s['xid'], '650', '350', 'click', '1')
        before_echo = events('fixture.ssh_input_line_echoed')
        xdo('type', '--clearmodifiers', '--delay', '1', 'SCOPE_SCROLL_INPUT')
        wait(lambda: not showing(), 'ssh_explicit_input_returns_latest_before_remote_echo')
        assert events('fixture.ssh_input_line_echoed') == before_echo
        key('Return')
        wait(lambda: events('fixture.ssh_input_line_echoed') == before_echo + 1, 'ssh_scroll_explicit_remote_echo')
        assert auth() == 5
        assert sum(events('fixture.ssh_scope_term.' + term) for term in ('xterm-256color', 'xterm', 'vt100')) == 5
        async_checks = asynchronous_scroll_settings(session, scroll_up, showing)
        return {'mouseAndKeyboardReturnToLatest': True, 'controlsSendNoRemoteInput': True,
                'explicitInputReturnsBeforeRemoteOutput': True, 'onlyOneExplicitLineEchoed': True,
                'noReauthenticationOrPtyRequest': True, 'session': session,
                'asynchronousSettings': async_checks}

    def history_search(session):
        focus(session)
        xdo('type', '--clearmodifiers', 'SCOPE_HISTORY')
        key('Return')
        wait(lambda: events('fixture.ssh_scope_history_sent') == 1, 'history_output')
        verify_terminal(session, 'classic', 'del_csi')  # OSC reply confirms preceding output parsed.
        scroll_checks = scroll_interaction(session)
        received = events('fixture.ssh_scope_input_received')
        click('查看当前窗格历史与查找')

        def find(text, expected):
            fill('查找历史与主屏', text)
            key('Return')
            def matched():
                value = accessible('历史分页状态', 'readonly-text')
                return value.stdout if value.returncode == 0 and expected in value.stdout else None
            message = wait(matched, 'ssh_history_' + expected)
            assert events('fixture.ssh_scope_input_received') == received, 'history.search_leaked_terminal_input'
            return message

        soft = 'SOFT' + '0123456789' * 20 + '中'
        message = find(soft, '匹配历史')
        assert '至' in message, 'history.match_did_not_cross_soft_wrap'
        text = accessible('当前页历史文本', 'readonly-text').stdout
        assert 'SOFT' in text and 'e\u0301NEEDLE' in text, (
            'history.missing_or_changed_combining_context.' + repr(text))
        find('HISTORY_ROW_000', '匹配历史')
        find('HARD_BREAK_AHARD_BREAK_B', '没有匹配')
        s['shot']('ssh-history')
        key('Escape')
        focus(session)
        xdo('type', '--clearmodifiers', 'exit')
        key('Return')
        wait(lambda: events('fixture.ssh_authenticated_stream_exited') == 1, 'history_natural_exit')
        wait(terminal_ready, 'ended_history_ready')
        received = events('fixture.ssh_scope_input_received')
        click('查看当前窗格历史与查找')
        find(soft, '匹配历史')
        find('GUI_SSH_END', '匹配主屏')
        ended_text = accessible('当前页历史文本', 'readonly-text')
        assert ended_text.returncode == 0 and 'GUI_SSH_ECHO:pnqr' in ended_text.stdout, 'scroll.partial_input_changed_or_leaked'
        s['shot']('ssh-ended-history')
        key('Escape')
        assert auth() == 5, 'history.search_reauthenticated'
        return scroll_checks

    def organization():
        click('分组、标签与收藏')
        wait(lambda: accessible('编辑分组').returncode == 0, 'organization')

    def save_behavior(scope_key, overrides):
        before = document(scope_key)
        before_auth = auth()
        for caption, index in overrides:
            select(caption, index)
        click('保存终端行为')
        wait(lambda: document(scope_key) != before, 'saved_scope')
        saved_scope = document(scope_key)
        assert saved_scope is not None, 'scope.saved_document_missing'
        stored = saved_scope['settings']['terminal']
        choices = {
            '终端配色': ('color_preset', [None, 'classic', 'midnight', 'paper']),
            'Backspace 发送': ('backspace_send', [None, 'del', 'bs']),
            'TERM': ('term_type', [None, 'xterm-256color', 'xterm', 'vt100']),
            'Delete 发送': ('delete_send', [None, 'csi', 'del']),
        }
        for caption, index in overrides:
            field, values = choices[caption]
            assert stored.get(field) == values[index], (
                f'scope.saved_value_mismatch.{field} expected={values[index]!r} actual={stored.get(field)!r}')
        assert auth() == before_auth, 'scope.metadata_save_started_connection'
        click('返回组织管理')
        click('完成')

    def group_behavior(group_key, restoring=False):
        organization()
        select('编辑分组', 1)
        click('分组终端配色与行为')
        wait(lambda: accessible('分组终端行为 · ALPHA_SCOPE_GROUP', 'inspect').returncode == 0,
             'group_behavior_target')
        s['shot']('group-behavior')
        save_behavior(group_key, [('终端配色', 0 if restoring else 2),
                                  ('TERM', 0 if restoring else 2),
                                  ('Backspace 发送', 0 if restoring else 2),
                                  ('Delete 发送', 0 if restoring else 2)])

    def host_behavior(restoring=False):
        organization()
        select('编辑分组', 0)
        click('SSH 入口终端配色与行为')
        wait(lambda: accessible('SSH 入口终端行为 · GUI_HOST', 'inspect').returncode == 0,
             'entry_behavior_target')
        save_behavior('settings.host.' + s['host'] + '.v1',
                      [('终端配色', 0 if restoring else 3),
                       ('TERM', 0 if restoring else 3),
                       ('Backspace 发送', 0 if restoring else 1),
                       ('Delete 发送', 0 if restoring else 1)])

    sessions = []
    global_session = cli_connect('global', 'xterm-256color', first=True)
    sessions.append(global_session)
    verify_terminal(global_session, 'paper', 'del_csi')
    palette_pixels(global_session, 'paper', 'system-light-global')

    organization()
    fill('分组名称', 'ALPHA_SCOPE_GROUP')
    click('创建分组')
    wait(lambda: document('catalogue.organization.v1') is not None, 'created_group')
    group_catalogue = document('catalogue.organization.v1')
    assert group_catalogue is not None, 'scope.created_group_missing'
    groups = group_catalogue['groups']
    group = next(row['metadata']['id'] for row in groups if row['name'] == 'ALPHA_SCOPE_GROUP')
    group_key = 'settings.group.' + group + '.v1'
    allowed.add(group_key)
    select('项目分组', 1)
    click('保存项目组织')
    wait(lambda: (catalogue := document('catalogue.organization.v1')) is not None and any(
        row['item'] == s['host'] and row['group'] == group for row in catalogue['memberships']),
        'assigned_direct_group')
    click('完成')
    group_behavior(group_key)
    verify_terminal(global_session, 'paper', 'del_csi')
    group_session = cli_connect('group', 'xterm')
    sessions.append(group_session)
    verify_terminal(group_session, 'midnight', 'bs_del')

    change_system('dark')
    verify_terminal(global_session, 'paper', 'del_csi')
    palette_pixels(global_session, 'paper', 'old-global-keeps-after-dark')
    host_behavior()
    verify_terminal(group_session, 'midnight', 'bs_del')
    host_session = cli_connect('host', 'vt100')
    sessions.append(host_session)
    verify_terminal(host_session, 'paper', 'del_csi')
    before_auth = auth()
    key('ctrl+shift+r')
    time.sleep(.5)
    wait(terminal_ready, 'refreshed_terminal')
    reveal('分组、标签与收藏', 'inspect')
    verify_terminal(host_session, 'paper', 'del_csi')
    assert auth() == before_auth, 'scope.refresh_redialed'

    host_behavior(restoring=True)
    verify_terminal(host_session, 'paper', 'del_csi')
    inherited_group = cli_connect('restored-group', 'xterm')
    sessions.append(inherited_group)
    verify_terminal(inherited_group, 'midnight', 'bs_del')

    group_behavior(group_key, restoring=True)
    verify_terminal(inherited_group, 'midnight', 'bs_del')
    inherited_global = cli_connect('restored-global', 'xterm-256color')
    sessions.append(inherited_global)
    verify_terminal(inherited_global, 'classic', 'del_csi')
    palette_pixels(inherited_global, 'classic', 'system-dark-restored-global')
    change_system('light')
    verify_terminal(inherited_global, 'classic', 'del_csi')
    palette_pixels(inherited_global, 'classic', 'old-dark-keeps-after-light')
    before_auth = auth()
    key('ctrl+shift+r')
    time.sleep(.5)
    wait(terminal_ready, 'system_light_refreshed_dark_session')
    verify_terminal(inherited_global, 'classic', 'del_csi')
    palette_pixels(inherited_global, 'classic', 'old-dark-keeps-after-refresh')
    assert auth() == before_auth == 5, 'scope.system_signal_refresh_reauthenticated'
    s['shot']('restored-global')
    scroll_checks = history_search(inherited_global)
    assert source_rows() == original, 'scope.connection_credential_or_trust_source_changed'
    assert auth() == 5
    s['close_gui'](s['app'])
    wait(lambda: events('fixture.ssh_authenticated_stream_exited') == 5, 'all_peer_streams_exited')
    assert s['stop_scope_palette_provider'](), 'scope.portal_bad_exit'
    (evidence / (prefix + '-result.json')).write_text(json.dumps({
        'version': 6, 'passed': True, 'scope': 'Linux production WebView, saved SSH password peer, cold and warm CLI',
        'global_direct_group_and_entrance_precedence': True,
        'clear_entrance_restores_group_and_clear_group_restores_global': True,
        'remote_raw_backspace_delete_and_osc10_foreground_verified': True,
        'systemTerminalPolicy': dict(light='paper', dark='classic'),
        'systemPaletteObservations': palette_observations,
        'systemSignalsSendNoInputOrAuthentication': True,
        'systemPortalProviderExited': True,
        'remote_pty_term_sequence': ['xterm-256color', 'xterm', 'vt100', 'xterm', 'xterm-256color'],
        'no_repeat_pty_on_setting_change_selection_or_refresh': True,
        'sshScrollInteraction': scroll_checks,
        'ssh_history_soft_wrap_unicode_and_hard_break_isolation': True,
        'ssh_natural_eof_retains_search_without_input_or_reauthentication': True,
        'existing_sessions_keep_frozen_keys_and_palette': True,
        'refresh_keeps_session_without_authentication': True,
        'organization_saves_do_not_connect': True,
        'connection_credentials_and_trust_unchanged': True,
        'sessions': sessions, 'password_authentication_count': auth(),
        'actual_gui_exit_and_all_five_peer_streams_exited': True,
    }, indent=2) + '\n')
    print('workspace.ssh_settings_precedence_restore_frozen_remote_bytes_palette_refresh_and_native_exit_confirmed')
