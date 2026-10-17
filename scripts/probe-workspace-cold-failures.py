#!/usr/bin/env python3
"""Actual CLI failures: process/connect receipts, no payload/token tracing."""
import ctypes
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import sys
import subprocess
import tempfile
import threading
import time

mode = sys.argv[1] if len(sys.argv) == 2 else "failures"
assert len(sys.argv) <= 2 and mode in {"failures", "unbound"}, "cold.invalid_probe_mode"
assert ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) == 0
parent = Path(os.environ["TMPDIR"])
assert parent.is_relative_to(Path("/data/temp"))
root = Path(tempfile.mkdtemp(prefix="f.", dir=parent))
root.chmod(0o700)
evidence = Path("target/probes")
evidence.mkdir(parents=True, exist_ok=True)
bundle = root / "bundle"
bundle.mkdir(mode=0o700)
for name in ("pulse", "pulseterm-workspace"):
    shutil.copy2(Path("target/debug") / name, bundle / name)
pulse, gui = bundle / "pulse", bundle / "pulseterm-workspace"
if mode == "unbound":
    commands = tuple((command, direction, "sh") for command in ("split", "split-pane") for direction in ("-v", "-h"))
    commands += (("ssh", "018f5691-0059-7a69-a45a-7c4d6a1cfa51", "--identity", "018f5691-0059-7a69-a45a-7c4d6a1cfa52"),)
else:
    commands = (("focus",), ("profiles",), ("new-tab", "sh"))
result_file = "cli-cold-unbound-result.json" if mode == "unbound" else "cli-cold-failures-result.json"
results = []


def snapshot(path):
    if not path.exists() and not path.is_symlink():
        return {}
    image = {}
    for item in [path, *sorted(path.rglob("*"))]:
        info = item.lstat()
        image[str(item.relative_to(path))] = (info.st_dev, info.st_ino, info.st_uid, info.st_mode, info.st_nlink,
                                            os.readlink(item) if item.is_symlink() else item.read_bytes() if item.is_file() else None)
    return image


def children():
    found = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit() or int(proc.name) == os.getpid():
            continue
        try:
            env = (proc / "environ").read_bytes().split(b"\0")
            if any(value.startswith(b"XDG_RUNTIME_DIR=" + os.fsencode(root) + b"/") for value in env):
                found.append(int(proc.name))
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            pass
    return found


