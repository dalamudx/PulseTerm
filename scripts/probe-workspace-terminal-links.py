#!/usr/bin/env python3
"""Trusted pointer activation and GTK clipboard readback in the private Xvfb."""
import csv
import subprocess
import time

OUTPUT = (
    "\x1b[2J\x1b[H"
    "PLAIN https://example.invalid/plain\r\n"
    "ENDPOINT 127.0.0.1:8123/alpha\r\n"
    "OSC \x1b]8;;https://example.invalid/actual?via=osc8\x1b\\"
    "https://friendly.invalid\x1b]8;;\x1b\\\r\n"
    "FILE \"/remote/team notes/report.txt\":17:4\r\n"
    "BLOCKED \x1b]8;;https://account@example.invalid/blocked\x1b\\"
    "BLOCKED_TARGET\x1b]8;;\x1b\\\r\n"
).encode()


def verify(fixture, xid, api, received):
    wait, accessible, visible, xdo, run = (
        api[key] for key in ("wait", "accessible", "visible", "xdo", "run")
    )
    labels = {"PLAIN", "ENDPOINT", "OSC", "FILE", "BLOCKED", "SYSTEM"}
    points = {}
    screenshot = fixture / "links-frame.png"
    ocr = fixture / "links-frame"
    before_documents = api["count"]("document_confirmed")
    before_input = received()

    def locate():
        run(["/usr/bin/import", "-window", xid, screenshot])
        run(["/usr/bin/env", "OMP_THREAD_LIMIT=1", "/usr/bin/tesseract", screenshot, ocr, "--psm", "6", "tsv"])
        points.clear()
        with ocr.with_suffix(".tsv").open() as source:
            for word in csv.DictReader(source, delimiter="\t"):
                label = word["text"].upper()
                if label in labels and label not in points:
                    points[label] = (
                        int(word["left"]) + int(word["width"]) + 24,
                        int(word["top"]) + int(word["height"]) // 2,
                    )
        return points.keys() >= labels

    (fixture / "interaction-links").touch()
    wait(lambda: (fixture / "interaction-links-ready").exists(), "links_output")
    wait(locate, "links_visible_markers")

    def activate(label, heading="检查终端链接"):
        x, y = points[label]
        xdo("mousemove", "--window", xid, str(x), str(y))
        # Linkifier lookup follows a real pointer event before the trusted click.
        time.sleep(.15)
        xdo("click", "1")
        wait(lambda: visible(heading), "link_review_" + label)

    def click(name):
        left, top, width, height = map(int, accessible(name, "bounds").split())
        xdo("mousemove", str(left + width // 2), str(top + height // 2), "click", "1")

    def close():
        xdo("key", "--clearmodifiers", "Escape")
        wait(lambda: not visible("检查终端链接") and not visible("检查终端文件路径"), "link_dismissed")
        assert received() == before_input, "links.controls_sent_pty_input"

    def target_value(name):
        try:
            return accessible(name, "readonly-text").strip()
        except subprocess.CalledProcessError:
            return None
    def copy_and_read(button, expected):
        click(button)
        status = "路径已复制。" if button == "复制路径" else "地址已复制。"
        wait(lambda: visible(status), "link_copy_confirmed")
        copied = fixture / "link-clipboard.txt"
        run(["target/probes/x11-clipboard", "read", copied])
        assert copied.read_text() == expected, "links.clipboard_mismatch"

    for label, target in (
        ("PLAIN", "https://example.invalid/plain"),
        ("ENDPOINT", "http://127.0.0.1:8123/alpha"),
        ("OSC", "https://example.invalid/actual?via=osc8"),
    ):
        activate(label)
        wait(lambda: target_value("实际目标地址") == target, "exact_link_target_" + label)
        assert "enabled=1" in accessible("在系统浏览器打开", "inspect")
        if label == "OSC":
            run(["/usr/bin/import", "-window", xid, "target/probes/workspace-terminal-osc8-review.png"])
        copy_and_read("复制地址", target)
        close()

    activate("FILE", "检查终端文件路径")
    path = "/remote/team notes/report.txt"
    wait(lambda: target_value("实际文件路径") == path, "exact_file_path")
    assert visible("Unix 绝对路径 · 行 17 · 列 4"), "links.missing_line_column"
    assert not visible("在系统浏览器打开"), "links.file_offers_browser"
    run(["/usr/bin/import", "-window", xid, "target/probes/workspace-terminal-file-review.png"])
    copy_and_read("复制路径", path)
    close()

    activate("BLOCKED")
    assert visible("此目标不可用：仅接受不含凭据的 HTTP／HTTPS 地址。"), "links.unsafe_target_accepted"
    assert not visible("实际目标地址"), "links.unsafe_target_exposed"
    assert "enabled=0" in accessible("复制地址", "inspect")
    assert "enabled=0" in accessible("在系统浏览器打开", "inspect")
    close()
    activate("SYSTEM")
    browser_config = api["read_json"](fixture / "browser-config.json")
    wait(lambda: target_value("实际目标地址") == browser_config["url"], "exact_browser_target")
    assert not (fixture / "browser-launched.json").exists(), "links.browser_opened_before_approval"
    assert not (fixture / "browser-request.json").exists(), "links.page_loaded_before_approval"
    click("在系统浏览器打开")
    wait(lambda: visible("已交给系统默认浏览器处理。"), "gio_handoff_confirmed")
    wait(lambda: (fixture / "browser-request.json").exists(), "browser_loaded_local_page")
    launched = wait(lambda: api["read_json"](fixture / "browser-launched.json"), "browser_launched")

    def browser_window():
        try:
            values = run(["/usr/bin/xdotool", "search", "--onlyvisible", "--name", "^PulseTerm link handoff probe"]).stdout.split()
        except subprocess.CalledProcessError:
            return None
        assert len(values) <= 1, "links.multiple_browser_windows"
        return values[0] if values else None

    browser_xid = wait(browser_window, "browser_window")
    assert run(["/usr/bin/xdotool", "getwindowpid", browser_xid]).stdout.strip() == str(launched["pid"])
    request = api["read_json"](fixture / "browser-request.json")
    assert request["path"] == "/page?handoff=1" and "Chrome/" in request["userAgent"]
    assert "HeadlessChrome/" not in request["userAgent"]
    browser_image = "target/probes/workspace-link-browser-open.png"
    browser_text = fixture / "browser-page"

    def browser_painted():
        run(["/usr/bin/import", "-window", browser_xid, browser_image])
        run(["/usr/bin/env", "OMP_THREAD_LIMIT=1", "/usr/bin/tesseract", browser_image, browser_text, "--psm", "6"])
        return "browser handoff confirmed" in browser_text.with_suffix(".txt").read_text().lower()

    wait(browser_painted, "browser_visible_page")
    xdo("windowactivate", "--sync", xid)
    assert "enabled=0" in accessible("在系统浏览器打开", "inspect")
    click("在系统浏览器打开")
    assert len((fixture / "browser-launches.jsonl").read_text().splitlines()) == 1, "links.duplicate_system_open"
    run(["/usr/bin/import", "-window", xid, "target/probes/workspace-link-handoff-confirmed.png"])
    xdo("windowactivate", "--sync", browser_xid)
    xdo("key", "--clearmodifiers", "alt+F4")
    exited = wait(lambda: api["read_json"](fixture / "browser-exited.json"), "browser_physical_exit")
    assert exited["exitCode"] == 0, "links.browser_failed"

    def browser_group_exited():
        for line in run(["/usr/bin/ps", "-eo", "pid=,pgid=,stat="]).stdout.splitlines():
            _pid, group, state = line.split()
            if int(group) == launched["pid"] and not state.startswith("Z"):
                return False
        return True

    wait(browser_group_exited, "browser_children_exited")
    xdo("windowactivate", "--sync", xid)
    close()
    assert api["count"]("document_confirmed") == before_documents, "links.navigated_workspace"
    assert received() == before_input, "links.extra_pty_input"
    (fixture / "interaction-links-done").touch()
    return {
        "plainHttpAndEndpoint": True,
        "osc8ActualTargetOverridesDisplayText": True,
        "quotedFilePathAndSeparateLineColumn": True,
        "actualGtkClipboardReadback": True,
        "credentialTargetRejected": True,
        "escapeDismissesWithoutInput": True,
        "noWorkspaceNavigationOrExtraPtyInput": True,
        "systemBrowserOpenTested": True,
        "browserHandoff": {
            "gioAccepted": True,
            "graphicalChromeLoadedLocalPage": True,
            "noLaunchOrRequestBeforeApproval": True,
            "duplicateOpenPrevented": True,
            "browserAndProcessGroupExited": True,
            "privateXdgHandler": True,
        },
    }
