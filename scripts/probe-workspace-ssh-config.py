"""Actual WebView config metadata preview; no terminal, credential or network startup."""
from PIL import Image
import csv
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time

root = Path(os.environ["PULSE_CONFIG_PROBE_ROOT"]).resolve()
assert root.parent == Path("/data/temp") and root.name.startswith("workspace-ssh-config.")
assert root.stat().st_mode & 0o777 == 0o700
source = root / "ssh"
source.mkdir(mode=0o700)
(source / "parts").mkdir(mode=0o700)
def write(path, text):
    path.write_text(text)
    path.chmod(0o600)
write(source / "config", "Host prod\n HostName preview.example\n Include parts/*.conf\n")
write(source / "parts" / "10.conf", "User fixtureuser\n Port 2222\n IdentityFile never-read-key\n")
os.mkfifo(source / "never-read-key", 0o600)
evidence = Path("target/probes")
probe_mode = os.environ.get("PULSE_CONFIG_PROBE_MODE", "preview")
assert probe_mode in ("preview", "import", "status")
artifact_prefix = "workspace-ssh-config-" + (probe_mode + "-" if probe_mode != "preview" else "")
app = stream = log = xid = None

def command(args, timeout=3):
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=timeout).stdout

def keys(*values):
    command(["xdotool", "key", "--clearmodifiers", *values])

def text(value):
    command(["xdotool", "type", "--clearmodifiers", "--delay", "1", value])

def count(stage):
    return log.read_text().splitlines().count("workspace." + stage)

def until(predicate, stage):
    deadline = time.monotonic() + 8
    while not predicate():
        assert app.poll() is None, "config-window.native_early_exit." + stage
        assert time.monotonic() < deadline, "config-window.timeout." + stage
        time.sleep(0.1)


def visible_status(name, expected):
    observed = ""
    def shown():
        nonlocal observed
        result = subprocess.run([os.environ["PULSE_CONFIG_STATUS_OBSERVER"], str(app.pid), expected], capture_output=True, text=True, timeout=2)
        observed = result.stderr
        assert result.returncode in (0, 1), "config-window.accessibility_observer_failed"
        return result.returncode == 0
    try:
        until(shown, "visible-status." + name)
    except AssertionError:
        shot(name + "-timeout")
        (evidence / (artifact_prefix + name + "-observer.txt")).write_text(observed)
        raise
    shot(name)
def shot(name):
    path = evidence / (artifact_prefix + name + ".png")
    command(["import", "-window", xid, str(path)])
    return path

def ocr(name, tsv=False):
    image = shot(name)
    doubled = image.with_name(image.stem + "-2x.png")
    command(["convert", str(image), "-resize", "200%", str(doubled)])
    prefix = image.with_suffix("")
    command(["tesseract", str(doubled), str(prefix), "--psm", "11", *(["tsv"] if tsv else [])])
    return prefix.with_suffix(".tsv" if tsv else ".txt")

def drawn(name, *matches):
    until(lambda: all(word in ocr(name).read_text() for word in matches), "drawn." + name)

