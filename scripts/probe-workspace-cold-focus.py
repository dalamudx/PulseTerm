#!/usr/bin/env python3
"""Isolated X11/D-Bus: only pulse may start GUI; metadata is not execution."""
import ctypes
import json
import os
from pathlib import Path
import shutil
import sys
import signal
import subprocess
import tempfile
import time

# Adopt the cold GUI when its CLI parent exits so actual GUI exit is waitable.
assert ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) == 0
cold_action = sys.argv[1] if len(sys.argv) == 2 else "focus"
assert len(sys.argv) <= 2 and (cold_action in {"focus", "profiles", "new-tab-cancel", "new-tab-disconnect", "new-tab-refresh", "new-tab-approve", "open-cancel", "open-disconnect", "open-refresh", "open-approve", "concurrent-ff", "concurrent-pp", "concurrent-fp", "sensitive-cancel", "sensitive-disconnect", "sensitive-refresh", "sensitive-approve", "chain-syntax", "multiwindow", "cwd"} or cold_action in {f"{kind}-{direction}-{scenario}" for kind in ["split", "chain"] for direction in ["row", "column"] for scenario in ["cancel", "partial", "disconnect", "refresh", "approve"]})
default_split = os.environ.get("PULSE_CLI_PROBE_SPLIT_DEFAULT")
assert default_split is None or (default_split == "saved" and cold_action.startswith(("split-", "chain-"))) or (default_split == "unset" and cold_action.startswith("split-")), "cold.invalid_default_split_mode"
evidence_prefix = f"cli-cold-{cold_action}"

def profiles_receipt(text):
    offers = json.loads(text)
    assert isinstance(offers, dict) and set(offers) == {"version", "profiles"}
    assert type(offers["version"]) is int and offers["version"] == 1
    profiles = offers["profiles"]
    assert isinstance(profiles, list) and all(isinstance(value, str) for value in profiles)
    assert len(profiles) == len(set(profiles)) and "sh" in profiles
    assert set(profiles) <= {"bash", "zsh", "fish", "sh", "pwsh"}
    return profiles

source = Path(os.environ.get("PULSE_CLI_PROBE_BIN_DIR", "target/debug")).resolve(strict=True)
assert all((source / name).is_file() for name in ["pulse", "pulseterm-workspace"]), "cold.fixture_missing_binary"
evidence = Path("target/probes")
run_tag = os.environ.get("PULSE_CLI_PROBE_RUN")
if run_tag is not None:
    assert 0 < len(run_tag) <= 80 and run_tag not in {".", ".."} and all(c.isascii() and (c.isalnum() or c in "._-") for c in run_tag), "cold.invalid_run_tag"
    evidence = evidence / "windows-cli" / run_tag / cold_action
evidence.mkdir(parents=True, exist_ok=run_tag is None)
root = Path(tempfile.mkdtemp(prefix="c.", dir=os.environ["TMPDIR"]))
assert root.is_relative_to(Path("/data/temp"))
root.chmod(0o700)
# Cargo debug ELF names hard-link its deps artifact. Exercise the exact same
# compiled production binaries in an install-like private, single-link layout;
# do not relax the launcher's rejection or use a alternate mock executable.
bundle = root / "bundle"
bundle.mkdir(mode=0o700)
for name in ["pulse", "pulseterm-workspace"]:
    shutil.copy2(source / name, bundle / name)
pulse = bundle / "pulse"
gui = bundle / "pulseterm-workspace"
env = os.environ.copy()
for name, directory in [("HOME", "home"), ("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")]:
    path = root / directory
    path.mkdir(mode=0o700)
    env[name] = str(path)
bin_dir = root / "bin"
bin_dir.mkdir(mode=0o700)
shell = bin_dir / "sh"
shell.write_text('#!/bin/sh\nprintf "unexpected shell\\n" > "$PULSETERM_FIXTURE_PID"\nexit 0\n')
shell.chmod(0o700)
env.update(PATH=str(bin_dir), SHELL=str(shell), PULSETERM_FIXTURE_PID=str(root / "unexpected-pty"))
if cold_action.startswith(("new-tab-", "open-", "sensitive-")) or cold_action == "cwd":
    shell.write_text('#!/bin/sh\nprintf "%s\\n" "$$" >> "$PULSETERM_FIXTURE_PID"\nprintf "cold-new-tab-ready\\r\\n"\nwhile IFS= read -r line; do printf "%s\\n" "$line" >> "$PULSETERM_FIXTURE_PID.input"; printf "cold-echo:%s\\r\\n" "$line"; done\n')
