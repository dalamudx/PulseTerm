#!/usr/bin/env python3
"""Owned production cold/warm CLI MFA; reused by the cold SSH supervisor."""
import json
import subprocess


def verify(context):
    wait, xdo = (context[key] for key in ('wait', 'xdo'))
    click, accessible = (context[key] for key in ('click', 'accessible'))
    launch, close_gui, client_result = (context[key] for key in ('launch', 'close_gui', 'client_result'))
    quiet, count, hashes = (context[key] for key in ('quiet', 'event_count', 'source_hashes'))
    initial = context['initial']
    evidence, prefix = context['evidence'], context['prefix']

    password_joint = context.get("password_interactive_mode", False)
    key_joint = context.get("key_interactive_mode", False)
    agent_joint = context.get("agent_interactive_mode", False)
    joint = password_joint or key_joint or agent_joint
    flag = "--password-interactive" if password_joint else "--agent-interactive" if agent_joint else "--key-interactive" if key_joint else "--interactive"
    def unchanged():
        assert hashes() == initial, 'cli.mfa.metadata_changed'

    def password_idle():
        if password_joint:
            assert count('fixture.ssh_password_joint_auth_attempt') == 0
            assert count('fixture.ssh_password_joint_partial_success') == 0
    def preview(locked=False):
        name = '请求的保存记录' if locked else '保存的主机与身份'
        result = accessible(name, 'readonly-text')
        text = result.stdout
        assert result.returncode == 0 and 'MFA' in text, 'cli.mfa.method_preview_missing'
        if key_joint:
            assert "私钥 + MFA" in text, "cli.key_mfa.explicit_method_preview_missing"
        if agent_joint:
            assert "Agent + MFA" in text, "cli.agent_mfa.explicit_method_preview_missing"
        if password_joint:
            assert "密码 + MFA" in text, "cli.password_mfa.explicit_method_preview_missing"
        if locked:
            assert context['host'] in text and context['identity'] in text
        else:
            assert '127.0.0.1' in text and 'GUI_HOST' in text

    def target(expected_password_attempts=None):
        before = context['auth_count']()
        wait(lambda: accessible('允许 CLI 连接保存的 SSH 主机？').returncode == 0, 'cli_mfa_target')
        preview()
        assert context['auth_count']() == before
        if password_joint:
            assert expected_password_attempts is not None
            assert count('fixture.ssh_password_joint_auth_attempt') == expected_password_attempts
        click('连接')
        wait(lambda: accessible('Account <literal>').returncode == 0, 'cli_mfa_round_one')
        assert context['auth_count']() == before
        if agent_joint:
            assert count('fixture.agent_selected_signed_once') == count('fixture.ssh_key_joint_partial_success')
            assert count('fixture.ssh_key_joint_partial_success') > 0
            assert count('fixture.ssh_key_joint_premature_mfa_rejected') == 0
        if password_joint:
            assert count('fixture.ssh_password_joint_auth_attempt') == count('fixture.ssh_password_joint_partial_success')
            assert count('fixture.ssh_password_joint_premature_mfa_rejected') == 0

    def warm():
        expected = count("fixture.ssh_password_joint_auth_attempt")
        context['client'] = subprocess.Popen(
            [str(context['pulse']), 'ssh', context['host'], '--identity', context['identity'], flag],
            env=context['env'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        target(expected)

    def field(name, value):
        assert accessible(name, 'focus').returncode == 0
        xdo('key', '--clearmodifiers', 'ctrl+a')
        xdo('type', '--clearmodifiers', value)

    def answer(tag):
        before = context['auth_count']()
        field('Account <literal>', 'user')
        field('First verification code', '123456')
        click('提交本轮回答')
        wait(lambda: accessible('Second verification code').returncode == 0, 'cli_mfa_round_two')
        assert context['auth_count']() == before
        context['shot'](tag + '-second-round')
        field('Second verification code', '654321')
        click('提交本轮回答')
        client_result(tag, True)
        wait(lambda: context['auth_count']() == before + 1, tag + '_authenticated')

    def terminal(tag, first_echo):
        import time
        for turn in range(2):
            if turn:
                xdo('key', '--clearmodifiers', 'ctrl+shift+r')
            # Cold startup deliberately discards GUI stdio. Verify through the
            # real accessibility input and actual peer receipt, never a mock.
            time.sleep(.5)
            wait(lambda: accessible('Terminal input', 'focus').returncode == 0, tag + '_terminal')
            before_auth = context['auth_count']()
            xdo('type', '--clearmodifiers', 'cli-mfa-echo-' + str(first_echo + turn))
            xdo('key', '--clearmodifiers', 'Return')
            wait(lambda: count('fixture.ssh_input_line_echoed') == first_echo + turn, tag + '_input_' + str(turn))
            assert context['auth_count']() == before_auth
        context['shot'](tag + '-terminal-refreshed')

    app = launch('cold-cancel')
    preview(True)
    click('取消'); client_result('cold-cancel', False)
    assert quiet(); password_idle(); unchanged(); close_gui(app)

    app = launch('cold-target-cancel')
    context['unlock_without_confirming'](); click('查看目标')
    wait(lambda: accessible('允许 CLI 连接保存的 SSH 主机？').returncode == 0, 'cold_target_cancel')
    preview(); click('取消'); client_result('cold-target-cancel', False)
    assert quiet(); password_idle(); unchanged(); close_gui(app)

    app = launch('cold-round-cancel')
    context['unlock_without_confirming'](); click('查看目标'); target(0)
    field('Account <literal>', 'discard-this-owned-draft')
    xdo('key', '--clearmodifiers', 'Escape')
    client_result('cold-round-cancel', False)
    wait(lambda: count('fixture.ssh_interactive_stream_exited') == 1, 'cold_round_native_exit')
    assert context['auth_count']() == 0
    if agent_joint: assert count('fixture.agent_selected_signed_once') == 1
    else: assert quiet()
    unchanged()

    warm()
    context['client'].terminate(); client_result('warm-disconnect', False)
    wait(lambda: accessible('Account <literal>').returncode != 0, 'disconnected_question_closed')
    wait(lambda: count('fixture.ssh_interactive_stream_exited') == 2, 'disconnect_native_exit')
    assert context['auth_count']() == 0
    if agent_joint: assert count('fixture.agent_selected_signed_once') == 2
    else: assert quiet()
    unchanged()

    close_gui(app)
    app = launch('cold-approved')
    context['unlock_without_confirming'](); click('查看目标'); target(2)
    answer('cold-approved'); terminal('cold-approved', 1)
    unchanged()
    warm(); answer('warm-approved'); terminal('warm-approved', 3)
    unchanged(); close_gui(app)
    wait(lambda: count('fixture.ssh_interactive_stream_exited') == 4 and count('fixture.ssh_authenticated_stream_exited') == 2, 'all_native_streams_exited')
    assert context['auth_count']() == 2
    if agent_joint:
        assert count('fixture.agent_selected_signed_once') == 4
        assert count('fixture.ssh_key_joint_partial_success') == 4
        assert count('fixture.agent_unexpected_sign_request') == 0
        assert count('fixture.ssh_password_auth_attempt_rejected') == 0
        assert count('fixture.ssh_public_key_authenticated') == 0
        assert count('fixture.ssh_key_joint_shell_requested') == 2
    if password_joint:
        assert count('fixture.ssh_password_joint_auth_attempt') == 4
        assert count('fixture.ssh_password_joint_partial_success') == 4
        assert count('fixture.ssh_password_joint_shell_requested') == 2
        assert count('fixture.ssh_password_joint_stream_exited') == 4
        assert count('fixture.ssh_password_joint_premature_mfa_rejected') == 0
        assert count('fixture.ssh_password_authenticated') == 0
        assert count('fixture.ssh_public_key_authenticated') == 0
        assert count('fixture.agent_selected_signed_once') == 0
    receipt = {
        'version': 1, 'passed': True, 'scope': 'Linux production WebView with real CLI and two-round russh fixture',
        'cold_locked_cancel_zero_auth': True, 'unlock_not_approval': True,
        'cold_target_cancel_zero_auth': True, 'cold_question_escape_cancels_native': True,
        'warm_cli_disconnect_cancels_native': True, 'warm_explicit_method_and_target_approval': True,
        'cold_and_warm_two_rounds_authenticated': True, 'terminal_input_before_after_refresh': True,
        'refresh_did_not_redial': True, 'source_hashes_unchanged': True,
        'original_gui_exit_zero': True, 'endpoints_drained': True,
        'cancelled_native_streams': 2, 'authenticated_native_streams': 2,
        'external_mfa_provider_checked': False, 'cross_platform_checked': False,
        "authentication": "password-interactive" if password_joint else "agent-interactive" if agent_joint else "key-interactive" if key_joint else "interactive",
        "joint_policy_requires_both_factors": joint,
        "password_authentication_attempts": count("fixture.ssh_password_joint_auth_attempt") if password_joint else 0,
        "password_native_streams_exited": count("fixture.ssh_password_joint_stream_exited") if password_joint else 0,
        "fixture_only_partial_success_correction": joint,
        "selected_agent_signatures": count('fixture.agent_selected_signed_once') if agent_joint else 0,
        "nonselected_agent_signatures": count('fixture.agent_unexpected_sign_request'),
    }
    (evidence / (prefix + '-result.json')).write_text(json.dumps(receipt, indent=2) + '\n')
    print('workspace.cli_interactive_cold_warm_two_rounds_cancellation_refresh_native_exit_confirmed')
