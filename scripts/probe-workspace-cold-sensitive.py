"""Two independent creation clients; only the original primary may ask consent."""
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import time


def reject_pending_pressure(pulse, env, root, pending, archive):
    # Real authenticated clients compete for a busy single-flight approval slot.
    # No request/response payload or authentication token is retained.
    clients = []
    try:
        for index in range(12):
            command = "open" if index % 2 else "new-tab"
            clients.append(subprocess.Popen([str(pulse), command, "sh"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True))
        deadline = time.monotonic() + 8
        for client in clients:
            output, _error = client.communicate(timeout=max(0.01, deadline - time.monotonic()))
            assert client.returncode != 0 and not output, "cold.pressure_borrowed_original_approval"
        assert pending.poll() is None and not (root / "unexpected-pty").exists()
        (archive / "pressure-result.json").write_text(json.dumps({"independent_clients": 12, "rejected": 12, "original_pending": True, "shell_starts": 0}) + "\n")
    finally:
        for client in clients:
            if client.poll() is None:
                client.terminate()
                client.wait(timeout=3)

def verify(mode, pulse, root, env, runtime, evidence, owned_guis, xdo, identity):
    scenario = mode.removeprefix("sensitive-")
    assert scenario in {"cancel", "disconnect", "refresh", "approve"}
    assert not runtime.exists()
    prefix = f"cli-cold-sensitive-{scenario}"
    archive = evidence / prefix
    archive.mkdir(exist_ok=True)
    shim = 'import os,sys; sys.stdin.buffer.read(1); os.execv(sys.argv[1], sys.argv[1:])'
    clients = []
    seen_guis = set()
    try:
        for _ in range(2):
            clients.append(subprocess.Popen(["/usr/bin/python3", "-c", shim, str(pulse), "new-tab", "sh"], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True))
        for client in clients:
            client.stdin.write("x")
            client.stdin.flush()
            client.stdin.close()
            client.stdin = None
        bus_name = json.loads(Path("apps/desktop/src-tauri/workspace/tauri.conf.json").read_text())["identifier"] + ".SingleInstance"
        end = time.monotonic() + 8
        owner = None
        while True:
            seen_guis.update(owned_guis())
            reply = subprocess.run(["/usr/bin/gdbus", "call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus", "--method", "org.freedesktop.DBus.GetConnectionUnixProcessID", bus_name], capture_output=True, text=True, timeout=2)
            seen_guis.update(owned_guis())
            match = re.fullmatch(r"\(uint32 ([1-9][0-9]*),\)\s*", reply.stdout)
            if reply.returncode == 0 and match:
                owner = int(match[1])
            finished = [client for client in clients if client.poll() is not None]
            if owner in owned_guis() and len(finished) == 1 and owned_guis() == [owner]:
                break
            assert len(finished) < 2, "cold.sensitive_both_requests_failed"
            assert time.monotonic() < end, "cold.sensitive_one_primary_observation_timeout"
            time.sleep(0.025)
        rejected = finished[0]
        output, error = rejected.communicate(timeout=3)
        assert rejected.returncode != 0 and not output, "cold.sensitive_secondary_borrowed_approval"
        pressure_flag = os.environ.get("PULSE_CLI_PENDING_PRESSURE", "0")
        assert pressure_flag in {"0", "1"}
        pressure_enabled = pressure_flag == "1"
        if pressure_enabled:
            archive = evidence / f"{prefix}-pressure"
            archive.mkdir(exist_ok=True)
        (archive / "secondary-client.log").write_text(f"exit={rejected.returncode}\n" + error)
        assert not (root / "unexpected-pty").exists(), "cold.sensitive_spawned_before_consent"
        assert len(seen_guis) == 2, "cold.sensitive_stimulus_not_two_gui_attempts"
        pending = next(client for client in clients if client is not rejected)
        def pressure():
            reject_pending_pressure(pulse, env, root, pending, archive)
        controller = runpy.run_path("scripts/probe-workspace-cold-new-tab.py")
        controller["verify"](f"new-tab-{scenario}", pending, pulse, root, env, runtime, archive, owned_guis, xdo, identity, pressure if pressure_enabled else None)
        (archive / "result.txt").write_text(f"independent_clients=2 gui_start_attempts=2 one_primary=true secondary_rejected=true original_consent={scenario} pending_pressure_rejections={12 if pressure_enabled else 0} max_shell_starts=1 actual_exit=true\n")
    finally:
        for client in clients:
            if client.poll() is None:
                client.terminate()
                client.wait(timeout=3)