if cold_action.startswith(("split-", "chain-")) or cold_action == "multiwindow":
    shell.write_text('#!/bin/sh\nprintf "%s\\n" "$$" >> "$PULSETERM_FIXTURE_PID"\nprintf "cold-split-ready:%s\\r\\n" "$$"\nwhile IFS= read -r line; do printf "%s\\n" "$line" >> "$PULSETERM_FIXTURE_PID.$$.input"; printf "cold-echo:%s\\r\\n" "$line"; done\n')
runtime = Path(env["XDG_RUNTIME_DIR"]) / "pulseterm-workspace-cli"
assert len(os.fsencode(runtime / "control.sock")) < 108, "cold.fixture_socket_path_long"
app = None
xid = None
client = None
decoy = None

def xdo(*args):
    return subprocess.check_output(["/usr/bin/xdotool", *map(str, args)], text=True, timeout=2).strip()

def owned_guis():
    result = []
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            if (path / "exe").resolve() == gui and ("XDG_RUNTIME_DIR=" + env["XDG_RUNTIME_DIR"]).encode() in (path / "environ").read_bytes().split(b"\0"):
                result.append(int(path.name))
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            pass
    return result

def identity(path):
    stat = path.lstat()
    return stat.st_dev, stat.st_ino, stat.st_uid, stat.st_mode, stat.st_nlink

def interrupted(number, _frame):
    raise SystemExit(128 + number)


signal.signal(signal.SIGTERM, interrupted)


