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


def verify_directory(pulse, root, env, runtime, evidence, owned_guis, xdo):
    """Reuse the real default editor and native lifecycle for cold/warm pulse ."""
    import json
    import runpy
    from pathlib import Path
    shared = runpy.run_path('scripts/probe-workspace-cold-split.py')
    directory = root / 'CLI 中文 folder'
    directory.mkdir(mode=0o700)
    pid_file = root / 'unexpected-pty'
    native = []
    results = []

    def launch():
        return subprocess.Popen([str(pulse), '.'], cwd=directory, env=env,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    client = launch()
    try:
        shared['verify_missing_default'](client, root, runtime, evidence, owned_guis, xdo)
    finally:
        if client.poll() is None:
            client.kill()
            client.wait(timeout=3)
    shared['prepare_default'](pulse, root, env, runtime, evidence, owned_guis, xdo)
    # Start the cancellation case warm; later cases still exercise cold bootstrap.
    ready = subprocess.run([str(pulse), 'profiles'], env=env, capture_output=True, text=True, timeout=15)
    assert ready.returncode == 0
    first_app = wait(lambda: (owned_guis() or [None])[0])
    wait(lambda: shared['_accessible'](first_app, 'POSIX sh'))
    for scenario in ['cancel', 'refresh', 'missing', 'cold-approve', 'warm-approve']:
        client = launch()
        app = None
        xid = None
        try:
            app = wait(lambda: (owned_guis() or [None])[0])
            assert owned_guis() == [app]
            xid = wait(lambda: shared['_window'](app, xdo))
            xdo('windowactivate', '--sync', xid)
            wait(lambda: shared['_accessible'](app, '允许 CLI 创建本地终端？'))
            wait(lambda: shared['_accessible'](app, 'sh\n' + str(directory)))
            assert (pid_file.read_text().splitlines() if pid_file.exists() else []) == native
            subprocess.run(['/usr/bin/import', '-window', xid, str(evidence / f'cwd-{scenario}-consent.png')], check=True, timeout=3)
            if scenario == 'refresh':
                xdo('key', '--clearmodifiers', 'ctrl+shift+r')
            else:
                if scenario == 'missing':
                    directory.rmdir()
                button = '取消' if scenario == 'cancel' else '创建终端'
                assert shared['_accessible'](app, button, 'focus')
                xdo('key', '--clearmodifiers', 'space')
            output, error = client.communicate(timeout=10)
            (evidence / f'cwd-{scenario}-client.log').write_text(output + error)
            accepted = scenario.endswith('approve')
            assert (client.returncode == 0) == accepted
            if accepted:
                assert re.fullmatch(r'session [1-9][0-9]*\n', output)
                wait(lambda: pid_file.exists() and len(pid_file.read_text().splitlines()) == len(native) + 1)
                native = pid_file.read_text().splitlines()
                assert len(set(native)) == len(native)
                assert all(Path(f'/proc/{pid}/cwd').resolve(strict=True) == directory for pid in native)
            else:
                assert not output and not pid_file.exists()
            results.append(dict(scenario=scenario, accepted=accepted, native_count=len(native), exact_directory_visible=True))
            if scenario == 'missing':
                directory.mkdir(mode=0o700)
            if scenario != 'cold-approve':
                shared['_close'](app, runtime, owned_guis, xdo)
                assert all(not Path(f'/proc/{pid}').exists() for pid in native)
        except Exception:
            if client.poll() is not None:
                output, error = client.communicate(timeout=3)
                (evidence / f'cwd-{scenario}-failure-client.log').write_text(f'exit={client.returncode}\n' + output + error)
            else:
                (evidence / f'cwd-{scenario}-failure-client.log').write_text('client still pending\n')
            if xid:
                subprocess.run(['/usr/bin/import', '-window', xid, str(evidence / f'cwd-{scenario}-failure.png')], timeout=3, check=False, capture_output=True)
            raise
        finally:
            if client.poll() is None:
                client.kill()
                client.wait(timeout=3)
    (evidence / 'cwd-result.json').write_text(json.dumps(dict(missing_default_zero_pty=True, checks=results, native_exit=True, endpoints_drained=True), ensure_ascii=False, indent=2) + '\n')
