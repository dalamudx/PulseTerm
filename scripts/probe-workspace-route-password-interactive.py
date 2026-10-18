#!/usr/bin/env python3
"""Owned three-hop password+MFA; no external peer, real user secret or arbitrary forwarding."""
import json
import subprocess


def verify(c):
    wait, accessible, xdo = (c[key] for key in ('wait', 'accessible', 'xdo'))
    event = c['event_count']
    checks = []
    captured_hops = set()
    c.setdefault('clients', [])
    def click(name):
        wait(lambda: accessible(name, 'activate').returncode == 0, 'activate_'+name)
    def fill(name, value):
        wait(lambda: accessible(name, 'focus').returncode == 0, 'focus_'+name)
        xdo('key', '--clearmodifiers', 'ctrl+a')
        xdo('type', '--clearmodifiers', value)
    def attempts():
        return [event('fixture.route.'+hop+'.password_attempt') for hop in ('outer', 'middle')] + [event('fixture.ssh_password_joint_auth_attempt')]
    def parent_exits():
        return sum(event('fixture.route.'+hop+'.connection_exited') for hop in ('outer', 'middle'))
    def prompt(index):
        c["stage"] = "password-hop-"+str(index)
        expected = '第 '+str(index+1)+'/3 跳'
        buttons = ('信任并连接此跳', '确认连接此跳')
        wait(lambda: any(expected in accessible(button, 'description').stdout for button in buttons), 'hop_'+str(index))
        button = next(button for button in buttons if expected in accessible(button, 'description').stdout)
        description = accessible(button, 'description').stdout
        label = ('GUI_JUMP_OUTER', 'GUI_JUMP_MIDDLE', 'GUI_HOST')[index]
        row = c['route_targets'][label]
        fp = c['descriptor']('fixture.route.'+('outer', 'middle')[index]+'.fingerprint.') if index < 2 else c['descriptor']('fixture.ssh_gui_fingerprint.')
        assert label in description and fp in description and str(row['username'])+'@'+row['host']+':'+str(row['port']) in description
        (c["evidence"]/(c["prefix"]+"-peer-"+str(index)+"-description.txt")).write_text(description)
        before = attempts()
        wait(lambda: accessible('认证方式', 'focus').returncode == 0, 'method_'+str(index))
        xdo('key', '--clearmodifiers', 'End', 'Return')
        assert attempts() == before, 'method_selection_must_not_use_password'
        if index not in captured_hops:
            c['shot']('password-hop-'+str(index)+'-before-approval')
            captured_hops.add(index)
        click(button)
    def answer():
        fill('Account <literal>', 'user')
        fill('First verification code', '123456')
        click('提交本轮回答')
        fill('Second verification code', '654321')
        click('提交本轮回答')
    def complete(index):
        prompt(index)
        answer()
    def result(tag, success):
        c['client_result'](tag, success)
        if not success:
            xdo('key', 'Escape')
    def input_line(expected):
        wait(lambda: accessible('Terminal input', 'focus').returncode == 0, 'terminal_input')
        xdo('type', '--clearmodifiers', 'echo GUI_SSH_GUI')
        xdo('key', 'Return')
        wait(lambda: event('fixture.ssh_input_line_echoed') == expected, 'input_'+str(expected))
    def warm(tag):
        wait(lambda: all(event('fixture.route.'+hop+'.connection_opened')-event('fixture.route.'+hop+'.connection_exited') == 1 for hop in ('outer', 'middle')), 'parents_before_'+tag)
        c['client'] = subprocess.Popen([str(c['binaries']/'pulse'), 'ssh', c['host'], '--identity', c['identity']], env=c['env'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        c['clients'].append(c['client'])
        wait(lambda: accessible('允许 CLI 连接保存的 SSH 主机？').returncode == 0, 'warm_'+tag)
        click('连接')

    c['app'] = c['launch']('password-cold')
    c['unlock_without_confirming']()
    click('查看目标')
    wait(lambda: accessible('允许 CLI 连接保存的 SSH 主机？').returncode == 0, 'cold_approval')
    assert attempts() == [0, 0, 0]
    click('连接')
    for index in range(3):
        complete(index)
    result('password-cold', True)
    assert attempts() == [1, 1, 1]
    assert all(event('fixture.route.'+hop+'.password_partial_success') == 1 for hop in ('outer', 'middle'))
    assert event('fixture.ssh_interactive_authenticated') == 1
    input_line(1)
    checks.append('cold_three_hops_each_explicit_password_and_two_mfa_rounds')
    before = attempts()
    xdo('key', 'ctrl+r')
    input_line(2)
    assert attempts() == before
    checks.append('refresh_same_terminal_without_password_or_mfa_retry')

    middle = c['policy'].with_suffix('.middle')
    middle.write_text('complete')
    baseline = parent_exits()
    warm('middle-complete')
    complete(0)
    prompt(1)
    result('middle-complete', False)
    wait(lambda: parent_exits() >= baseline+2, 'complete_success_rejected_and_parents_exited')
    assert event('fixture.route.middle.password_complete_success') == 1
    assert event('fixture.route.middle.interactive_authenticated') == 1
    assert event('fixture.ssh_password_joint_auth_attempt') == 1
    middle.write_text('partial')
    checks.append('middle_single_factor_complete_success_rejected_without_mfa_or_leaf')

    baseline, leaf_before = parent_exits(), event('fixture.ssh_interactive_stream_exited')
    warm('leaf-cancel')
    complete(0)
    complete(1)
    prompt(2)
    wait(lambda: accessible('First verification code', 'focus').returncode == 0, 'leaf_question')
    xdo('key', 'Escape')
    result('leaf-cancel', False)
    wait(lambda: event('fixture.ssh_interactive_stream_exited') == leaf_before+1, 'cancelled_leaf_exited')
    wait(lambda: parent_exits() >= baseline+2, 'cancelled_parents_exited')
    input_line(3)
    checks.append('leaf_mfa_cancel_drains_both_parents_and_keeps_successful_terminal')

    warm('password-warm')
    for index in range(3):
        complete(index)
    result('password-warm', True)
    input_line(4)
    assert attempts() == [4, 4, 3]
    assert event('fixture.route.outer.interactive_authenticated') == 4
    assert event('fixture.route.middle.interactive_authenticated') == 3
    assert event('fixture.ssh_interactive_authenticated') == 2
    assert event('fixture.route.outer.premature_mfa_rejected') == event('fixture.route.middle.premature_mfa_rejected') == 0
    assert event('fixture.ssh_password_joint_premature_mfa_rejected') == 0
    checks.append('warm_three_hops_two_factors_no_method_fallback')
    c['verify_route_pins']()
    current = c['source_hashes']()
    assert {k:v for k,v in current.items() if k not in ('known_hosts', 'known_hosts_state')} == {k:v for k,v in c['initial'].items() if k not in ('known_hosts', 'known_hosts_state')}
    c['shot']('password-two-route-terminals')
    c['close_gui'](c['app'])
    for hop in ('outer', 'middle'):
        base = 'fixture.route.'+hop
        wait(lambda: event(base+'.connection_exited') == event(base+'.connection_opened'), 'all_'+hop+'_exited')
        wait(lambda: event(base+'.forward_exited') == event(base+'.forwarded'), 'all_'+hop+'_forward_exited')
        assert event(base+'.terminal_requested') == 0
    wait(lambda: event('fixture.ssh_interactive_stream_exited') == 3, 'all_mfa_leaf_streams_exited')
    checks.append('source_unchanged_exact_forward_only_and_all_native_streams_exited')
    (c['evidence']/(c['prefix']+'-result.json')).write_text(json.dumps({'version':1, 'passed':True, 'checks':checks, 'count':len(checks), 'passwordAttempts':attempts(), 'successfulSessions':2, 'realWebView':True, 'realLoopbackSsh':True, 'crossPlatformChecked':False, 'externalServersChecked':False}, indent=2)+'\n')
