#!/usr/bin/env python3
"""Synthetic real three-hop MFA; only the fixture-owned window, private store and loopback peers."""
import json
import subprocess
import time


def verify(c):
    wait, accessible, xdo = (c[key] for key in ("wait", "accessible", "xdo"))
    def click(name):
        wait(lambda: accessible(name, "activate").returncode == 0, "activate_"+name)
    event = c['event_count']
    c.setdefault("clients", [])
    joint = c.get("route_key_interactive_mode", False)
    if joint:
        policies = {"leaf": c["policy"], **{hop: c["policy"].with_suffix("."+hop) for hop in ("outer", "middle")}}
        policies["middle"].write_text("complete")
    checks = []
    def parent_exits():
        return sum(event("fixture.route."+hop+".connection_exited") for hop in ("outer","middle"))
    def record(name): checks.append(name)
    def fill(name, value):
        wait(lambda: accessible(name).returncode == 0, 'field_'+name)
        assert accessible(name,'focus').returncode == 0
        xdo('key','--clearmodifiers','ctrl+a')
        xdo('type','--clearmodifiers',value)
    def prompt(index, interactive=True):
        expected = '第 '+str(index+1)+'/3 跳'
        buttons = ('信任并连接此跳','确认连接此跳')
        wait(lambda: any(expected in accessible(button,'description').stdout for button in buttons), 'hop_'+str(index))
        button = next(button for button in buttons if expected in accessible(button,'description').stdout)
        description = accessible(button,'description').stdout
        label = ('GUI_JUMP_OUTER','GUI_JUMP_MIDDLE','GUI_HOST')[index]
        row = c['route_targets'][label]
        fingerprint = c['descriptor']('fixture.route.'+('outer','middle')[index]+'.fingerprint.') if index < 2 else c['descriptor']('fixture.ssh_gui_fingerprint.')
        assert label in description and fingerprint in description and str(row['username'])+'@'+row['host']+':'+str(row['port']) in description
        (c["evidence"]/(c["prefix"]+"-peer-"+str(index)+"-description.txt")).write_text(description)
        assert accessible('认证方式','focus').returncode == 0
        xdo('key','--clearmodifiers','Home')
        if interactive: xdo('key','Down')
        xdo('key','Return')
        if agent_joint:
            assert event("fixture.agent_selected_signed_once") == agent_signatures_before + max(0, index-1)
            assert event("fixture.agent_connection_failed") == 0
        click(button)
    def answer(wrong=False, first_only=False):
        fill('Account <literal>', 'user')
        fill('First verification code', '000000' if wrong else '123456')
        click('提交本轮回答')
        if not wrong and not first_only:
            fill('Second verification code','654321')
            click('提交本轮回答')
    def complete(index, interactive=True):
        prompt(index, interactive)
        if interactive: answer()
    def warm(tag):
        wait(lambda: all(event("fixture.route."+hop+".connection_opened")-event("fixture.route."+hop+".connection_exited") == 1 for hop in ("outer", "middle")), "previous_attempt_parents_drained")
        args = [str(c['binaries']/'pulse'), 'ssh',c['host'],'--identity',c['identity']]
        c['client'] = subprocess.Popen(args,env=c['env'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        c['clients'].append(c['client'])
        wait(lambda: accessible("允许 CLI 连接保存的 SSH 主机？").returncode == 0,"warm_"+tag)
        def exact_preview():
            result = accessible("保存的主机与身份", "readonly-text")
            if result.returncode != 0: return False
            assert c["host"] in result.stdout and c["identity"] in result.stdout, "warm_wrong_saved_target"
            return True
        wait(exact_preview, "warm_exact_readonly_saved_target")
        click("连接")
    def result(tag, accepted):
        c['client_result'](tag,accepted)
        if not accepted:
            xdo('key','Escape')
    def terminal_input(expected):
        wait(lambda: accessible('Terminal input').returncode == 0,'terminal_input')
        assert accessible('Terminal input','focus').returncode == 0
        xdo('type','--clearmodifiers','echo GUI_SSH_GUI')
        xdo('key','Return')
        wait(lambda:event("fixture.ssh_input_line_echoed") == expected,"real_terminal_input")
    agent_joint = c.get("route_agent_interactive_mode", False)
    agent_signatures_before = 0
    c['app'] = c['launch']('mixed-cold')
    c['unlock_without_confirming']()
    click('查看目标')
    wait(lambda: accessible('允许 CLI 连接保存的 SSH 主机？').returncode == 0,'cold_approval')
    assert event('fixture.route.outer.authenticated') == event('fixture.route.middle.authenticated') == event('fixture.ssh_interactive_authenticated') == 0
    click('连接')
    complete(0, not agent_joint)
    complete(1, agent_joint)
    complete(2)
    result('mixed-cold',True)
    assert event("fixture.route.outer.interactive_authenticated") == (0 if agent_joint else 1)
    assert event("fixture.route.middle.authenticated") == 1 and event("fixture.route.middle.interactive_authenticated") == (1 if agent_joint else 0)
    assert event('fixture.ssh_interactive_authenticated') == 1
    record("cold_saved_password_agent_mfa_forward_agent_mfa_leaf_exact_peers" if agent_joint else "cold_mixed_explicit_key_mfa_saved_key_key_mfa_exact_peers" if joint else "cold_mixed_explicit_mfa_password_mfa_exact_peers")
    terminal_input(1)
    xdo('key','ctrl+r')
    terminal_input(2)
    assert event('fixture.ssh_interactive_authenticated') == 1
    record('real_terminal_input_and_refresh_without_reauthentication')
    if agent_joint:
        assert event("fixture.agent_selected_signed_once") == 2
        assert event("fixture.agent_connection_failed") == 0
        record("selected_agent_signed_only_after_each_exact_peer_approval")
        agent_signatures_before = 2
        warm("cancel-agent-leaf")
        complete(0, False)
        complete(1)
        prompt(2)
        wait(lambda: accessible("First verification code").returncode == 0, "agent_leaf_question_before_cancel")
        assert event("fixture.agent_selected_signed_once") == 4
        wait(lambda: all(event("fixture.route."+hop+".connection_opened")-event("fixture.route."+hop+".connection_exited") == 2 for hop in ("outer", "middle")), "cancel_route_has_only_two_live_attempts")
        exits_before_cancel = {hop:event("fixture.route."+hop+".connection_exited") for hop in ("outer", "middle")}
        leaf_exits_before_cancel = event("fixture.ssh_interactive_stream_exited")
        xdo("key", "Escape")
        result("cancel-agent-leaf", False)
        wait(lambda: all(event("fixture.route."+hop+".connection_exited") == count+1 for hop,count in exits_before_cancel.items()) and event("fixture.ssh_interactive_stream_exited") == leaf_exits_before_cancel+1, "cancelled_agent_route_drained")
        assert event("fixture.ssh_interactive_authenticated") == 1
        record("pending_leaf_cancel_denied_without_fallback_and_parents_drained")
        c["verify_route_pins"]()
        current = c["source_hashes"]()
        assert {key:value for key,value in current.items() if key not in ("known_hosts","known_hosts_state")} == {key:value for key,value in c["initial"].items() if key not in ("known_hosts","known_hosts_state")}
        record("saved_agent_metadata_and_ciphertexts_unchanged")
        c["shot"]("verified-agent-route-terminal")
        c["close_gui"](c["app"])
        for hop in ("outer", "middle"):
            base = "fixture.route."+hop
            wait(lambda: event(base+".connection_exited") == event(base+".connection_opened"), "all_"+hop+"_connections_exited")
            wait(lambda: event(base+".forward_exited") == event(base+".forwarded"), "all_"+hop+"_forward_channels_exited")
            assert event(base+".terminal_requested") == 0
        wait(lambda: event("fixture.ssh_interactive_stream_exited") == 2, "all_agent_leaf_streams_exited")
        assert event("fixture.agent_selected_signed_once") == 4 and event("fixture.agent_connection_failed") == 0
        record("all_forward_and_leaf_resources_exited_and_no_intermediate_terminal")
        (c["evidence"]/(c["prefix"]+"-result.json")).write_text(json.dumps({"checks":checks,"count":len(checks),"authentication":"password-forward+agent-mfa-forward+agent-mfa-terminal","selectedKeySignatures":4,"agentProtocolFailures":0,"realWebView":True,"realLoopbackSsh":True,"originalSavedTargets":True,"syntheticAnswersOnly":True,"externalServersChecked":False,"crossPlatformChecked":False},indent=2)+chr(10))
        return
    if joint:
        policies["middle"].write_text("partial")
        for tag, policy_value, selected in (("ordinary-key-partial", "partial", False), ("key-rejected", "reject", True), ("key-only-success", "complete", True)):
            policies["outer"].write_text(policy_value)
            before_exit = parent_exits()
            before_forward = event("fixture.route.outer.forwarded")
            before_mfa = event("fixture.route.outer.interactive_round.1")
            warm(tag)
            prompt(0, selected)
            result(tag, False)
            wait(lambda: parent_exits() == before_exit+1, "first_factor_rejected_connection_exit")
            assert event("fixture.route.outer.forwarded") == before_forward
            assert event("fixture.route.outer.interactive_round.1") == before_mfa
            record(tag+"_zero_mfa_and_zero_forwarding")
        policies["outer"].write_text("partial")
    for cancelled_hop in (0,1,2):
        warm('cancel-'+str(cancelled_hop))
        for index in range(cancelled_hop): complete(index)
        prompt(cancelled_hop)
        wait(lambda: accessible('First verification code').returncode == 0,'question_before_cancel')
        before = parent_exits()
        leaf_before = event("fixture.ssh_interactive_stream_exited")
        xdo('key','Escape')
        result('cancel-'+str(cancelled_hop),False)
        wait(lambda:parent_exits() >= before+min(cancelled_hop+1,2),"parents_drained")
        if cancelled_hop == 2:
            wait(lambda:event("fixture.ssh_interactive_stream_exited") == leaf_before+1,"cancelled_leaf_drained")
        record('cancel_hop_'+str(cancelled_hop)+'_all_owned_parents_drained')
    if joint:
        for index in (0, 1):
            tag = "wrong-joint-hop-"+str(index)
            before_exit = parent_exits()
            warm(tag)
            for previous in range(index): complete(previous)
            prompt(index); answer(True)
            result(tag, False)
            wait(lambda: all(event("fixture.route."+hop+".connection_opened")-event("fixture.route."+hop+".connection_exited") == 1 for hop in ("outer", "middle")), "wrong_answer_parents_drained")
            assert parent_exits() >= before_exit+index+1
            record(tag+"_denied_without_method_fallback")
    warm('wrong-leaf')
    complete(0); complete(1); prompt(2); answer(True)
    result('wrong-leaf',False)
    assert event('fixture.ssh_password_authenticated') == 0 and event('fixture.ssh_public_key_authenticated') == 0
    record('wrong_leaf_answer_denied_without_password_or_key_fallback')
    if joint:
        import sqlite3
        warm("changed-leaf-source")
        complete(0); complete(1); prompt(2)
        wait(lambda: accessible("First verification code").returncode == 0, "leaf_question_before_source_change")
        before_second = event("fixture.ssh_interactive_round_two")
        before_leaf_exit = event("fixture.ssh_interactive_stream_exited")
        with sqlite3.connect(c["database"]) as db:
            original_label, = db.execute("SELECT label FROM key_identities WHERE id=? AND deleted=0", (c["identity"],)).fetchone()
            assert db.execute("UPDATE key_identities SET label=? WHERE id=? AND deleted=0", (original_label+" changed fixture", c["identity"])).rowcount == 1
        try:
            answer(first_only=True)
            result("changed-leaf-source", False)
            wait(lambda: event("fixture.ssh_interactive_stream_exited") == before_leaf_exit+1, "revoked_leaf_stream_exited")
            assert event("fixture.ssh_interactive_round_two") == before_second, "changed source must reject before sending the answer"
        finally:
            with sqlite3.connect(c["database"]) as db:
                assert db.execute("UPDATE key_identities SET label=? WHERE id=? AND deleted=0", (original_label, c["identity"])).rowcount == 1
        record("changed_key_metadata_while_question_pending_zero_answer_and_leaf_exit")
    warm('all-interactive')
    complete(0); complete(1); complete(2)
    result('all-interactive',True)
    assert event('fixture.ssh_interactive_authenticated') == 2
    terminal_input(3)
    record('warm_all_three_hops_two_rounds_and_root_terminal')
    c['verify_route_pins']()
    current = c['source_hashes']()
    assert {key:value for key,value in current.items() if key not in ('known_hosts','known_hosts_state')} == {key:value for key,value in c['initial'].items() if key not in ('known_hosts','known_hosts_state')}
    record('saved_metadata_and_ciphertexts_unchanged_except_explicit_first_pins')
    c["shot"]("verified-two-route-terminals")
    c["close_gui"](c["app"])
    for hop in ("outer","middle"):
        base = "fixture.route."+hop
        wait(lambda:event(base+".connection_exited") == event(base+".connection_opened"),"all_"+hop+"_connections_exited")
        wait(lambda:event(base+".forward_exited") == event(base+".forwarded"),"all_"+hop+"_forward_channels_exited")
    assert parent_exits() >= (19 if joint else 11), "missing_expected_parent_connections"
    wait(lambda:event("fixture.ssh_interactive_stream_exited") == (5 if joint else 4), "all_leaf_peers_exited")
    record('window_exit_waited_all_owned_parent_and_leaf_connections')
    (c["evidence"]/(c["prefix"]+"-result.json")).write_text(json.dumps({"checks":checks,"count":len(checks),"authentication":"private-key+keyboard-interactive" if joint else "keyboard-interactive", "realWebView":True,"realLoopbackSsh":True,"originalSavedTargets":True,"syntheticAnswersOnly":True,"externalServersChecked":False,"crossPlatformChecked":False},indent=2)+chr(10))
