"""True independent CLI execs released together; no action/claim retry."""
import json
import re
from pathlib import Path
import os
import subprocess
import time


def wait(predicate, seconds=8):
    end = time.monotonic() + seconds
    while True:
        result = predicate()
        if result:
            return result
        assert time.monotonic() < end, "cold.concurrent_observation_timeout"
        time.sleep(0.025)


def verify(mode, pulse, root, env, runtime, evidence, owned_guis, xdo, identity, profiles_receipt):
    pair = mode.removeprefix("concurrent-")
    methods = [{"f": "focus", "p": "profiles"}[kind] for kind in pair]
    clients = []
    prefix = f"cli-cold-concurrent-{pair}"
    # Each shim waits before execing the exact installed-like CLI sibling.
    # CLI production absence/spawn/hello/single-use claim runs unmodified.
    shim = 'import os,sys; sys.stdin.buffer.read(1); os.execv(sys.argv[1], sys.argv[1:])'
    try:
        assert not runtime.exists(), "cold.not_absent"
        for action in methods:
            clients.append(subprocess.Popen(["/usr/bin/python3", "-c", shim, str(pulse), action], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True))
        for client in clients:
            client.stdin.write("x")
            client.stdin.flush()
        name = json.loads(Path("apps/desktop/src-tauri/workspace/tauri.conf.json").read_text())["identifier"] + ".SingleInstance"
        seen_guis = set()
        def owner():
            seen_guis.update(owned_guis())
            result = subprocess.run(["/usr/bin/gdbus", "call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus", "--method", "org.freedesktop.DBus.GetConnectionUnixProcessID", name], capture_output=True, text=True, timeout=2)
            seen_guis.update(owned_guis())
            match = re.fullmatch(r"\(uint32 ([1-9][0-9]*),\)\s*", result.stdout)
            if result.returncode == 0 and match:
                pid = int(match[1])
                return pid if pid in owned_guis() else None
            return None
        app = wait(owner)

        def window():
            try:
                ids = xdo("search", "--onlyvisible", "--pid", app).splitlines()
                ids = [xid for xid in ids if xdo("getwindowname", xid) == "PulseTerm · 工作区预览"]
                return ids[0] if len(ids) == 1 else None
            except subprocess.CalledProcessError:
                return None

        xid = wait(window)
        assert xdo("getwindowpid", xid) == str(app)
        xdo("windowactivate", "--sync", xid)
        results = []
        for index, (client, action) in enumerate(zip(clients, methods)):
            client.stdin.close()
            client.stdin = None
            output, error = client.communicate(timeout=20)
            (evidence / f"{prefix}-client-{index}.log").write_text(f"exit={client.returncode}\n" + output + error)
            results.append(client.returncode)
            if client.returncode == 0:
                if action == "profiles":
                    profiles_receipt(output)
                else:
                    assert output == "focus requested\n"
        assert results == [0, 0], "cold.concurrent_request_not_confirmed"
        assert owned_guis() == [app], "cold.multiple_surviving_primary"
        assert not (root / "unexpected-pty").exists(), "cold.readonly_created_pty"
        socket, token = runtime / "control.sock", runtime / "control.token"
        original = identity(socket), identity(token), token.read_bytes()
        for action in ["profiles", "focus"]:
            result = subprocess.run([str(pulse), action], env=env, capture_output=True, text=True, timeout=8)
            assert result.returncode == 0
            if action == "profiles":
                profiles_receipt(result.stdout)
            assert (identity(socket), identity(token), token.read_bytes()) == original
            assert owned_guis() == [app]
        assert not (root / "unexpected-pty").exists()
        time.sleep(0.25)
        subprocess.run(["/usr/bin/import", "-window", xid, str(evidence / f"{prefix}-rendered.png")], check=True, timeout=3)
        xdo("windowclose", xid)

        def exited():
            pid, status = os.waitpid(app, os.WNOHANG)
            if pid:
                assert os.waitstatus_to_exitcode(status) == 0
                return True
            return False

        wait(exited, 10)
        assert not socket.exists() and not token.exists()
        assert len(seen_guis) == 2, "cold.concurrent_stimulus_not_two_gui_attempts"
        (evidence / f"{prefix}-result.txt").write_text(f"primary_pid={app} independent_clients=2 gui_start_attempts={len(seen_guis)} results={results} warm_continuity=true shell_starts=0 actual_gui_exit=true endpoints_drained=true\n")
    finally:
        for client in clients:
            if client.poll() is None:
                client.terminate()
                client.wait(timeout=3)
