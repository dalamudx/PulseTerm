"""Actual same-process WebViews: empty-window approval, precise routing, and exit."""
import json
import os
import re
import subprocess
import time


def verify(pulse, root, env, runtime, evidence, owned_guis, xdo, identity):
    prefix = "cli-multiwindow"
    clients = []
    app = None
    primary_title, secondary_title = "PulseTerm · 工作区预览", "PulseTerm · 工作区"

    def wait(probe, tag, seconds=12):
        deadline = time.monotonic() + seconds
        while True:
            result = probe()
            if result:
                return result
            assert time.monotonic() < deadline, "multiwindow.timeout." + tag
            time.sleep(.05)

    def run(*args):
        result = subprocess.run([str(pulse), *args], env=env, text=True, capture_output=True, timeout=8)
        return result

    def start(*args):
        client = subprocess.Popen([str(pulse), *args], env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        clients.append(client)
        return client

    def finish(client, tag, accepted):
        output, error = client.communicate(timeout=15)
        (evidence / f"{prefix}-{tag}-client.log").write_text(f"exit={client.returncode}\n" + output + error)
        assert (client.returncode == 0) == accepted, "multiwindow.client." + tag
        return output.strip()

    def labels():
        result = run("windows")
        assert result.returncode == 0, "multiwindow.list_rejected"
        value = json.loads(result.stdout)
        assert set(value) == {"version", "windows"} and value["version"] == 1
        assert len(set(value["windows"])) == len(value["windows"])
        return set(value["windows"])

    def windows():
        try:
            return {xid for xid in xdo("search", "--onlyvisible", "--pid", app).splitlines()
                    if xdo("getwindowname", xid) in (primary_title, secondary_title)}
        except subprocess.CalledProcessError:
            return set()

    def accessible(title, name, action=None):
        query = ["target/probes/x11-status", str(app), name]
        if action:
            query.append(action)
        return subprocess.run(query, env={**os.environ, "PULSETERM_PROBE_FRAME": title}, text=True, capture_output=True, timeout=3)

    def ready(title):
        result = accessible(title, "管理", "inspect")
        return (result.returncode == 0 and "enabled=1" in result.stdout and "showing=1" in result.stdout
                and accessible(title, "没有匹配的主机").returncode == 0)

    def consent(title, text, client):
        def drawn():
            assert client.poll() is None, "multiwindow.rejected_before_consent"
            return accessible(title, text).returncode == 0
        wait(drawn, text)

    def click(xid, title, name):
        xdo("windowactivate", "--sync", xid)
        assert accessible(title, name, "focus").returncode == 0, "multiwindow.button." + name
        xdo("key", "--clearmodifiers", "space")

    def pids():
        path = root / "unexpected-pty"
        return [int(line) for line in path.read_text().splitlines()] if path.exists() else []

    def capture(xid, tag):
        xdo("windowactivate", "--sync", xid)
        subprocess.run(["/usr/bin/import", "-window", xid, str(evidence / f"{prefix}-{tag}.png")], timeout=3, check=True)

    def close(xid):
        xdo("windowactivate", "--sync", xid)
        xdo("key", "--clearmodifiers", "alt+F4")
        wait(lambda: xid not in windows(), "window_destroyed")

    def input_once(xid, pid, text):
        xdo("windowactivate", "--sync", xid)
        xdo("mousemove", "--window", xid, 550, 300, "click", 1)
        xdo("type", "--clearmodifiers", "--delay", 1, text)
        xdo("key", "--clearmodifiers", "Return")
        path = root / f"unexpected-pty.{pid}.input"
        wait(lambda: path.exists() and text in path.read_text().splitlines(), "input." + text)

    try:
        assert run("focus").returncode == 0
        app = wait(lambda: (owned_guis() or [None])[0], "gui")
        assert owned_guis() == [app]
        a = wait(lambda: next(iter(windows())) if len(windows()) == 1 else None, "primary")
        wait(lambda: ready(primary_title), "primary_ready")
        assert labels() == {"workspace"} and not pids()
        socket, token = runtime / "control.sock", runtime / "control.token"
        original = identity(socket), identity(token), token.read_bytes()
        for scenario in ("cancel", "disconnect", "refresh"):
            client = start("new-window")
            consent(primary_title, "允许 CLI 新建窗口？", client)
            assert len(windows()) == 1 and not pids()
            capture(a, "new-window-" + scenario)
            if scenario == "cancel":
                click(a, primary_title, "取消")
            elif scenario == "disconnect":
                client.terminate()
            else:
                xdo("windowactivate", "--sync", a)
                xdo("key", "--clearmodifiers", "ctrl+shift+r")
            finish(client, "new-window-" + scenario, False)
            wait(lambda: accessible(primary_title, "允许 CLI 新建窗口？").returncode != 0, "dialog_closed")
            wait(lambda: ready(primary_title), "primary_recovered")
            assert labels() == {"workspace"} and not pids()
        client = start("-w", "workspace", "new-tab", "sh")
        consent(primary_title, "允许 CLI 创建本地终端？", client)
        click(a, primary_title, "创建终端")
        session_a = finish(client, "session-a", True).removeprefix("session ")
        assert re.fullmatch(r"[1-9][0-9]*", session_a)
        wait(lambda: len(pids()) == 1, "first_pty")
        pid_a = pids()[0]
        input_once(a, pid_a, "multiwindow-a-before")
        client = start("-w", "workspace", "new-window")
        consent(primary_title, "允许 CLI 新建窗口？", client)
        click(a, primary_title, "新建窗口")
        label_b = finish(client, "create-b", True)
        assert re.fullmatch(r"workspace-[0-9a-f-]{36}", label_b)
        b = wait(lambda: next(iter(windows() - {a})) if len(windows()) == 2 else None, "secondary")
        wait(lambda: ready(secondary_title), "secondary_ready")
        assert labels() == {"workspace", label_b} and pids() == [pid_a]
        assert accessible(primary_title, "会话 " + session_a).returncode == 0
        assert accessible(secondary_title, "会话 " + session_a).returncode != 0
        capture(b, "empty-b")
        assert run("profiles").returncode != 0, "multiwindow.guessed_default"
        assert run("-w", label_b, "focus-tab", session_a).returncode != 0, "multiwindow.foreign_session_selected"
        assert run("-w", "workspace-00000000-0000-0000-0000-000000000000", "new-tab", "sh").returncode != 0
        client = start("-w", label_b, "new-tab", "sh")
        consent(secondary_title, "允许 CLI 创建本地终端？", client)
        click(b, secondary_title, "创建终端")
        session_b = finish(client, "session-b", True).removeprefix("session ")
        assert re.fullmatch(r"[1-9][0-9]*", session_b) and session_b != session_a
        wait(lambda: len(pids()) == 2, "second_pty")
        pid_b = pids()[1]
        input_once(b, pid_b, "multiwindow-b-before")
        for xid, title, pid, marker in [(a, primary_title, pid_a, "a"), (b, secondary_title, pid_b, "b")]:
            xdo("windowactivate", "--sync", xid)
            xdo("key", "--clearmodifiers", "ctrl+shift+r")
            wait(lambda: ready(title), "refreshed_" + marker)
            time.sleep(1)  # Presentation only; each native input is sent exactly once.
            input_once(xid, pid, "multiwindow-" + marker + "-after")
            capture(xid, "refreshed-" + marker)
        assert pids() == [pid_a, pid_b], "multiwindow.refresh_started_pty"
        assert accessible(secondary_title, "会话 " + session_a).returncode != 0
        assert accessible(primary_title, "会话 " + session_b).returncode != 0
        close(a)
        wait(lambda: labels() == {label_b}, "primary_retired")
        assert owned_guis() == [app] and run("profiles").returncode == 0
        assert run("-w", "workspace", "focus").returncode != 0
        input_once(b, pid_b, "multiwindow-b-after-primary-close")
        client = start("-w", label_b, "new-window")
        consent(secondary_title, "允许 CLI 新建窗口？", client)
        click(b, secondary_title, "新建窗口")
        label_c = finish(client, "create-c", True)
        assert re.fullmatch(r"workspace-[0-9a-f-]{36}", label_c) and label_c != label_b
        c = wait(lambda: next(iter(windows() - {b})) if len(windows()) == 2 else None, "third")
        assert labels() == {label_b, label_c} and pids() == [pid_a, pid_b]
        close(c)
        wait(lambda: labels() == {label_b}, "third_retired")
        assert run("-w", label_c, "new-tab", "sh").returncode != 0
        assert (identity(socket), identity(token), token.read_bytes()) == original
        assert (root / f"unexpected-pty.{pid_a}.input").read_text().splitlines() == ["multiwindow-a-before", "multiwindow-a-after"]
        assert (root / f"unexpected-pty.{pid_b}.input").read_text().splitlines() == ["multiwindow-b-before", "multiwindow-b-after", "multiwindow-b-after-primary-close"]
        close(b)
        def exited():
            waited, status = os.waitpid(app, os.WNOHANG)
            if waited:
                assert os.waitstatus_to_exitcode(status) == 0, "multiwindow.gui_bad_exit"
                return True
            return False
        wait(exited, "gui_exit")
        assert not socket.exists() and not token.exists()
        assert all(not os.path.exists(f"/proc/{pid}") for pid in (pid_a, pid_b))
        (evidence / f"{prefix}-result.json").write_text(json.dumps(dict(gui_pid=app, window_labels=["workspace", label_b, label_c], native_pids=[pid_a, pid_b], sessions=[session_a, session_b], cancellation_disconnect_navigation_created_nothing=True, exact_window_routes=True, empty_window_has_no_foreign_session=True, foreign_session_rejected=True, refresh_same_ptys=True, independent_input=True, primary_close_preserves_secondary=True, retired_labels_rejected=True, endpoints_preserved_until_last_close=True, actual_gui_exit=True, native_exit=True, endpoints_drained=True), indent=2) + "\n")
    finally:
        if app and owned_guis():
            for xid in windows():
                try:
                    capture(xid, "failure-" + xid)
                except (OSError, subprocess.SubprocessError):
                    pass
        for client in clients:
            if client.poll() is None:
                client.kill()
                client.wait(timeout=3)
