#!/usr/bin/env python3
"""Independent-process keymap receipt and reopen validation; no user locations."""
import os
import select
import subprocess
import tempfile
from pathlib import Path

binary = Path("target/debug/examples/keymap_file_probe").resolve()
assert binary.is_file(), "build the locked fixture first"
Path("/data/temp").mkdir(exist_ok=True)
children = []
with tempfile.TemporaryDirectory(prefix="pulseterm-keymap-process-", dir="/data/temp") as directory:
    os.chmod(directory, 0o700)
    def start(mode):
        env = dict(os.environ, TMPDIR=directory, TMP=directory, TEMP=directory)
        child = subprocess.Popen([str(binary), directory, mode], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True)
        children.append(child)
        return child
    def receipt(child, expected):
        assert child.stdout is not None
        assert select.select([child.stdout], [], [], 5)[0], "fixed receipt deadline"
        assert child.stdout.readline(256).strip() == expected, "unexpected fixture receipt"
    def save(child):
        assert child.stdin is not None
        child.stdin.write("save\n")
        child.stdin.flush()
    def exit_ok(child):
        assert child.wait(timeout=5) == 0, "fixture process failed"
    try:
        first = start("first")
        receipt(first, "keymap.fixture.loaded")
        stale = start("stale")
        receipt(stale, "keymap.fixture.loaded")
        save(first)
        receipt(first, "keymap.fixture.committed")
        exit_ok(first)
        save(stale)
        receipt(stale, "keymap.fixture.stale_rejected")
        exit_ok(stale)
        reopened = start("reopen")
        receipt(reopened, "keymap.fixture.reopened")
        exit_ok(reopened)
        assert Path(directory, "keymap.toml").stat().st_mode & 0o7777 == 0o600
        assert all(child.poll() is not None for child in children)
    finally:
        for child in children:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=3)
            for stream in [child.stdin, child.stdout, child.stderr]:
                if stream is not None:
                    stream.close()
assert not Path(directory).exists(), "owned fixture cleanup failed"
print("keymap.three_process_snapshot_conflict_durable_reopen_and_cleanup_confirmed")
