#!/usr/bin/env python3
"""Real PTY scroll/bell scenario for the existing settings contract fixture."""
import json
import os
from pathlib import Path
import select
import sys
import termios
import time
import tty


def worker(fixture):
    fixture = Path(fixture)
    received = bytearray()
    deadline = time.monotonic() + 75
    old = termios.tcgetattr(0)

    def publish():
        target = fixture / "interaction-received.json"
        temporary = fixture / "interaction-received.pending"
        temporary.write_text(json.dumps({"hex": received.hex()}))
        temporary.replace(target)

    def until(stage):
        while not (fixture / stage).exists():
            assert time.monotonic() < deadline, "interaction.worker_timeout." + stage
            if select.select([0], [], [], .03)[0]:
                data = os.read(0, 256)
                assert data, "interaction.worker_eof"
                received.extend(data)
                assert len(received) <= 256, "interaction.unexpected_input_volume"
                publish()

    try:
        tty.setraw(0)
        publish()
        os.write(1, "".join(f"INTERACTION_ROW_{row:03d}\r\n" for row in range(160)).encode())
        (fixture / "interaction-ready").touch()
        until("interaction-emit")
        os.write(1, b"LIVE_WHILE_READING\r\n" * 12 + b"\a\a")
        (fixture / "interaction-emitted").touch()
        until("interaction-ring-again")
        os.write(1, b"\a")
        (fixture / "interaction-rang-again").touch()
        until("interaction-links")
        import runpy
        links = runpy.run_path(str(fixture / "terminal-links.py"))
        browser = json.loads((fixture / "browser-config.json").read_text())
        os.write(1, links["OUTPUT"] + f'SYSTEM {browser["url"]}\r\n'.encode())
        (fixture / "interaction-links-ready").touch()
        until("interaction-links-done")
        until("interaction-finish")
        os.write(1, b"LIVE_AFTER_REFRESH\r\n")
    finally:
        termios.tcsetattr(0, termios.TCSANOW, old)
    publish()
    (fixture / "interaction-finished").touch()


def verify(fixture, app, xid, native_log, api):
    wait, accessible, visible, xdo, count, command, query, read_json, pid = (
        api[key] for key in ("wait", "accessible", "visible", "xdo", "count", "command", "query", "read_json", "pid")
    )
    returned = "回到最新输出"
    bell = "清除终端响铃提示"
    original_pid = pid()
    before_sessions = count("session_opened")
    before_input = (fixture / "input").read_bytes()

    def received():
        result = read_json(fixture / "interaction-received.json")
        return None if result is None else result["hex"]

    def scroll_up():
        xdo("mousemove", "--window", xid, "600", "350", "click", "--repeat", "12", "--delay", "20", "4")
        wait(lambda: visible(returned), "scroll_return_shown")

    def mouse_click(name):
        left, top, width, height = map(int, accessible(name, "bounds").split())
        xdo("mousemove", str(left + width // 2), str(top + height // 2), "click", "1")

    command("ALPHA_INTERACTION")
    wait(lambda: (fixture / "interaction-ready").exists(), "interaction_worker_ready")
    assert (fixture / "input").read_bytes() == before_input + b"ALPHA_INTERACTION\n"
    scroll_up()
    (fixture / "interaction-emit").touch()
    wait(lambda: visible(bell), "live_bell")
    assert visible(returned), "interaction.output_moved_reading_viewport"
    assert received() == "", "interaction.scroll_sent_terminal_input"
    xdo("mousemove", "--window", xid, "20", "30")
    api["run"](["/usr/bin/import", "-window", xid, "target/probes/workspace-terminal-reading-bell.png"])
    mouse_click(returned)
    wait(lambda: not visible(returned), "mouse_return_to_bottom")
    assert received() == "", "interaction.mouse_return_sent_input"
    scroll_up()
    accessible(returned, "focus")
    xdo("key", "--clearmodifiers", "space")
    wait(lambda: not visible(returned), "keyboard_return_to_bottom")
    accessible(bell, "focus")
    xdo("key", "--clearmodifiers", "space")
    wait(lambda: not visible(bell), "coalesced_bell_cleared_once")
    assert received() == "", "interaction.notice_controls_sent_input"
    scroll_up()
    xdo("mousemove", "--window", xid, "600", "350", "click", "1")
    xdo("type", "--clearmodifiers", "k")
    wait(lambda: received() == "6b", "only_explicit_key_reaches_pty")
    wait(lambda: not visible(returned), "input_returns_to_bottom")
    (fixture / "interaction-ring-again").touch()
    wait(lambda: visible(bell), "fresh_bell_after_clear")
    before_ready = count("terminal_input_ready")
    xdo("key", "--clearmodifiers", "ctrl+shift+r")
    wait(lambda: count("terminal_input_ready") > before_ready, "interaction_refresh_ready")
    assert pid() == original_pid, "interaction.refresh_restarted_session"
    assert not visible(bell), "interaction.replayed_bell_after_refresh"
    assert received() == "6b", "interaction.refresh_sent_input"
    import runpy
    link_checks = runpy.run_path("scripts/probe-workspace-terminal-links.py")["verify"](
        fixture, xid, api, received)
    (fixture / "interaction-finish").touch()
    wait(lambda: (fixture / "interaction-finished").exists(), "interaction_worker_finished")
    query("ALPHA_COLORS_INTERACTION_DRAIN")
    assert not visible(bell), "interaction.late_replayed_bell"
    assert received() == "6b", "interaction.extra_input"
    assert count("session_opened") == before_sessions, "interaction.opened_extra_session"
    api["run"](["/usr/bin/import", "-window", xid, "target/probes/workspace-terminal-interaction-refreshed.png"])
    return {
        "defaultOutputPreservesReadingPosition": True,
        "mouseAndKeyboardReturnToLatest": True,
        "defaultInputReturnsToLatest": True,
        "repeatedLiveBellsCoalesceAndClear": True,
        "freshBellAfterClear": True,
        "refreshDoesNotReplayBellsOrRestartSession": True,
        "rawPtyBytesHex": "6b",
        "onlyExplicitTestKeyReachedPty": True,
        "links": link_checks,
    }


if __name__ == "__main__":
    assert len(sys.argv) == 3 and sys.argv[1] == "worker"
    worker(sys.argv[2])