def run_case(kind, command):
    case = root / str(len(results))
    case.mkdir(mode=0o700)
    env = os.environ.copy()
    for variable, name in (("HOME", "home"), ("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")):
        directory = case / name
        directory.mkdir(mode=0o700)
        env[variable] = str(directory)
    env.update(TOKIO_WORKER_THREADS="2", DBUS_SESSION_BUS_ADDRESS="unix:path=" + str(case / "missing-bus"))
    runtime = Path(env["XDG_RUNTIME_DIR"]) / "pulseterm-workspace-cli"
    if kind != "missing-runtime":
        runtime.mkdir(mode=0o700)
    endpoint, token = runtime / "control.sock", runtime / "control.token"
    assert len(os.fsencode(endpoint)) < 108
    listener, thread = None, None
    stopped = threading.Event()
    accepted, failures = [], []
    if kind not in {"no-broker", "missing-runtime", "empty-runtime"}:
        if kind != "partial-token":
            if kind == "regular-socket":
                endpoint.write_bytes(b"not a socket")
            else:
                listener = socket.socket(socket.AF_UNIX)
                listener.bind(str(endpoint))
            endpoint.chmod(0o600)
        if kind != "partial-socket":
            token.write_text(os.urandom(32).hex())
            token.chmod(0o600)
        if kind == "unsafe-directory":
            runtime.chmod(0o755)
        elif kind == "unsafe-token":
            token.chmod(0o644)
        elif kind == "unsafe-socket":
            endpoint.chmod(0o666)
        elif kind == "short-token":
            token.write_bytes(b"short")
        elif kind == "nonhex-token":
            token.write_bytes(b"z" * 64)
        elif kind == "hardlinked-token":
            os.link(token, runtime / "token-peer")
        elif kind == "symlink-token":
            token.rename(runtime / "real-token")
            token.symlink_to("real-token")
        elif kind == "symlink-directory":
            actual = runtime.with_name("real-runtime")
            runtime.rename(actual)
            runtime.symlink_to(actual.name)
        if kind in {"peer-eof", "peer-timeout"}:
            assert listener is not None
            peer_listener = listener
            peer_listener.listen(4)
            peer_listener.settimeout(0.1)

            def peer():
                try:
                    while not stopped.is_set():
                        try:
                            connection, _ = peer_listener.accept()
                        except socket.timeout:
                            continue
                        with connection:
                            accepted.append(1)
                            if kind == "peer-timeout":
                                stopped.wait(7)
                except Exception as error:
                    failures.append(type(error).__name__)

            thread = threading.Thread(target=peer)
            thread.start()
    before = snapshot(runtime)
    trace = case / "process.trace"
    try:
        outcome = subprocess.run(["/usr/bin/strace", "-f", "-qq", "-s", "256", "-e", "trace=process,connect", "-o", str(trace), str(pulse), *command],
                                 env=env, capture_output=True, text=True, timeout=12)
        assert outcome.returncode == 1 and not outcome.stdout, (kind, command, outcome.returncode)
        assert outcome.stderr == "pulse: request not confirmed\n", (kind, outcome.stderr)
        records = trace.read_text()
        execs = re.findall(r'execve\("([^"\n]+)"', records)
        assert execs.count(str(pulse)) == 1, (kind, execs)
        expected = 1 if kind == "no-broker" else 0
        assert execs.count(str(gui)) == expected and set(execs) <= {str(pulse), str(gui)}, (kind, execs)
        connects = sum("connect(" in line and str(endpoint) in line for line in records.splitlines())
        assert connects == (1 if kind in {"connection-refused", "peer-eof", "peer-timeout"} else 0), (kind, connects)
        assert snapshot(runtime) == before, kind
        stopped.set()
        if thread:
            thread.join(timeout=3)
            assert not thread.is_alive() and not failures and accepted == [1], (kind, accepted, failures)
        assert not children(), (kind, children())
        results.append({"case": kind, "command": list(command), "exit": 1, "gui_execs": expected, "endpoint_connects": connects,
                        "endpoint_preserved": True, "actual_descendants_exited": True})
        # Never archive raw argv/environment, token bytes or wire buffers.
        (evidence / result_file).write_text(json.dumps(results, indent=2) + "\n")
    finally:
        stopped.set()
        if thread:
            thread.join(timeout=8)
            assert not thread.is_alive(), "cold.failure_peer_not_exited"
        if listener:
            listener.close()


try:
    if mode == "unbound":
        for kind in ("missing-runtime", "empty-runtime"):
            for command in commands:
                run_case(kind, command)
        assert len(results) == 10
        print("workspace.cli_cold_unbound_10_valid_split_and_ssh_zero_gui_zero_connect_no_endpoint_write_and_resource_exit_confirmed")
    else:
        for kind in ("partial-token", "partial-socket", "regular-socket", "unsafe-directory", "unsafe-token", "unsafe-socket",
                     "short-token", "nonhex-token", "hardlinked-token", "symlink-token", "symlink-directory", "connection-refused", "peer-eof", "peer-timeout"):
            for command in commands:
                run_case(kind, command)
        run_case("no-broker", ("focus",))
        assert len(results) == 43
        print("workspace.cli_cold_43_actual_failure_cases_no_warm_retry_no_endpoint_repair_and_resource_exit_confirmed")
finally:
    deadline = time.monotonic() + 5
    while True:
        for pid in children():
            try:
                os.kill(pid, signal.SIGTERM if time.monotonic() < deadline else signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            if os.waitpid(-1, os.WNOHANG)[0]:
                continue
            assert time.monotonic() < deadline + 3, "cold.failure_descendant_cleanup_timeout"
            time.sleep(0.025)
        except ChildProcessError:
            assert not children()
            break
    assert root.parent == parent and root.name.startswith("f.")
    shutil.rmtree(root)
    assert not root.exists()
