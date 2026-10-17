"""Real X11 cold new-tab stimuli; no backend bypass or plaintext IPC."""
import os
import re
import subprocess
import time


def wait(predicate, seconds=8):
    deadline = time.monotonic() + seconds
    while True:
        value = predicate()
        if value:
            return value
        assert time.monotonic() < deadline, "cold.new_tab_observation_timeout"
        time.sleep(0.05)


def verify(mode, client, pulse, root, env, runtime, evidence, owned_guis, xdo, identity, before_consent=None):
    prefix = f"cli-cold-{mode}"
    scenario = mode.rsplit("-", 1)[1]
    assert mode in {f"{command}-{action}" for command in ["new-tab", "open"] for action in ["cancel", "disconnect", "refresh", "approve"]}
    pid_file = root / "unexpected-pty"
    app = wait(lambda: (owned_guis() or [None])[0])
    assert owned_guis() == [app], "cold.multiple_primary"

    def window():
        try:
            windows = xdo("search", "--onlyvisible", "--pid", app).splitlines()
            windows = [value for value in windows if xdo("getwindowname", value) == "PulseTerm · 工作区预览"]
            return windows[0] if len(windows) == 1 else None
        except subprocess.CalledProcessError:
            return None

    xid = wait(window)
    assert xdo("getwindowpid", xid) == str(app)
    xdo("windowactivate", "--sync", xid)
    image = evidence / f"{prefix}-confirmation.png"

    def drawn_dialog():
        assert client.poll() is None, "cold.new_tab_rejected_before_consent"
        subprocess.run(["/usr/bin/import", "-window", xid, str(image)], check=True, timeout=3)
        # Existing consent controller's Cancel is the initial focused button.
        # Require actually rendered modal text, not merely an enqueued event.
        text = subprocess.check_output(["/usr/bin/tesseract", str(image), "stdout", "-l", "eng", "--psm", "11"], env={**os.environ, "OMP_THREAD_LIMIT": "1"}, stderr=subprocess.DEVNULL, text=True, timeout=3)
        return "CLI" in text and re.search(r"\bsh\b", text) is not None

    wait(drawn_dialog)
    # Read-only page confirmation alone cannot authorize startup or create a PTY.
    result = subprocess.run([str(pulse), "profiles"], env=env, capture_output=True, text=True, timeout=8)
    assert result.returncode == 0 and '"sh"' in result.stdout
    socket, token = runtime / "control.sock", runtime / "control.token"
    original = identity(socket), identity(token), token.read_bytes()
    assert not pid_file.exists(), "cold.spawned_before_consent"
    if before_consent is not None:
        before_consent()
        assert client.poll() is None and not pid_file.exists(), "cold.pressure_lost_original_consent"
        assert (identity(socket), identity(token), token.read_bytes()) == original
        wait(drawn_dialog)
    if scenario == "cancel":
        xdo("key", "--clearmodifiers", "Return")
    elif scenario == "disconnect":
        client.terminate()
    elif scenario == "refresh":
        xdo("key", "--clearmodifiers", "ctrl+shift+r")
    else:
        # Explicit approval after the obsolete 15s bootstrap response budget.
        time.sleep(16)
        assert client.poll() is None and not pid_file.exists()
        xdo("key", "--clearmodifiers", "Tab", "Return")
    output, error = client.communicate(timeout=8)
    (evidence / f"{prefix}-client.log").write_text(output + error)
    native = None
    if scenario != "approve":
        assert client.returncode != 0, "cold.cancel_accepted"
        time.sleep(0.5)
        assert not pid_file.exists(), "cold.cancel_created_pty"
        # A fresh page/read can be used normally, but cannot resurrect old intent.
        result = subprocess.run([str(pulse), "profiles"], env=env, capture_output=True, text=True, timeout=8)
        assert result.returncode == 0
        assert not pid_file.exists()
    else:
        assert client.returncode == 0 and re.fullmatch(r"session [1-9][0-9]*\n", output)
        wait(pid_file.exists)
        pids = pid_file.read_text().splitlines()
        assert len(pids) == 1, "cold.duplicate_native_start"
        native = int(pids[0])
        os.kill(native, 0)
        input_file = root / "unexpected-pty.input"
        # Wait for actual input admission, never resend an uncertain write.
        time.sleep(1)
        for turn in range(2):
            if turn:
                xdo("key", "--clearmodifiers", "ctrl+shift+r")
                time.sleep(1)
            xdo("mousemove", "--window", xid, 650, 340, "click", 1)
            text = f"cold-new-tab-echo-{turn}"
            xdo("type", "--clearmodifiers", "--delay", 1, text)
            xdo("key", "--clearmodifiers", "Return")
            wait(lambda: input_file.exists() and text in input_file.read_text().splitlines())
            assert pid_file.read_text().splitlines() == pids
            os.kill(native, 0)
        assert input_file.read_text().splitlines() == ["cold-new-tab-echo-0", "cold-new-tab-echo-1"]
        def final_echo_drawn():
            shot = evidence / f"{prefix}-rendered.png"
            subprocess.run(["/usr/bin/import", "-window", xid, str(shot)], check=True, timeout=3)
            crop = root / "final-echo-ocr.png"
            subprocess.run(["/usr/bin/convert", str(shot), "-crop", "740x90+200+149", "-negate", "-resize", "300%", str(crop)], check=True, timeout=3)
            text = subprocess.check_output(["/usr/bin/tesseract", str(crop), "stdout", "--psm", "6", "-l", "eng"], env={**os.environ, "OMP_THREAD_LIMIT": "1"}, stderr=subprocess.DEVNULL, text=True, timeout=3)
            return "cold-echo:cold-new-tab-echo-1" in text.replace(" ", "")
        wait(final_echo_drawn)
        (evidence / f"{prefix}-input.txt").write_text(input_file.read_text())
    assert owned_guis() == [app]
    assert (identity(socket), identity(token), token.read_bytes()) == original
    xdo("windowclose", xid)

    def exited():
        waited, status = os.waitpid(app, os.WNOHANG)
        if waited:
            assert os.waitstatus_to_exitcode(status) == 0
            return True
        return False

    wait(exited, 10)
    assert not socket.exists() and not token.exists()
    if native:
        assert not os.path.exists(f"/proc/{native}"), "cold.native_not_reaped"
    (evidence / f"{prefix}-result.txt").write_text(f"primary_pid={app} native_pid={native} consent={mode} one_primary=true actual_exit=true endpoints_drained=true\n")
