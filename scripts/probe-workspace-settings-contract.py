#!/usr/bin/env python3
"""GUI/CLI settings contract on the existing isolated native settings fixture."""
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import time
import tomllib

RESULT = Path("target/probes/workspace-settings-contract-result.json")
QUERY = r'''import json, os, select, sys, termios, time, tty
from pathlib import Path
name, pid = sys.argv[1:]
base = Path(__file__).parent / (name + "-" + pid)
old = termios.tcgetattr(0)
data = b""
try:
    tty.setraw(0)
    base.with_suffix(".ready").write_text("ready")
    if name.startswith("ALPHA_COLORS"): os.write(1, b"\x1b]10;?\x07")
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline and len(data) < 256:
        if not select.select([0], [], [], .15 if data else .5)[0]:
            if data: break
            continue
        data += os.read(0, 256-len(data))
        if name.startswith("ALPHA_COLORS") and (data.endswith(b"\x07") or data.endswith(b"\x1b\\")): break
finally:
    termios.tcsetattr(0, termios.TCSANOW, old)
base.with_suffix(".json").write_text(json.dumps({"hex": data.hex()}))
'''


def wait(predicate, label):
    deadline = time.monotonic() + 12
    while True:
        value = predicate()
        if value: return value
        assert time.monotonic() < deadline, "settings.contract_timeout." + label
        time.sleep(.05)


def finalize(fixture):
    pids = set(map(int, (fixture / "native-pids").read_text().split()))
    for pid in pids:
        try: os.kill(pid, 0)
        except ProcessLookupError: continue
        raise AssertionError("settings.native_session_still_alive")
    receipt = json.loads(RESULT.read_text())
    receipt.update(passed=True, nativeExitConfirmed=True, nativeSessionCount=len(pids))
    RESULT.write_text(json.dumps(receipt, indent=2) + "\n")
    print("workspace.settings_global_save_reopen_gui_cli_frozen_startup_and_native_exit_confirmed")


