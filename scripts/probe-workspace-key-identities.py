"""Current-page key metadata CRUD in four real Tauri processes; no secret ingress."""
import csv
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import time

root = Path(os.environ["PULSE_KEY_METADATA_PROBE_ROOT"]).resolve()
assert root.parent == Path("/data/temp") and root.name.startswith("workspace-identities.")
assert root.stat().st_mode & 0o777 == 0o700
evidence = Path("target/probes")
directory = Path(os.environ["XDG_DATA_HOME"]) / "dev.pulseterm.workspace-preview"
app = None
log = None
stream = None
xid = None
database = None


def command(args, timeout=3):
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=timeout).stdout


def keys(*values):
    command(["xdotool", "key", "--clearmodifiers", *values])


def type_text(value):
    command(["xdotool", "type", "--clearmodifiers", "--delay", "1", value])


def count(stage):
    return log.read_text().splitlines().count("workspace." + stage)


def until(predicate, stage):
    deadline = time.monotonic() + 8
    while not predicate():
        assert app.poll() is None, "key-metadata.native_early_exit." + stage
        assert time.monotonic() < deadline, "key-metadata.timeout." + stage
        time.sleep(0.1)


def shot(name):
    path = evidence / ("workspace-key-identities-" + name + ".png")
    command(["import", "-window", xid, str(path)])
    return path


def ocr(name, tsv=False):
    image = shot(name)
    prefix = image.with_suffix("")
    args = ["tesseract", str(image), str(prefix), "--psm", "11"]
    if tsv:
        args.append("tsv")
    command(args)
    return prefix.with_suffix(".tsv" if tsv else ".txt")


def drawn(name, *matches):
    def matches_frame():
        text = ocr(name).read_text()
        return all(text.count(value) >= matches.count(value) for value in set(matches))
    until(matches_frame, "drawn." + name)


def start(round):
    global app, log, stream, xid
    log = evidence / ("workspace-key-identities-native-" + str(round) + ".log")
    stream = log.open("w")
    app = subprocess.Popen(["target/debug/pulseterm-workspace"], stdout=stream, stderr=subprocess.STDOUT)
    until(lambda: count("profiles_listed") == 1, "profiles")
    ids = command(["xdotool", "search", "--onlyvisible", "--all", "--pid", str(app.pid), "--name", "^PulseTerm · 工作区预览$"]).splitlines()
    assert len(ids) == 1 and command(["xdotool", "getwindowpid", ids[0]]).strip() == str(app.pid)
    xid = ids[0]
    command(["xdotool", "windowactivate", "--sync", xid], timeout=2)


def open_editor():
    heading = None
    deadline = time.monotonic() + 8
    while heading is None:
        with ocr("sidebar", tsv=True).open() as file:
            words = list(csv.DictReader(file, delimiter="\t"))
        candidates = [word for word in words if word["text"].startswith("SSH") and int(word["left"]) < 85 and int(word["top"]) > 150]
        if len(candidates) == 2:
            heading = sorted(candidates, key=lambda word: int(word["top"]))[1]
            break
        assert time.monotonic() < deadline, "key-metadata.sidebar_not_drawn"
        time.sleep(0.1)
    width = int(command(["xdotool", "getwindowgeometry", "--shell", xid]).split("WIDTH=")[1].splitlines()[0])
    assert width >= 700
    y = int(heading["top"]) + int(heading["height"]) // 2
    command(["xdotool", "mousemove", "--window", xid, "150", str(y), "click", "1"])
    time.sleep(0.3)


def snapshot():
    global database
    if database is None:
        database = directory / "state.db"
        assert database.is_file() and not database.is_symlink(), "key-metadata.native_database"
    with sqlite3.connect("file:" + str(database) + "?mode=ro", uri=True, timeout=0.25) as db:
        version = db.execute("PRAGMA user_version").fetchone()[0]
        assert version == 5
        rows = db.execute("SELECT id,created_at,hlc,deleted,label,username,vault_id,secret_id,kind,format,passphrase_id FROM key_identities ORDER BY id").fetchall()
    return rows


