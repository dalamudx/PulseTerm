"""Probe-owned public pin checks only. Never create or edit a user database."""
import pathlib, sqlite3, sys, tempfile
path, mode, key_path, port = sys.argv[1:]
p = pathlib.Path(path).resolve(strict=True)
root = pathlib.Path(tempfile.gettempdir()).resolve(strict=True)
assert p.is_relative_to(root) and any(part.startswith("workspace-fingerprint.") for part in p.relative_to(root).parts)
key = " ".join(pathlib.Path(key_path).read_text().split()[:2])
with sqlite3.connect(p.as_uri()+"?mode=ro", uri=True) as c:
    assert c.execute("pragma user_version").fetchone()[0] == 5
    revision = c.execute("select revision from known_hosts_state").fetchone()[0]
    rows = c.execute("select id,created_at,hlc,deleted,host,port,key from known_hosts").fetchall()
    if mode == "empty":
        assert revision == 0 and rows == []
    elif mode == "saved":
        assert revision == 1 and len(rows) == 1
        row = rows[0]
        assert row[0][14] == "7" and row[1] > 0 and row[2] and row[3] == 0
        assert row[4:] == ("127.0.0.1", int(port), key)
    else:
        raise ValueError("unknown probe mode")
print("workspace.fingerprint_private_pin_bytes_"+mode+"_confirmed")
