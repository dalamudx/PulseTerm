"""Real CLI requests against the probe-owned two-key Agent and original WebView."""
import json
import subprocess


def command(c):
    identity = c.agent_catalogue()[1][0][0]
    return ['target/debug/pulse', 'ssh', c.host_id, '--identity', identity]


def signed(c):
    return c.peer_log.read_text().splitlines().count('fixture.agent_selected_signed_once')


def events(c, name):
    return c.log.read_text().splitlines().count('workspace.' + name)


def locked_request(c):
    result = c.run(command(c), check=False, timeout=5)
    (c.evidence / (c.prefix + '-locked.txt')).write_text(result.stdout + result.stderr)
    assert result.returncode == 1 and signed(c) == 0
    assert events(c, 'cli_ssh_approval_requested') == 0 and c.recent() is None


def exercise(c, original):
    saved = c.agent_catalogue()

    def submit(name):
        expected = events(c, 'cli_ssh_approval_requested') + 1
        output = (c.evidence / (c.prefix + '-' + name + '-client.txt')).open('wb')
        process = subprocess.Popen(command(c), stdout=output, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        c.cli_clients.append((process, output))
        c.wait(lambda: events(c, 'cli_ssh_approval_requested') == expected, 'cli_' + name)
        c.reveal('连接')
        return process

    def rejected(process):
        assert process.wait(timeout=5) == 1
        assert signed(c) == 0 and events(c, 'cli_ssh_opened') == 0
        assert c.agent_catalogue() == saved and c.sources() == original and c.recent() is None

    process = submit('cancel')
    c.endpoint_screenshot('approval-cancel'); c.click('取消'); rejected(process)

    process = submit('navigation')
    expected = events(c, 'profiles_listed') + 1
    c.key('ctrl+shift+r')
    c.wait(lambda: events(c, 'profiles_listed') == expected, 'cli_navigation')
    rejected(process)

    process = submit('disconnect')
    process.terminate(); process.wait(timeout=3)
    c.click('连接')
    assert signed(c) == 0 and c.recent() is None
    c.click('取消')

    process = submit('approve')
    c.endpoint_screenshot('approval'); c.click('连接')
    c.wait(lambda: events(c, 'cli_ssh_opened') == 1, 'cli_registered')
    assert process.wait(timeout=5) == 0
    c.wait(lambda: events(c, 'terminal_input_ready') >= 1 and
           'fixture.ssh_window_request.' in c.peer_log.read_text(), 'cli_input_ready')
    assert signed(c) == 1
    assert c.peer_log.read_text().splitlines().count('fixture.ssh_public_key_authenticated') == 1
    assert 'fixture.ssh_password_authenticated' not in c.peer_log.read_text()
    c.wait(lambda: len((c.recent() or {}).get('entries', [])) == 1, 'cli_recent')
    recent = c.recent(); assert recent['entries'][0]['host'] == c.host_id
    c.run(['/usr/bin/xdotool', 'mousemove', '--window', c.xid, '450', '210', 'click', '1'])
    c.run(['/usr/bin/xdotool', 'type', '--clearmodifiers', 'A']); c.key('Return')
    c.wait(lambda: 'fixture.agent_input_A_confirmed' in c.peer_log.read_text(), 'cli_raw_input')
    c.endpoint_screenshot('terminal')
    expected = events(c, 'profiles_listed') + 1
    c.click('重新加载工作区')
    c.wait(lambda: events(c, 'profiles_listed') == expected, 'cli_refresh')
    assert signed(c) == 1 and c.agent_catalogue() == saved and c.sources() == original
    c.endpoint_screenshot('refreshed'); c.close()
    c.wait(lambda: 'fixture.ssh_authenticated_stream_exited' in c.peer_log.read_text(), 'cli_stream_exit')

    c.start(2); c.click('最近连接'); c.click(c.target)
    c.reveal('选择保存的SSH身份', 'focus'); c.key('End', 'Tab')
    c.wait(lambda: 'GUI_AGENT_RENAMED' in c.accessible('选择保存的SSH身份', 'description').stdout,
           'cli_saved_identity_reopen')
    c.endpoint_screenshot('reopened'); c.click('取消'); c.close()
    assert signed(c) == 1 and c.agent_catalogue() == saved and c.sources() == original
    assert c.recent() == recent
    assert 'workspace.cli_ssh_opened' not in c.log.read_text()
    result = {'version': 1, 'passed': True, 'scope': 'Linux real CLI and original WebView with two-key Agent socket',
              'locked_rejected': True, 'cancel_navigation_disconnect_zero_sign': True,
              'selected_key_signed_once_no_fallback': True, 'native_terminal_input': True,
              'refresh_no_reauthentication': True, 'independent_reopen': True,
              'sources_unchanged': True, 'native_exit_confirmed': True,
              'sources_sha256': original}
    (c.evidence / (c.prefix + '-result.json')).write_text(json.dumps(result, indent=2) + '\n')
    print('workspace.cli_agent_original_window_cancel_navigation_disconnect_one_selected_signature_refresh_reopen_native_exit_confirmed')
