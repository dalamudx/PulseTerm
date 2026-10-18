#!/usr/bin/env python3
"""Real saved SSH settings inheritance; invoked by the isolated cold-CLI fixture."""
import json
import re
import sqlite3
import subprocess
import time


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

    def history_search(session):
        focus(session)
        xdo('type', '--clearmodifiers', 'SCOPE_HISTORY')
        key('Return')
        wait(lambda: events('fixture.ssh_scope_history_sent') == 1, 'history_output')
        verify_terminal(session, 'classic', 'del_csi')  # OSC reply confirms preceding output parsed.
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
        s['shot']('ssh-ended-history')
        key('Escape')
        assert auth() == 5, 'history.search_reauthenticated'

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
        stored = document(scope_key)['settings']['terminal']
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
    verify_terminal(global_session, 'classic', 'del_csi')

    organization()
    fill('分组名称', 'ALPHA_SCOPE_GROUP')
    click('创建分组')
    wait(lambda: document('catalogue.organization.v1') is not None, 'created_group')
    groups = document('catalogue.organization.v1')['groups']
    group = next(row['metadata']['id'] for row in groups if row['name'] == 'ALPHA_SCOPE_GROUP')
    group_key = 'settings.group.' + group + '.v1'
    allowed.add(group_key)
    select('项目分组', 1)
    click('保存项目组织')
    wait(lambda: any(row['item'] == s['host'] and row['group'] == group
                     for row in document('catalogue.organization.v1')['memberships']), 'assigned_direct_group')
    click('完成')
    group_behavior(group_key)
    verify_terminal(global_session, 'classic', 'del_csi')
    group_session = cli_connect('group', 'xterm')
    sessions.append(group_session)
    verify_terminal(group_session, 'midnight', 'bs_del')

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
    s['shot']('restored-global')
    history_search(inherited_global)
    assert source_rows() == original, 'scope.connection_credential_or_trust_source_changed'
    assert auth() == 5
    s['close_gui'](s['app'])
    wait(lambda: events('fixture.ssh_authenticated_stream_exited') == 5, 'all_peer_streams_exited')
    (evidence / (prefix + '-result.json')).write_text(json.dumps({
        'version': 3, 'passed': True, 'scope': 'Linux production WebView, saved SSH password peer, cold and warm CLI',
        'global_direct_group_and_entrance_precedence': True,
        'clear_entrance_restores_group_and_clear_group_restores_global': True,
        'remote_raw_backspace_delete_and_osc10_foreground_verified': True,
        'remote_pty_term_sequence': ['xterm-256color', 'xterm', 'vt100', 'xterm', 'xterm-256color'],
        'no_repeat_pty_on_setting_change_selection_or_refresh': True,
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