try:
    assert not owned_guis() and not runtime.exists()
    decoy = subprocess.Popen(["/usr/bin/xmessage", "-title", "PulseTerm cold focus decoy", "isolated observer"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = time.monotonic() + 3
    while True:
        try:
            assert decoy.poll() is None, "cold.decoy_exited"
            decoy_ids = xdo("search", "--onlyvisible", "--name", "^PulseTerm cold focus decoy$").splitlines()
            assert len(decoy_ids) == 1, "cold.decoy_not_unique"
            decoy_xid = decoy_ids[0]
            break
        except (subprocess.CalledProcessError, IndexError):
            assert time.monotonic() < deadline, "cold.decoy_timeout"
            time.sleep(0.025)
    xdo("windowactivate", "--sync", decoy_xid)
    if cold_action == "cwd":
        import runpy
        runpy.run_path("scripts/probe-workspace-cold-new-tab.py")["verify_directory"](pulse, root, env, runtime, evidence, owned_guis, xdo)
        print("workspace.cli_current_directory_consent_no_fallback_cold_warm_native_exit_confirmed")
        sys.exit(0)
    if cold_action == "multiwindow":
        import runpy
        runpy.run_path("scripts/probe-workspace-multiwindow.py")["verify"](pulse, root, env, runtime, evidence, owned_guis, xdo, identity)
        print("workspace.cli_multiwindow_same_process_exact_routes_independent_input_refresh_close_exit_confirmed")
        sys.exit(0)
    if cold_action == "chain-syntax":
        for arguments in [["new-tab", "sh", ".", "split", "-v", "/bin/sh"], ["focus", ";"], ["focus", ".", ";", "profiles"], ["profiles"] + [word for _ in range(8) for word in [".", "focus"]]]:
            result = subprocess.run([str(pulse), *arguments], env=env, capture_output=True, text=True, timeout=3)
            assert result.returncode == 2 and not result.stdout
            assert not owned_guis() and not runtime.exists() and not (root / "unexpected-pty").exists()
        (evidence / f"{evidence_prefix}-result.json").write_text(json.dumps(dict(syntax_rejected=4, no_gui=True, no_endpoint=True, no_pty=True)) + "\n")
        print("workspace.cli_chain_entire_syntax_rejected_before_io_confirmed")
        sys.exit(0)
    if cold_action.startswith(("split-", "chain-")):
        import runpy
        controller = runpy.run_path("scripts/probe-workspace-cold-split.py")
        if default_split == "saved":
            controller["prepare_default"](pulse, root, env, runtime, evidence, owned_guis, xdo)
        _, direction, scenario = cold_action.split("-")
        arguments = ["new-tab", "sh", ".", "split", "-v" if direction == "row" else "-h", "sh", ";", "profiles"] if cold_action.startswith("chain-") else ["split", "-v" if direction == "row" else "-h", "sh"]
        if default_split is not None:
            arguments.pop(5 if cold_action.startswith("chain-") else -1)  # Resolve the saved default at the product boundary.
        client = subprocess.Popen([str(pulse), *arguments], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            if default_split == "unset":
                controller["verify_missing_default"](client, root, runtime, evidence, owned_guis, xdo)
            else:
                controller["verify"](cold_action, client, pulse, root, env, runtime, evidence, owned_guis, xdo, identity)
        except Exception:
            for owner in owned_guis():
                try:
                    windows = [value for value in xdo("search", "--onlyvisible", "--pid", owner).splitlines() if xdo("getwindowname", value) == "PulseTerm · 工作区预览"]
                    if windows:
                        xid = windows[0]
                        subprocess.run(["/usr/bin/import", "-window", xid, str(evidence / f"{evidence_prefix}-failure.png")], timeout=3, check=False, capture_output=True)
                except (OSError, subprocess.SubprocessError):
                    pass
            if client.poll() is not None:
                output, error = client.communicate(timeout=1)
                (evidence / f"{evidence_prefix}-client.log").write_text(output + error)
            else:
                (evidence / f"{evidence_prefix}-client.log").write_text("client still waiting at failure observation\n")
            raise
        print("workspace.cli_default_unset_no_fallback_no_pty_native_exit_confirmed" if default_split == "unset" else f"workspace.cli_cold_{cold_action}_two_explicit_consents_bound_target_independent_pty_refresh_native_exit_confirmed")
        sys.exit(0)
    if cold_action.startswith("sensitive-"):
        import runpy
        controller = runpy.run_path("scripts/probe-workspace-cold-sensitive.py")
        controller["verify"](cold_action, pulse, root, env, runtime, evidence, owned_guis, xdo, identity)
        print(f"workspace.cli_cold_{cold_action}_two_clients_one_primary_original_consent_and_native_exit_confirmed")
        sys.exit(0)
    if cold_action.startswith("concurrent-"):
        import runpy
        controller = runpy.run_path("scripts/probe-workspace-cold-concurrent.py")
        controller["verify"](cold_action, pulse, root, env, runtime, evidence, owned_guis, xdo, identity, profiles_receipt)
        print(f"workspace.cli_cold_{cold_action}_two_clients_one_primary_zero_pty_actual_exit_confirmed")
        sys.exit(0)
    if cold_action.startswith(("new-tab-", "open-")):
        from importlib.machinery import SourceFileLoader
        controller = SourceFileLoader("cold_new_tab", "scripts/probe-workspace-cold-new-tab.py").load_module()
        client = subprocess.Popen([str(pulse), ("open" if cold_action.startswith("open-") else "new-tab"), "sh"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        controller.verify(cold_action, client, pulse, root, env, runtime, evidence, owned_guis, xdo, identity)
        print(f"workspace.cli_cold_{cold_action}_original_page_consent_and_native_exit_confirmed")
        sys.exit(0)
    # Sole launch stimulus is an independent production CLI process.
    client = subprocess.Popen([str(pulse), cold_action], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    output, error = client.communicate(timeout=25)
    (evidence / f"{evidence_prefix}-client.log").write_text(output + error)
    if client.returncode != 0:
        observation = subprocess.run([str(pulse), "profiles"], env=env, capture_output=True, text=True, timeout=8)
        (evidence / f"{evidence_prefix}-page-read.log").write_text(f"read_only_exit={observation.returncode}\n" + observation.stdout + observation.stderr)
        for owner in owned_guis():
            for window in xdo("search", "--onlyvisible", "--pid", owner).splitlines():
                subprocess.run(["/usr/bin/import", "-window", window, str(evidence / f"{evidence_prefix}-failure.png")], timeout=3, capture_output=True, check=False)
    assert client.returncode == 0, "cold.cli_unconfirmed"
    if cold_action == "focus":
        assert output == "focus requested\n"
    else:
        profiles_receipt(output)

    owners = owned_guis()
    assert len(owners) == 1, "cold.not_one_primary"
    app = owners[0]
    ids = xdo("search", "--onlyvisible", "--pid", app).splitlines()
    ids = [value for value in ids if xdo("getwindowname", value) == "PulseTerm · 工作区预览"]
    assert len(ids) == 1, "cold.not_one_workspace_window"
    xid = ids[0]
    if cold_action == "focus":
        assert xdo("getactivewindow") == xid
    assert xdo("getwindowpid", xid) == str(app)
    assert not (root / "unexpected-pty").exists(), "cold.focus_started_shell"
    socket = runtime / "control.sock"
    token = runtime / "control.token"
    socket_id, token_id, token_bytes = identity(socket), identity(token), token.read_bytes()
    if os.environ.get("PULSE_CLI_CAPACITY_PROBE") == "1":
        import runpy
        runpy.run_path("scripts/probe-workspace-cli-capacity.py")["verify"](pulse, env, runtime, evidence)
        assert owned_guis() == [app] and not (root / "unexpected-pty").exists()
        print("workspace.cli_eight_authenticated_slots_ninth_rejected_one_slot_reclaimed_no_new_gui_or_pty_confirmed")
    for turn in range(3):
        xdo("windowactivate", "--sync", decoy_xid)
        assert xdo("getactivewindow") == decoy_xid
        result = subprocess.run([str(pulse), "focus"], env=env, capture_output=True, text=True, timeout=8)
        assert result.returncode == 0 and result.stdout == "focus requested\n"
        assert xdo("getactivewindow") == xid
        assert owned_guis() == [app]
        assert identity(socket) == socket_id and identity(token) == token_id and token.read_bytes() == token_bytes
        assert not (root / "unexpected-pty").exists()
    discovery = subprocess.run([str(pulse), "profiles"], env=env, capture_output=True, text=True, timeout=8)
    (evidence / f"{evidence_prefix}-discovery.log").write_text(f"read_only_exit={discovery.returncode}\n" + discovery.stdout + discovery.stderr)
    directories = [f"{item.relative_to(root)} mode={item.stat().st_mode & 0o7777:o}" for item in (root / "data").rglob("*") if item.is_dir() and not item.is_symlink()]
    (evidence / f"{evidence_prefix}-directory-modes.log").write_text("\n".join(directories) + "\n")
    assert discovery.returncode == 0, "cold.workspace_discovery_unavailable"
    profiles_receipt(discovery.stdout)
    assert not (root / "unexpected-pty").exists()
    subprocess.run(["/usr/bin/import", "-window", xid, str(evidence / f"{evidence_prefix}-rendered.png")], check=True, timeout=3)
    xdo("windowclose", xid)
    deadline = time.monotonic() + 10
    while True:
        waited, status = os.waitpid(app, os.WNOHANG)
        if waited:
            assert os.waitstatus_to_exitcode(status) == 0, "cold.gui_bad_exit"
            break
        assert time.monotonic() < deadline, "cold.gui_exit_timeout"
        time.sleep(0.025)
    assert not socket.exists() and not token.exists(), "cold.endpoints_not_drained"
    (evidence / f"{evidence_prefix}-result.txt").write_text(f"primary_pid={app} xid={xid} cold_process=1 warm_processes=3 shell_starts=0 endpoints_preserved=true actual_gui_exit=true\n")
    app = None
    print(f"workspace.cli_cold_{cold_action}_original_document_one_gui_three_warm_focus_no_pty_and_actual_exit_confirmed")
finally:
    if xid and owned_guis():
        try:
            subprocess.run(["/usr/bin/import", "-window", xid, str(evidence / f"{evidence_prefix}-failure.png")], timeout=3, check=False, capture_output=True)
        except (OSError, subprocess.TimeoutExpired):
            pass
    if client and client.poll() is None:
        client.kill()
        client.wait(timeout=3)
    # Only this unique runtime's exact GUI binary; never the developer's GUI.
    for pid in owned_guis():
        try:
            os.kill(pid, signal.SIGTERM)
            deadline = time.monotonic() + 3
            while not os.waitpid(pid, os.WNOHANG)[0]:
                if time.monotonic() >= deadline:
                    os.kill(pid, signal.SIGKILL)
                    os.waitpid(pid, 0)
                    break
                time.sleep(0.025)
        except (ProcessLookupError, ChildProcessError):
            pass
    if decoy:
        decoy.terminate()
        decoy.wait(timeout=3)
    # Reap adopted children before removal; zero WNOHANG is not proof of exit.
    # A unique runtime environment identifies only this task's descendants.
    deadline = time.monotonic() + 5
    while True:
        owned_children = []
        for proc in Path("/proc").iterdir():
            if not proc.name.isdigit() or int(proc.name) == os.getpid():
                continue
            try:
                if ("XDG_RUNTIME_DIR=" + env["XDG_RUNTIME_DIR"]).encode() in (proc / "environ").read_bytes().split(b"\0"):
                    owned_children.append(int(proc.name))
            except (FileNotFoundError, ProcessLookupError, PermissionError):
                pass
        for pid in owned_children:
            try:
                os.kill(pid, signal.SIGTERM if time.monotonic() < deadline else signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            waited, _ = os.waitpid(-1, os.WNOHANG)
            if waited:
                continue
            assert time.monotonic() < deadline + 3, "cold.owned_child_cleanup_timeout"
            time.sleep(0.025)
        except ChildProcessError:
            assert not owned_children, "cold.owned_children_not_reaped"
            break
    assert root.parent == Path(os.environ["TMPDIR"]) and root.name.startswith("c.")
    shutil.rmtree(root)