def verify(fixture, app, xid, config_file, native_log):
    helper = os.environ.get("PULSE_NATIVE_HELPER", "target/probes/x11-status")
    def run(args):
        return subprocess.run(list(map(str, args)), check=True, capture_output=True, text=True, timeout=4)
    def accessible(name, action="activate"):
        return run([helper, app, name, action]).stdout
    def visible(name):
        return subprocess.run([helper, app, name], capture_output=True, text=True, timeout=4).returncode == 0
    def xdo(*args): return run(["/usr/bin/xdotool", *args])
    def count(event): return native_log.read_text().splitlines().count("workspace." + event)
    def pid(): return (fixture / "native.pid").read_text().strip()
    def command(text):
        xdo("mousemove", "--window", xid, "400", "300", "click", "1")
        xdo("type", "--clearmodifiers", "--delay", "1", text)
        xdo("key", "--clearmodifiers", "Return")
    def read_json(path):
        try: return json.loads(path.read_text())
        except (FileNotFoundError, json.JSONDecodeError): return None
    def query(tag, keys=None):
        base = fixture / (tag + "-" + pid())
        command(tag)
        if keys:
            wait(lambda: base.with_suffix(".ready").exists(), tag + "_raw_ready")
            xdo("key", "--clearmodifiers", *keys)
        return bytes.fromhex(wait(lambda: read_json(base.with_suffix(".json")), tag)["hex"])
    def term(tag):
        path = fixture / (tag + "-" + pid() + ".json")
        command(tag)
        return wait(lambda: read_json(path), tag)["term"]
    def open_settings():
        before = count("settings_read_confirmed")
        accessible("设置")
        wait(lambda: count("settings_read_confirmed") > before, "settings_read")
    def search_setting(key):
        wait(lambda: subprocess.run([helper, app, "搜索设置", "focus"], capture_output=True, timeout=4).returncode == 0, "search_ready")
        xdo("key", "--clearmodifiers", "ctrl+a")
        xdo("type", "--clearmodifiers", key)
    def select_option(caption, index):
        accessible(caption, "focus")
        xdo("key", "--clearmodifiers", "space")
        wait(lambda: "expanded=1" in accessible(caption, "inspect-combo"), "menu_expanded_" + caption)
        xdo("key", "--clearmodifiers", "Home")
        for _ in range(index): xdo("key", "--clearmodifiers", "Down")
        xdo("key", "--clearmodifiers", "Return")
        wait(lambda: "expanded=0" in accessible(caption, "inspect-combo"), "menu_committed_" + caption)
    def save_select(caption, index, key):
        search_setting(key)
        original = config_file.read_bytes() if config_file.exists() else b""
        select_option(caption, index)
        assert (config_file.read_bytes() if config_file.exists() else b"") == original, "settings.wrote_before_explicit_save"
        before = count("settings_save_confirmed")
        wait(lambda: subprocess.run([helper, app, "保存" + caption, "activate"], capture_output=True, timeout=4).returncode == 0, "save_control_ready_" + key)
        wait(lambda: count("settings_save_confirmed") == before + 1, "save_" + caption)
    def saved(): return tomllib.loads(config_file.read_text())["terminal"]
    def history_rows(expected):
        command("ALPHA_HISTORY")
        accessible("查看当前窗格历史与查找")
        def retained():
            match = re.search(r"共保留 (\d+) 行", accessible("历史分页状态", "readonly-text"))
            if match and expected(int(match[1])): return int(match[1])
            accessible("最新历史")
            return None
        rows = wait(retained, "retained_history")
        xdo("key", "--clearmodifiers", "Escape")
        return rows

    def history_search():
        def status(): return accessible("历史分页状态", "readonly-text").strip()
        def find(text, expected, field="查找历史与主屏"):
            before = (fixture / "input").read_bytes()
            accessible(field, "focus")
            xdo("key", "--clearmodifiers", "ctrl+a")
            xdo("type", "--clearmodifiers", text)
            xdo("key", "--clearmodifiers", "Return")
            message = wait(lambda: (value if expected in (value := status()) else None), "search_" + expected)
            assert (fixture / "input").read_bytes() == before, "history.search_sent_terminal_input"
            return message
        def open_history():
            accessible("查看当前窗格历史与查找")
            wait(lambda: visible("查找历史与主屏"), "history_search_open")
        assert history_rows(lambda value: value == 1000) == 1000
        query("ALPHA_COLORS_HISTORY_DRAIN")
        open_history()
        find("alpha-history-0500", "匹配历史")
        find("alpha-history-1099", "匹配主屏")
        find("ALPHA-HISTORY-0500", "匹配历史")
        accessible("区分大小写")
        find("ALPHA-HISTORY-0500", "没有匹配")
        find("alpha-history-0500", "匹配历史")
        accessible("区分大小写")
        first = find("alpha-history-050", "匹配历史")
        before = (fixture / "input").read_bytes()
        xdo("key", "--clearmodifiers", "Return")
        wait(lambda: (value := status()).startswith("匹配历史") and value != first, "next_match")
        xdo("key", "--clearmodifiers", "shift+Return")
        wait(lambda: status() == first, "previous_match")
        accessible("整词匹配")
        find("alpha-history-0500", "匹配历史")
        accessible("整词匹配")
        accessible("正则表达式")
        find(r"^alpha-history-050[0-9]$", "匹配历史")
        find("(", "正则表达式无效")
        accessible("正则表达式")
        assert (fixture / "input").read_bytes() == before, "history.controls_sent_terminal_input"
        xdo("key", "--clearmodifiers", "Escape")
        command("ALPHA_ALT")
        query("ALPHA_COLORS_ALT_DRAIN")
        open_history()
        select_option("查找范围", 1)
        find("ALPHA-ALT-needle", "匹配备用屏", "查找备用屏")
        xdo("key", "--clearmodifiers", "Escape")
        command("ALPHA_PRIMARY")
        query("ALPHA_COLORS_PRIMARY_DRAIN")
        ended_pid = int(pid())
        command("ALPHA_FINISH")
        def ended():
            try: os.kill(ended_pid, 0); return False
            except ProcessLookupError: return True
        wait(ended, "native_eof")
        open_history()
        find("ALPHA-ENDED-needle", "匹配主屏")
        xdo("key", "--clearmodifiers", "Escape")
        return {"historyAndPrimary": True, "caseWordRegex": True, "bidirectional": True, "alternate": True, "ended": True, "noTerminalInput": True}

    RESULT.write_text(json.dumps({"passed": False, "checksPassed": False, "nativeExitConfirmed": False}) + "\n")
    (fixture / "settings-query.py").write_text(QUERY)
    original_pid = pid()
    assert term("ALPHA_TERM_OLD_INITIAL") == "xterm-256color"
    initial_color = query("ALPHA_COLORS_OLD_INITIAL")
    assert re.fullmatch(rb"\x1b\]10;rgb:[0-9a-fA-F]{4}/[0-9a-fA-F]{4}/[0-9a-fA-F]{4}(?:\x07|\x1b\\)", initial_color)
    assert query("ALPHA_KEYS_OLD_INITIAL", ["BackSpace", "Delete"]) == b"\x7f\x1b[3~"
    open_settings()
    save_select("Backspace 发送", 1, "backspace_send")
    save_select("Delete 发送编码（下次显示生效）", 1, "delete_send")
    save_select("TERM（仅新建会话；不切换终端模拟器）", 2, "term_type")
    save_select("终端配色", 1, "terminal_preset")
    search_setting("history_limit")
    accessible("保留历史行数", "focus")
    xdo("key", "--clearmodifiers", "ctrl+a")
    xdo("type", "--clearmodifiers", "1000")
    before = count("settings_save_confirmed")
    accessible("保存保留历史行数")
    wait(lambda: count("settings_save_confirmed") == before + 1, "save_history_limit")
    assert saved()["backspace_send"] == "bs" and saved()["delete_send"] == "del", str(saved())
    assert saved()["term_type"] == "vt100" and saved()["history_limit"] == 1000
    frozen_document = config_file.read_bytes()
    xdo("key", "--clearmodifiers", "Escape")
    open_settings()
    assert config_file.read_bytes() == frozen_document, "settings.reopen_wrote_config"
    xdo("key", "--clearmodifiers", "Escape")
    before = count("terminal_input_ready")
    xdo("key", "--clearmodifiers", "ctrl+shift+r")
    wait(lambda: count("terminal_input_ready") > before, "same_session_refresh")
    assert pid() == original_pid
    assert query("ALPHA_KEYS_OLD_REFRESH", ["BackSpace", "Delete"]) == b"\x08\x7f"
    assert term("ALPHA_TERM_OLD_REFRESH") == "xterm-256color"
    assert query("ALPHA_COLORS_OLD_REFRESH") == initial_color
    old_rows = history_rows(lambda value: value > 1000)
    before = count("terminal_input_ready")
    accessible("POSIX sh")
    wait(lambda: count("terminal_input_ready") > before and pid() != original_pid, "new_gui_session")
    assert term("ALPHA_TERM_NEW_GUI") == "vt100"
    gui_color = query("ALPHA_COLORS_NEW_GUI")
    assert gui_color != initial_color and gui_color.startswith(b"\x1b]10;rgb:")
    assert history_rows(lambda value: value == 1000) == 1000
    gui_pid = pid()
    pulse = Path(os.environ.get("CARGO_TARGET_DIR", "target")) / "debug/pulse"
    before = count("terminal_input_ready")
    with subprocess.Popen([str(pulse), "new-tab", "sh"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as client:
        try:
            wait(lambda: visible("创建终端") or client.poll() is not None, "cli_request")
            if client.poll() is not None:
                stdout, stderr = client.communicate()
                raise AssertionError(f"settings.cli_request_failed exit={client.returncode} stdout={stdout!r} stderr={stderr!r}")
            accessible("创建终端")
            client.communicate(timeout=12)
            assert client.returncode == 0, "settings.cli_new_tab_rejected"
        finally:
            if client.poll() is None:
                client.terminate()
                client.communicate(timeout=3)
    wait(lambda: count("terminal_input_ready") > before and pid() != gui_pid, "new_cli_session")
    assert term("ALPHA_TERM_NEW_CLI") == "vt100"
    assert query("ALPHA_COLORS_NEW_CLI") == gui_color
    interaction_checks = runpy.run_path("scripts/probe-workspace-terminal-interaction.py")["verify"](
        fixture, app, xid, native_log, {**locals(), "wait": wait})
    history_checks = history_search()
    assert config_file.read_bytes() == frozen_document, "settings.session_start_wrote_config"
    run(["/usr/bin/import", "-window", xid, "target/probes/workspace-settings-contract.png"])
    RESULT.write_text(json.dumps({"version": 4, "checksPassed": True, "nativeExitConfirmed": False, "oldSessionRetainedRows": old_rows, "newSessionRetainedRows": 1000, "sameNativeSessionAfterRefresh": True, "encodingAppliedAfterRefresh": True, "oldTermAndPaletteFrozen": True, "newGuiAndCliInheritGlobalSettings": True, "reopenAndLaunchDoNotWriteSettings": True, "terminalSettings": saved(), "historySearch": history_checks, "terminalInteraction": interaction_checks}, indent=2) + "\n")


if __name__ == "__main__":
    if sys.argv[1] == "finalize": finalize(Path(sys.argv[2]))
    else:
        try:
            verify(Path(sys.argv[1]), sys.argv[2], sys.argv[3], Path(sys.argv[4]), Path(sys.argv[5]))
        except BaseException:
            subprocess.run(["/usr/bin/import", "-window", sys.argv[3], "target/probes/workspace-settings-contract-failure.png"], check=False, timeout=4)
            raise
