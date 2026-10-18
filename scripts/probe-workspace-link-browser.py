#!/usr/bin/env python3
"""Private XDG/GIO browser fixture. Never changes the user's browser registration."""
import http.server
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

TITLE = "PulseTerm link handoff probe"
PAGE = b"<!doctype html><meta charset=utf-8><title>PulseTerm link handoff probe</title><h1>Browser handoff confirmed</h1><p>Local fixture page loaded through the system URI handler.</p>"


def publish(path, value):
    temporary = path.with_name(path.name + ".pending")
    temporary.write_text(json.dumps(value))
    temporary.replace(path)


def serve(fixture):
    browser = next((shutil.which(name) for name in ("google-chrome", "chromium", "chromium-browser") if shutil.which(name)), None)
    assert browser, "browser_fixture.chrome_required"
    artifacts = Path.cwd() / "target/probes"

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == "/page?handoff=1":
                publish(fixture / "browser-request.json", {"path": self.path, "userAgent": self.headers.get("User-Agent", "")})
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(PAGE)))
                self.end_headers()
                self.wfile.write(PAGE)
            else:
                self.send_response(404)
                self.end_headers()

        def log_message(self, format: str, *args: object) -> None:
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    server.timeout = .1
    try:
        target = f"http://127.0.0.1:{server.server_port}/page?handoff=1"
        applications = Path(os.environ["XDG_DATA_HOME"]) / "applications"
        applications.mkdir()
        script = fixture / "link-browser.py"
        (applications / "pulseterm-link-probe.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=PulseTerm isolated browser\nTerminal=false\nNoDisplay=true\n"
            f"Exec=/usr/bin/python3 {script} launch {fixture} %u\n"
            "MimeType=x-scheme-handler/http;x-scheme-handler/https;\n")
        (Path(os.environ["XDG_CONFIG_HOME"]) / "mimeapps.list").write_text(
            "[Default Applications]\nx-scheme-handler/http=pulseterm-link-probe.desktop\n"
            "x-scheme-handler/https=pulseterm-link-probe.desktop\n")
        publish(fixture / "browser-config.json", {"url": target, "executable": browser, "artifacts": str(artifacts)})
        deadline = time.monotonic() + 150
        while not (fixture / "browser-stop").exists():
            assert time.monotonic() < deadline, "browser_fixture.server_timeout"
            server.handle_request()
    finally:
        server.server_close()
        (fixture / "browser-server-exited").touch()


def launch(fixture, target):
    config = json.loads((fixture / "browser-config.json").read_text())
    assert target == config["url"], "browser_fixture.unexpected_target"
    with (fixture / "browser-launches.jsonl").open("a") as record:
        record.write(json.dumps({"url": target}) + "\n")
    profile = fixture / "chrome-profile"
    command = [config["executable"], f"--user-data-dir={profile}", "--no-sandbox",
               "--disable-dev-shm-usage", "--disable-gpu",
               "--disable-background-networking", "--disable-component-update", "--disable-sync",
               "--disable-extensions", "--no-first-run", "--no-default-browser-check",
               "--password-store=basic", "--window-size=800,500", f"--app={target}"]
    env = {**os.environ, "PATH": "/usr/local/bin:/usr/bin:/bin"}
    with (Path(config["artifacts"]) / "workspace-link-browser.log").open("w") as log:
        child = subprocess.Popen(command, env=env, stdout=log, stderr=log, start_new_session=True)
        try:
            publish(fixture / "browser-launched.json", {"pid": child.pid, "url": target})
            deadline = time.monotonic() + 60
            while child.poll() is None and not (fixture / "browser-stop").exists() and time.monotonic() < deadline:
                time.sleep(.05)
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)
            publish(fixture / "browser-exited.json", {"pid": child.pid, "exitCode": child.returncode})


def stop(fixture):
    (fixture / "browser-stop").touch()
    deadline = time.monotonic() + 12
    while True:
        launched = (fixture / "browser-launched.json").exists()
        exited = (fixture / "browser-exited.json").exists()
        if (fixture / "browser-server-exited").exists() and (not launched or exited):
            return
        assert time.monotonic() < deadline, f"browser_fixture.cleanup_timeout.{fixture}"
        time.sleep(.05)


if __name__ == "__main__":
    mode, directory, *extra = sys.argv[1:]
    fixture = Path(directory).resolve(strict=True)
    if mode == "serve": serve(fixture)
    elif mode == "launch":
        assert len(extra) == 1
        launch(fixture, extra[0])
    elif mode == "stop": stop(fixture)
    else: raise ValueError("browser_fixture.invalid_mode")
