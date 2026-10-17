"""Real same-process WebViews with independent Vault and SSH approval ownership."""
import json
import os
import re
import subprocess
import time


def verify(c):
    env, evidence, prefix = c['env'], c['evidence'], c['prefix']
    primary, secondary = 'PulseTerm · 工作区预览', 'PulseTerm · 工作区'
    clients, app, a, b = [], None, None, None
    wait, xdo, count = c['wait'], c['xdo'], c['event_count']

    def run(*args):
        return c['run']([c['pulse'], *args], check=False, timeout=8)

    def start(*args):
        client = subprocess.Popen([str(c['pulse']), *args], env=env,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        clients.append(client)
        return client

    def finish(client, tag, accepted):
        out, err = client.communicate(timeout=15)
        (evidence / (prefix + '-' + tag + '-client.log')).write_text(f'exit={client.returncode}\n' + out + err)
        assert (client.returncode == 0) == accepted, ('multiwindow.ssh.client', tag, client.returncode)
        return out.strip()

    def accessible(title, name, action=None):
        query = ['target/probes/x11-status', str(app), name]
        if action:
            query.append(action)
        return subprocess.run(query, env={**env, 'PULSETERM_PROBE_FRAME': title},
                              capture_output=True, text=True, timeout=3)

    def click(xid, title, name):
        xdo('windowactivate', '--sync', xid)
        wait(lambda: accessible(title, name, 'focus').returncode == 0, 'button.' + name)
        xdo('key', '--clearmodifiers', 'space')

    def shot(xid, tag):
        c['run'](['/usr/bin/import', '-window', xid, evidence / (prefix + '-' + tag + '.png')])

    def windows():
        result = c['run'](['/usr/bin/xdotool', 'search', '--onlyvisible', '--pid', str(app)], check=False)
        return {xid for xid in result.stdout.splitlines()
                if xdo('getwindowname', xid) in (primary, secondary)}

    def ready(title):
        state = accessible(title, '管理', 'inspect')
        return state.returncode == 0 and 'enabled=1' in state.stdout and 'showing=1' in state.stdout

    def unchanged():
        assert c['quiet'](), 'multiwindow.ssh.unexpected_authentication'
        assert c['source_hashes']() == c['initial'], 'multiwindow.ssh.cancel_wrote_sources'

    def ssh():
        return start('-w', label_b, 'ssh', c['host'], '--identity', c['identity'])

    def prompt(client):
        def drawn():
            assert client.poll() is None, 'multiwindow.ssh.rejected_before_target'
            return accessible(secondary, '允许 CLI 连接保存的 SSH 主机？').returncode == 0
        wait(drawn, 'secondary_target')
        assert accessible(primary, '允许 CLI 连接保存的 SSH 主机？').returncode != 0
        unchanged()

    def open_vault(xid, title):
        xdo('windowactivate', '--sync', xid)
        xdo('key', '--clearmodifiers', 'ctrl+shift+p')
        wait(lambda: accessible(title, '搜索命令', 'focus').returncode == 0, 'command_search')
        xdo('type', '--clearmodifiers', 'vault')
        xdo('key', '--clearmodifiers', 'Return')
        wait(lambda: accessible(title, '主密码').returncode == 0
             or accessible(title, '锁定保险库').returncode == 0, 'vault_dialog')

    def vault(xid, title, unlock=False, lock=False):
        open_vault(xid, title)
        if unlock:
            wait(lambda: accessible(title, '主密码').returncode == 0, 'master_field')
            assert accessible(title, '主密码', 'focus').returncode == 0
            xdo('key', '--clearmodifiers', 'ctrl+a')
            xdo('type', '--clearmodifiers', 'fixed-target-master')
            click(xid, title, '解锁保险库')
        if lock:
            click(xid, title, '锁定保险库')
        state = '已解锁 · 不代表允许 SSH 使用凭据' if unlock else '已锁定'
        wait(lambda: accessible(title, state).returncode == 0, 'vault.' + state)
        shot(xid, 'vault-' + ('a' if xid == a else 'b') + ('-available' if unlock else '-locked'))
        click(xid, title, '关闭')

    def route_prompt(index):
        # Reuse existing exact current-server fingerprint/SQL-target checks.
        env['PULSETERM_PROBE_FRAME'] = secondary
        c['xid'] = b
        button = c['route_prompt'](index)
        assert accessible(primary, button).returncode != 0
        return button

    def auth_counts():
        return {name: count(name) for name in (
            'fixture.ssh_password_authenticated', 'fixture.ssh_public_key_authenticated',
            'fixture.agent_selected_signed_once', 'fixture.route.outer.authenticated',
            'fixture.route.middle.agent_authenticated')}

    def input_once(text, expected):
        xdo('windowactivate', '--sync', b)
        xdo('mousemove', '--window', b, '500', '220', 'click', '1')
        xdo('type', '--clearmodifiers', '--delay', '1', text)
        xdo('key', '--clearmodifiers', 'Return')
        wait(lambda: count('fixture.ssh_input_line_echoed') == expected, 'raw_input.' + text)

    try:
        assert run('focus').returncode == 0
        app = wait(lambda: (c['guis']() or [None])[0], 'gui')
        assert c['guis']() == [app]
        a = wait(lambda: next(iter(windows())) if len(windows()) == 1 else None, 'primary')
        c['xid'] = a
        wait(lambda: ready(primary), 'primary_ready')
        client = start('-w', 'workspace', 'new-window')
        wait(lambda: accessible(primary, '允许 CLI 新建窗口？').returncode == 0, 'create_invitation')
        click(a, primary, '新建窗口')
        label_b = finish(client, 'create-secondary', True)
        assert re.fullmatch(r'workspace-[0-9a-f-]{36}', label_b)
        b = wait(lambda: next(iter(windows() - {a})) if len(windows()) == 2 else None, 'secondary')
        c['xid'] = b
        wait(lambda: ready(secondary), 'secondary_ready')
        unchanged()
        result = run('-w', label_b, 'ssh', c['host'], '--identity', c['identity'])
        (evidence / (prefix + '-locked-client.log')).write_text(result.stdout + result.stderr)
        assert result.returncode != 0
        vault(a, primary, unlock=True)
        assert run('-w', label_b, 'ssh', c['host'], '--identity', c['identity']).returncode != 0
        vault(b, secondary)
        unchanged()
        vault(b, secondary, unlock=True)
        for scenario in ('cancel', 'disconnect', 'navigation'):
            client = ssh()
            prompt(client)
            shot(b, 'target-' + scenario)
            if scenario == 'cancel':
                click(b, secondary, '取消')
            elif scenario == 'disconnect':
                client.terminate()
            else:
                xdo('windowactivate', '--sync', b)
                xdo('key', '--clearmodifiers', 'ctrl+shift+r')
            finish(client, scenario, False)
            wait(lambda: accessible(secondary, '允许 CLI 连接保存的 SSH 主机？').returncode != 0, 'target_gone')
            wait(lambda: ready(secondary), 'secondary_recovered')
            unchanged()
            if scenario == 'navigation':
                # Navigation revokes the old operation, preserving previously accepted keys.
                open_vault(a, primary)
                wait(lambda: accessible(primary, '已解锁 · 不代表允许 SSH 使用凭据').returncode == 0, 'primary_epoch_preserved')
                click(a, primary, '关闭')
                open_vault(b, secondary)
                wait(lambda: accessible(secondary, "已解锁 · 不代表允许 SSH 使用凭据").returncode == 0, "secondary_key_preserved")
                click(b, secondary, "关闭")
        if c['route_mode']:
            client = ssh()
            prompt(client)
            click(b, secondary, '连接')
            route_prompt(0)
            click(b, secondary, '取消')
            finish(client, 'first-hop-cancel', False)
            wait(lambda: count('fixture.route.outer.connection_exited') >= 1, 'cancelled_parent_exit')
            wait(lambda: ready(secondary), 'route_cancel_recovered')
            unchanged()
        client = ssh()
        prompt(client)
        # Lock A while B's independently approved epoch and target gesture are pending.
        vault(a, primary, lock=True)
        assert accessible(secondary, '允许 CLI 连接保存的 SSH 主机？').returncode == 0
        click(b, secondary, '连接')
        if c['route_mode']:
            for index in range(3):
                click(b, secondary, route_prompt(index))
        session = finish(client, 'approved', True).removeprefix('session ')
        assert re.fullmatch(r'[1-9][0-9]*', session)
        wait(lambda: c['auth_count']() == 1 and 'fixture.ssh_window_request.' in c['peer_log'].read_text(), 'native_authenticated')
        counts = auth_counts()
        assert counts['fixture.agent_selected_signed_once'] == (2 if c['route_mode'] else 1 if c['agent_mode'] else 0)
        assert counts['fixture.ssh_password_authenticated'] == (0 if c['agent_mode'] else 1)
        assert counts['fixture.ssh_public_key_authenticated'] == (1 if c['agent_mode'] else 0)
        if c['route_mode']:
            assert counts['fixture.route.outer.authenticated'] == counts['fixture.route.middle.agent_authenticated'] == 1
            c['verify_route_pins']()
        wait(lambda: accessible(secondary, '会话 ' + session).returncode == 0, 'secondary_session')
        assert accessible(primary, '会话 ' + session).returncode != 0
        assert run('-w', 'workspace', 'focus-tab', session).returncode != 0
        input_once('multiwindow-ssh-before', 1)
        shot(b, 'terminal-before')
        xdo('windowactivate', '--sync', b)
        xdo('key', '--clearmodifiers', 'ctrl+shift+r')
        wait(lambda: ready(secondary), 'refreshed_secondary')
        time.sleep(.5)  # Presentation only; send each input once, never retry it.
        input_once('multiwindow-ssh-refreshed', 2)
        assert auth_counts() == counts
        shot(b, 'terminal-refreshed')
        xdo('windowactivate', '--sync', a)
        xdo('key', '--clearmodifiers', 'alt+F4')
        wait(lambda: windows() == {b}, 'primary_closed')
        assert c['guis']() == [app] and run('-w', label_b, 'profiles').returncode == 0
        assert run('-w', 'workspace', 'focus').returncode != 0
        input_once('multiwindow-ssh-primary-closed', 3)
        assert auth_counts() == counts
        shot(b, 'primary-closed')
        after = c['source_hashes']()
        for table, digest in c['initial'].items():
            if not c['route_mode'] or table not in {'known_hosts', 'known_hosts_state'}:
                assert after[table] == digest, ('source_changed', table)
        parents = {name: count('fixture.route.' + name + '.connection_exited') for name in ('outer', 'middle')}
        forwards = {name: count('fixture.route.' + name + '.forward_exited') for name in ('outer', 'middle')}
        c['close_gui'](app)
        wait(lambda: count('fixture.ssh_authenticated_stream_exited') == 1, 'authenticated_stream_exit')
        if c['route_mode']:
            wait(lambda: all(count('fixture.route.' + name + '.connection_exited') > value for name, value in parents.items())
                 and all(count('fixture.route.' + name + '.forward_exited') > value for name, value in forwards.items()), 'parent_forward_exit')
        assert auth_counts() == counts
        result = dict(version=1, passed=True, scope='Linux same-process real WebViews and local software SSH fixture',
                      gui_pid=app, window_labels=['workspace', label_b], session=session,
                      peer_pid=c["peer"].pid, runtime_directory=str(c["runtime"]), seed_directory=str(c["seed_root"]),
                      identity_kind='route' if c['route_mode'] else 'agent' if c['agent_mode'] else 'password',
                      primary_unlock_does_not_unlock_secondary=True, secondary_navigation_preserves_primary_epoch=True,
                      cancel_disconnect_navigation_zero_auth_and_writes=True, primary_lock_preserves_secondary_approval=True,
                      session_and_prompts_owned_by_secondary=True, refresh_no_reauthentication=True,
                      primary_close_preserves_secondary_input=True, exact_selected_signature_counts=counts,
                      actual_gui_exit_and_endpoints_drained=True, peer_and_parent_streams_exited=True)
        (evidence / (prefix + '-result.json')).write_text(json.dumps(result, indent=2) + '\n')
    finally:
        if app and c['guis']():
            for xid in windows():
                try:
                    shot(xid, 'failure-' + xid)
                except (OSError, subprocess.SubprocessError):
                    pass
        for client in clients:
            if client.poll() is None:
                client.terminate()
            try:
                client.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                client.kill()
                client.communicate(timeout=3)