def click_word(name, target):
    if target == "ssh_config":
        def activate():
            return subprocess.run([os.environ["PULSE_CONFIG_STATUS_OBSERVER"], str(app.pid), "预览 ssh_config", "activate"], capture_output=True, timeout=2).returncode == 0
        until(activate, "button_not_drawn." + target)
        return
    found = None
    offset_x = offset_y = 0
    deadline = time.monotonic() + 8
    while found is None:
        if target == "ssh_config":
            image = shot(name)
            footer = image.with_name(image.stem + "-footer.png")
            with Image.open(image) as im:
                width, height = im.size
                offset_x, offset_y = width // 2, height * 3 // 4
                region = im.crop((offset_x, offset_y, width, height))
                region.resize((region.width * 2, region.height * 2)).save(footer)
            prefix = footer.with_suffix("")
            command(["tesseract", str(footer), str(prefix), "--psm", "6", "tsv"])
            observed = prefix.with_suffix(".tsv")
        else:
            observed = ocr(name, True)
        with observed.open() as f:
            candidates = [row for row in csv.DictReader(f, delimiter="\t") if target in row["text"]]
        if candidates:
            found = candidates[0]
            break
        assert time.monotonic() < deadline, "config-window.button_not_drawn." + target
        time.sleep(0.1)
    x = offset_x + (int(found["left"]) + int(found["width"]) // 2) // 2
    y = offset_y + (int(found["top"]) + int(found["height"]) // 2) // 2
    command(["xdotool", "mousemove", "--window", xid, str(x), str(y), "click", "1"])

def open_preview():
    # First SSH sidebar entry is hosts; second is identities.
    deadline = time.monotonic() + 8
    while True:
        with ocr("sidebar", True).open() as f:
            candidates = [row for row in csv.DictReader(f, delimiter="\t") if row["text"].startswith("SSH") and int(row["left"]) < 170 and int(row["top"]) > 300]
        if len(candidates) == 2:
            first = sorted(candidates, key=lambda row: int(row["top"]))[0]
            command(["xdotool", "mousemove", "--window", xid, "150", str((int(first["top"]) + int(first["height"]) // 2) // 2), "click", "1"])
            break
        assert time.monotonic() < deadline, "config-window.sidebar_not_drawn"
        time.sleep(0.1)
    click_word("preview-entry", "ssh_config")
    until(lambda: subprocess.run([os.environ["PULSE_CONFIG_STATUS_OBSERVER"], str(app.pid), "Host 别名", "focus"], capture_output=True, timeout=2).returncode == 0, "host_alias_focus")
    text("prod")
    keys("Return")

def start(round):
    global app, stream, log, xid
    log = evidence / (artifact_prefix + "native-" + str(round) + ".log")
    stream = log.open("w")
    app = subprocess.Popen(["target/debug/pulseterm-workspace"], stdout=stream, stderr=subprocess.STDOUT)
    try:
        until(lambda: count("profiles_listed") == 1, "profiles")
    except AssertionError:
        observed = command(["xdotool", "search", "--onlyvisible", "--all", "--pid", str(app.pid), "--name", "^PulseTerm · 工作区预览$"]).splitlines()
        if len(observed) == 1:
            xid = observed[0]
            shot("profiles-timeout")
        raise
    ids = command(["xdotool", "search", "--onlyvisible", "--all", "--pid", str(app.pid), "--name", "^PulseTerm · 工作区预览$"]).splitlines()
    assert len(ids) == 1 and command(["xdotool", "getwindowpid", ids[0]]).strip() == str(app.pid)
    xid = ids[0]
    command(["xdotool", "windowactivate", "--sync", xid], 2)

def counts():
    database = Path(os.environ["XDG_DATA_HOME"]) / "dev.pulseterm.workspace-preview" / "state.db"
    with sqlite3.connect("file:" + str(database) + "?mode=ro", uri=True, timeout=0.25) as db:
        tables = sorted(row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"))
        assert {"app_state", "known_hosts", "identities", "key_identities"}.issubset(tables), "config-window.required_tables_missing"
        snapshot = {}
        for name in tables:
            quoted = name.replace('"', '""')
            rows = sorted(db.execute('SELECT * FROM "' + quoted + '"').fetchall(), key=repr)
            snapshot[name] = {"rows": len(rows), "sha256": hashlib.sha256(repr(rows).encode()).hexdigest()}
        return snapshot

def stop():
    global app, stream
    assert count("session_opened") == 0
    assert not any("workspace.ssh_" in line for line in log.read_text().splitlines())
    keys("alt+F4")
    until(lambda: count("exited") == 1, "exit")
    assert app.wait(timeout=3) == 0
    app = None
    stream.close()
    stream = None

def preview_case():
    start(1)
    before = counts()
    open_preview()
    drawn("initial", "preview.example", "2222", "fixtureuser", "IdentityFile")
    # alias -> read -> label -> prepare -> source confirmation
    keys("Tab", "Tab", "Tab", "Tab", "Return")
    until(lambda: count("config_confirmed") == 1, "source-confirmed")
    visible_status("source-confirmed", "本次预览的源文件未变。没有保存主机或建立连接。")
    keys("Escape")
    open_preview()
    until(lambda: count("config_previewed") == 2, "second-preview")
    drawn("before-source-change", "fixtureuser")
    write(source / "parts" / "00.conf", "User updateduser\n Port 2223\n")
    keys("Tab", "Tab", "Tab", "Tab", "Return")
    until(lambda: count("config_confirm_denied") == 1, "source-conflict")
    visible_status("source-conflict", "配置已变化或核对未确认，请重新预览。没有保存主机或建立连接。")
    keys("Escape")
    open_preview()
    drawn("updated", "preview.example", "2223", "updateduser")
    keys("Escape", "ctrl+shift+r")
    until(lambda: count("document_confirmed") == 2 and count("profiles_listed") == 2, "refresh")
    open_preview()
    drawn("refreshed", "2223", "updateduser")
    keys("Escape")
    assert counts() == before
    stop()
    start(2)
    open_preview()
    drawn("reopened", "2223", "updateduser")
    keys("Escape")
    assert counts() == before
    stop()
    (evidence / (artifact_prefix + "result.json")).write_text(json.dumps({"app_processes":2,"actual_preview_include_change_refresh_reopen":True,"unchanged_storage_snapshot":before,"terminal_starts":0,"actual_exit":True}) + "\n")
    print("workspace.ssh_config_actual_preview_include_change_refresh_reopen_no_store_no_session_exit_confirmed")

def saved_host():
    database = Path(os.environ["XDG_DATA_HOME"]) / "dev.pulseterm.workspace-preview" / "state.db"
    with sqlite3.connect("file:" + str(database) + "?mode=ro", uri=True, timeout=0.25) as db:
        row = db.execute("SELECT value FROM app_state WHERE key='ssh.endpoints.v1'").fetchone()
        assert row is not None
        document = json.loads(row[0])
        assert document["schemaVersion"] == 1 and document["revision"] == 1 and len(document["hosts"]) == 1
        record = document["hosts"][0]
        assert record["label"] == "Imported prod" and record["host"] == "preview.example" and record["port"] == 2223 and record["username"] == "updateduser"
        assert set(record) == {"id", "label", "host", "port", "username"}
        assert record["id"][14] == "7"
        return record

def prepare(name, number):
    # alias -> read -> label; its input clears the old preview and requires a new read.
    keys("Tab", "Tab", "ctrl+a")
    text(name)
    keys("shift+Tab", "Return")
    until(lambda: count("config_previewed") >= number * 2, "prepare-fresh-read")
    keys("Tab", "Tab", "Return")
    until(lambda: count("config_import_prepared") == number, "prepared")
    visible_status("prepared-" + str(number), "已准备主机“" + name + "”。保存后可在 SSH 主机中查看，不会建立连接。")

def import_case():
    start(1)
    before = counts()
    open_preview()
    drawn("initial", "2222", "fixtureuser")
    prepare("Imported prod", 1)
    keys("Escape")
    assert counts() == before
    open_preview()
    drawn("before-conflict", "2222", "fixtureuser")
    prepare("Imported prod", 2)
    write(source / "parts" / "00.conf", "User updateduser\n Port 2223\n")
    keys("Return")
    until(lambda: count("config_import_denied") == 1, "source-save-conflict")
    visible_status("source-conflict", "保存未确认或来源已变化，请先查看主机目录，再重新预览。不会自动重试。")
    assert counts() == before
    keys("Escape")
    open_preview()
    drawn("fresh-target", "2223", "updateduser")
    prepare("Imported prod", 3)
    keys("Return")
    until(lambda: count("config_import_saved") == 1, "saved")
    visible_status("saved", "已保存主机“Imported prod”。可在 SSH 主机中查看。")
    record = saved_host()
    after = counts()
    assert {k:v for k,v in before.items() if k != "app_state"} == {k:v for k,v in after.items() if k != "app_state"}
    assert after["app_state"]["rows"] == before["app_state"]["rows"] + 1
    keys("Escape", "Escape", "ctrl+shift+r")
    until(lambda: count("document_confirmed") == 2 and count("profiles_listed") == 2, "saved-refresh")
    assert saved_host() == record
    open_preview()
    drawn("refreshed", "2223", "updateduser")
    keys("Escape", "Escape")
    stop()
    start(2)
    assert saved_host() == record and counts() == after
    open_preview()
    drawn("reopened", "2223", "updateduser")
    keys("Escape", "Escape")
    stop()
    (evidence / (artifact_prefix + "result.json")).write_text(json.dumps({"app_processes":2,"cancel_and_source_conflict_zero_write":True,"saved_host":record,"storage_before":before,"storage_after":after,"same_id_refresh_and_reopen":True,"terminal_starts":0,"actual_exit":True}) + "\n")
    print("workspace.ssh_config_actual_separate_approval_source_conflict_saved_host_refresh_reopen_no_credentials_no_session_exit_confirmed")

def source_status_case():
    start(1)
    before = counts()
    open_preview()
    drawn("initial", "2222", "fixtureuser")
    until(lambda: count("config_source_unchanged") >= 1, "unchanged-status")
    write(source / "parts" / "00.conf", "User monitoreduser\n Port 2223\n")
    until(lambda: count("config_source_changed") == 1, "automatic-source-change")
    visible_status("source-changed", "配置源已变化，请重新预览。旧值保留供核对，不能保存。")
    drawn("last-good", "2222", "fixtureuser")
    keys("Escape")
    open_preview()
    drawn("updated", "2223", "monitoreduser")
    until(lambda: count("config_source_unchanged") >= 2, "new-source-status")
    command([os.environ["PULSE_CONFIG_STATUS_OBSERVER"], str(app.pid), "核对源文件", "activate"])
    until(lambda: count("config_confirmed") == 1, "status-did-not-consume-ticket")
    visible_status("explicit-confirmation", "本次预览的源文件未变。没有保存主机或建立连接。")
    keys("Escape", "ctrl+shift+r")
    until(lambda: count("document_confirmed") == 2 and count("profiles_listed") == 2, "refresh")
    open_preview()
    drawn("refreshed", "2223", "monitoreduser")
    keys("Escape")
    assert counts() == before
    stop()
    start(2)
    open_preview()
    drawn("reopened", "2223", "monitoreduser")
    keys("Escape")
    assert counts() == before
    stop()
    (evidence / (artifact_prefix + "result.json")).write_text(json.dumps({"app_processes": 2, "automatic_source_change": True, "preserved_last_good_display": True, "status_keeps_explicit_confirmation_ticket": True, "unchanged_storage_snapshot": before, "terminal_starts": 0, "actual_exit": True}) + "\n")
    print("workspace.ssh_config_actual_source_monitor_change_preserved_display_original_ticket_refresh_reopen_no_store_no_session_exit_confirmed")

try:
    ({"preview": preview_case, "import": import_case, "status": source_status_case}[probe_mode])()
finally:
    if app is not None and app.poll() is None:
        app.terminate()
        try: app.wait(timeout=3)
        except subprocess.TimeoutExpired:
            app.kill()
            app.wait(timeout=3)
    if stream is not None: stream.close()
