"""Real-window encrypted key ingress; never capture or report a populated private input."""
import csv
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import time

root = Path(os.environ["PULSE_KEY_IMPORT_FIXTURE_ROOT"]).resolve()
assert root.parent == Path("/data/temp") and root.name.startswith("workspace-key-import.")
assert root.stat().st_mode & 0o777 == 0o700
output = Path("target/probes")
app = None
stream = None
log = None
xid = None
secret_input = False
fixture_test = "product_workspace::profiles::key_write_service::tests::private_key_window_fixture"


def terminate(_number, _frame):
    raise KeyboardInterrupt


signal.signal(signal.SIGTERM, terminate)


def command(args, timeout=3):
    # Do not include command arguments in failures (a fixture passphrase may be an argument).
    result = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    assert result.returncode == 0, "key-import.controller_failed"
    return result.stdout


def keys(*args):
    command(["xdotool", "key", "--clearmodifiers", *args])


def type_text(value):
    command(["xdotool", "type", "--clearmodifiers", "--delay", "1", value])


def count(stage):
    return log.read_text().splitlines().count("workspace." + stage)


def until(predicate, stage):
    end = time.monotonic() + 8
    while not predicate():
        assert app.poll() is None, "key-import.native_early_exit." + stage
        assert time.monotonic() < end, "key-import.timeout." + stage
        time.sleep(0.1)


def shot(stage, tsv=False):
    assert not secret_input, "key-import.no_secret_capture"
    path = output / ("workspace-key-import-" + stage + ".png")
    command(["import", "-window", xid, str(path)])
    args = ["tesseract", str(path), str(path.with_suffix("")), "--psm", "11"]
    if tsv:
        args.append("tsv")
    command(args)
    return path.with_suffix(".tsv" if tsv else ".txt")


def drawn(stage, *values):
    until(lambda: all(value in shot(stage).read_text() for value in values), "drawn." + stage)


def sidebar(index):
    words = None
    end = time.monotonic() + 8
    while words is None:
        with shot("sidebar", True).open() as file:
            matches = [r for r in csv.DictReader(file, delimiter="\t") if r["text"].startswith("SSH") and int(r["left"]) < 85 and int(r["top"]) > 150]
        if len(matches) == 2:
            words = sorted(matches, key=lambda r: int(r["top"]))
        else:
            assert time.monotonic() < end, "key-import.sidebar_not_drawn"
    y = int(words[1]["top"]) + int(words[1]["height"]) // 2 + (52 if index == "vault" else 0)
    command(["xdotool", "mousemove", "--window", xid, "150", str(y), "click", "1"])
    time.sleep(0.3)


def fixture(mode):
    env = os.environ.copy()
    env["PULSE_KEY_IMPORT_FIXTURE_MODE"] = mode
    result = subprocess.run(["mise", "exec", "--", "cargo", "test", "--offline", "-p", "pulseterm-desktop", "--features", "workspace-cli,workspace-ssh", "--lib", "--locked", fixture_test, "--", "--ignored", "--exact", "--nocapture"], env=env, capture_output=True, text=True, timeout=20)
    (output / ("workspace-key-import-fixture-" + mode + ".log")).write_text(result.stdout + result.stderr)
    assert result.returncode == 0 and "1 passed" in result.stdout, "key-import.fixture." + mode


def snapshot():
    database = root / "data/dev.pulseterm.workspace-preview/state.db"
    assert database.is_file() and not database.is_symlink()
    with sqlite3.connect("file:" + str(database) + "?mode=ro", uri=True, timeout=0.25) as db:
        assert db.execute("PRAGMA user_version").fetchone()[0] == 5
        identity = db.execute("SELECT id,created_at,hlc,deleted,label,username,vault_id,secret_id,kind,format,passphrase_id FROM key_identities").fetchall()
        records = db.execute("SELECT id,kind,length(ciphertext) FROM secrets ORDER BY id").fetchall()
        revision = db.execute("SELECT revision FROM vault_envelopes WHERE slot=1").fetchone()[0]
    return identity, records, revision


def start(round):
    global app, stream, log, xid
    log = output / ("workspace-key-import-native-" + str(round) + ".log")
    stream = log.open("w")
    app = subprocess.Popen(["target/debug/pulseterm-workspace"], stdout=stream, stderr=subprocess.STDOUT)
    until(lambda: count("profiles_listed") == 1, "profiles")
    matches = command(["xdotool", "search", "--onlyvisible", "--all", "--pid", str(app.pid), "--name", "^PulseTerm · 工作区预览$"]).splitlines()
    assert len(matches) == 1 and command(["xdotool", "getwindowpid", matches[0]]).strip() == str(app.pid)
    xid = matches[0]
    command(["xdotool", "windowactivate", "--sync", xid], 2)