def stop():
    global app, stream
    assert count("session_opened") == 0
    keys("alt+F4")
    until(lambda: count("exited") == 1, "exit")
    assert app.wait(timeout=3) == 0
    app = None
    stream.close()
    stream = None


try:
    start("absent")
    open_editor()
    shot("absent-vault")
    assert snapshot() == []
    keys("Escape")
    stop()
    env = os.environ.copy()
    env["PULSE_IDENTITY_FIXTURE_DIRECTORY"] = str(directory)
    fixture = subprocess.run(["mise", "exec", "--", "cargo", "test", "--offline", "-p", "pulseterm-desktop", "--lib", "--features", "workspace", "--locked", "product_workspace::identities::tests::identity_window_fixture", "--", "--ignored", "--exact", "--nocapture"], env=env, capture_output=True, text=True, timeout=15)
    (evidence / "workspace-key-identities-fixture.log").write_text(fixture.stdout + fixture.stderr)
    assert fixture.returncode == 0 and "1 passed" in fixture.stdout and "workspace.identity_metadata_only_fixture_confirmed" in fixture.stdout
    start(1)
    open_editor()
    keys("shift+Tab", "shift+Tab", "shift+Tab", "shift+Tab", "ctrl+a")
    type_text("KEYONE")
    keys("Tab", "ctrl+a")
    type_text("alice")
    keys("Tab", "Down", "Tab", "Return")
    until(lambda: len(snapshot()) == 1, "create")
    created = snapshot()[0]
    assert created[3:6] == (0, "KEYONE", "alice") and created[8:10] == ("ssh_private_key", 1)
    assert created[10] is not None and created[7] != created[10]
    assert re.fullmatch(r"[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}", created[0])
    drawn("created", "KEYONE", "KEYONE", "alice")
    keys("shift+Tab", "shift+Tab", "shift+Tab", "shift+Tab", "ctrl+a")
    type_text("KEYTWO")
    keys("Tab", "ctrl+a")
    type_text("bob")
    keys("Tab", "Return")
    until(lambda: snapshot()[0][4:6] == ("KEYTWO", "bob"), "update")
    updated = snapshot()[0]
    assert created[:2] == updated[:2] and created[6:] == updated[6:] and created[2] != updated[2]
    drawn("updated", "KEYTWO", "KEYTWO", "bob")
    keys("Escape", "ctrl+shift+r")
    until(lambda: count("document_confirmed") == 2 and count("profiles_listed") == 2, "refresh")
    open_editor()
    drawn("refreshed", "KEYTWO", "bob")
    assert snapshot()[0] == updated
    keys("Escape")
    stop()
    start(2)
    open_editor()
    drawn("reopened", "KEYTWO", "bob")
    assert snapshot()[0] == updated
    keys("shift+Tab", "Return")
    assert snapshot()[0] == updated
    shot("delete-confirmation")
    keys("Return")
    until(lambda: snapshot()[0][3] == 1, "delete")
    deleted = snapshot()[0]
    assert deleted[:2] == updated[:2] and deleted[4:] == updated[4:] and deleted[2] != updated[2]
    keys("Escape")
    stop()
    start(3)
    open_editor()
    shot("deleted-reopened")
    assert "KEYTWO" not in ocr("deleted-reopened").read_text()
    assert snapshot()[0] == deleted
    keys("Escape")
    stop()
    (evidence / "workspace-key-identities-result.json").write_text(json.dumps({"app_processes":4,"schema":5,"gui_create_update_refresh_reopen_two_click_delete":True,"same_backend_refs":True,"retained_id_timestamp_tombstone":True,"terminal_starts":0,"actual_native_exit":True}) + "\n")
    print("workspace.key_identity_metadata_gui_create_update_refresh_reopen_delete_and_exit_confirmed")
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