def stop():
    global app, stream
    assert count("session_opened") == 0
    keys("alt+F4")
    until(lambda: count("exited") == 1, "exit")
    code = app.wait(timeout=3)
    assert code == 0, "key-import.native_exit_code." + str(code)
    app = None
    stream.close()
    stream = None


def submit(passphrase):
    global secret_input
    # A selected identity orders inputs as metadata, key, passphrase, Save key, Done.
    keys("shift+Tab", "shift+Tab", "ctrl+a")
    secret_input = True
    clipboard_log = root / "clipboard.log"
    with clipboard_log.open("w") as clip_log:
        clip = subprocess.Popen([str(root / "clipboard"), str(root / "fixed-key.pem")], stdout=clip_log, stderr=subprocess.STDOUT)
        try:
            until(lambda: "clipboard.ready" in clipboard_log.read_text(), "clipboard_ready")
            keys("ctrl+v")
            until(lambda: "clipboard.served" in clipboard_log.read_text(), "clipboard_served")
            time.sleep(0.1)
        finally:
            clip.terminate()
            try:
                clip.wait(timeout=3)
            except subprocess.TimeoutExpired:
                clip.kill()
                clip.wait(timeout=3)
    keys("Tab", "ctrl+a")
    type_text(passphrase)
    keys("Tab", "Return")
    # Submission clears both fields synchronously. No captures until its result is observed.


try:
    fixture("seed")
    empty = snapshot()
    assert len(empty[0]) == 1 and empty[1] == []
    start(1)
    sidebar("identity")
    drawn("locked-empty", "fixed-native-key")
    submit("fixed-native-private-key-passphrase")
    # Rejected saves remove the form and offer metadata reload, with Done focused.
    until(lambda: count("identity_key_prepare_rejected") == 1, "locked_rejected")
    keys("Escape")
    secret_input = False
    assert snapshot() == empty
    shot("locked-rejected-cleared")
    sidebar("vault")
    until(lambda: count("vault_state_locked") == 1, "vault_locked")
    keys("shift+Tab", "shift+Tab", "shift+Tab", "ctrl+a")
    type_text("fixed-native-private-key-master")
    keys("Tab", "Return")
    until(lambda: count("vault_front_confirmed") == 1, "vault_confirmed")
    keys("Escape")
    sidebar("identity")
    drawn("unlocked-empty", "fixed-native-key")
    submit("wrong-private-key-passphrase")
    until(lambda: count("identity_key_submit_rejected") == 1, "wrong_passphrase_rejected")
    keys("Escape")
    secret_input = False
    assert snapshot() == empty
    shot("wrong-passphrase-cleared")
    sidebar("identity")
    drawn("before-save", "fixed-native-key")
    submit("fixed-native-private-key-passphrase")
    until(lambda: count("identity_key_saved") == 1 and len(snapshot()[1]) == 2, "ciphertext_commit")
    saved = snapshot()
    assert saved[0] == empty[0] and saved[2] == empty[2] + 1
    assert {r[1] for r in saved[1]} == {"ssh_private_key", "ssh_key_passphrase"}
    assert {r[0] for r in saved[1]} == {saved[0][0][7], saved[0][0][10]}
    time.sleep(0.3)
    secret_input = False
    shot("saved-cleared")
    keys("Escape", "ctrl+shift+r")
    until(lambda: count("document_confirmed") == 2 and count("profiles_listed") == 2, "refresh")
    assert snapshot() == saved
    sidebar("identity")
    drawn("refreshed", "fixed-native-key")
    keys("Escape")
    stop()
    fixture("verify")
    start(2)
    assert snapshot() == saved
    sidebar("identity")
    drawn("reopened", "fixed-native-key")
    keys("Escape")
    sidebar("vault")
    until(lambda: count("vault_state_locked") == 1, "reopened_locked")
    shot("reopened-locked")
    keys("Escape")
    stop()
    (output / "workspace-key-import-result.json").write_text(json.dumps({"app_processes":2,"locked_rejected":True,"wrong_passphrase_rejected":True,"single_atomic_ciphertext_revision":True,"same_identity_refs":True,"refresh_reopen_ciphertext":True,"independent_rust_exact_bytes":True,"terminal_starts":0,"actual_native_exit":True}) + "\n")
    print("workspace.key_import_locked_wrong_passphrase_atomic_save_refresh_reopen_exact_bytes_exit_confirmed")
finally:
    if app is not None and app.poll() is None:
        app.terminate()
        try:
            app.wait(timeout=3)
        except subprocess.TimeoutExpired:
            app.kill()
            app.wait(timeout=3)
    if stream is not None:
        stream.close()
