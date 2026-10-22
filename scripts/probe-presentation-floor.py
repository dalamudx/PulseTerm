#!/usr/bin/env python3
"""Fixed X11 versus WebKit input-to-pixel baselines; never product performance acceptance."""
import argparse
import hashlib
import json
import math
import os
import re
from pathlib import Path
import signal
import shutil
import tempfile
import subprocess
import time


MODES = ('native', 'webkit-immediate', 'webkit-raf', 'webkit-raf', 'webkit-immediate', 'native')


def summarize(report):
    samples = report.get('samplesMs')
    starts, observed = report.get('startNs'), report.get('observedNs')
    if (not all(isinstance(rows, list) and len(rows) == 100 for rows in (samples, starts, observed))
            or any(type(v) not in (int, float) or not math.isfinite(v) or v <= 0 for v in samples)
            or any(type(v) is not int or v <= 0 for v in starts + observed)):
        raise ValueError('Incomplete or invalid pixel samples')
    previous = 0
    for elapsed, start, end in zip(samples, starts, observed, strict=True):
        if start <= previous or end <= start or abs(elapsed - (end - start) / 1_000_000) > .000002:
            raise ValueError('Uncorrelated or unordered pixel clocks')
        previous = end
    ordered = sorted(samples)
    return {'sampleCount': 100, 'p50Ms': ordered[49], 'p95Ms': ordered[94],
            'p99Ms': ordered[98], 'maxMs': ordered[-1]}


def identity(pid):
    try:
        parts = (Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()
        return parts[19] if parts[0] != 'Z' else None
    except (FileNotFoundError, ProcessLookupError):
        return None


def tree(pid):
    result, pending = {}, [pid]
    while pending:
        current = pending.pop()
        stamp = identity(current)
        if stamp is None or current in result:
            continue
        result[current] = stamp
        for children in (Path("/proc") / str(current) / "task").glob("*/children"):
            try:
                pending.extend(map(int, children.read_text().split()))
            except (FileNotFoundError, ProcessLookupError):
                pass
    return result


def rollup_memory(text):
    fields = dict(re.findall(r'^(Rss|Pss|Private_Clean|Private_Dirty):\s+(\d+) kB$', text, re.M))
    if len(fields) != 4:
        raise ValueError('Incomplete process memory fields')
    return {'rssBytes': int(fields['Rss']) * 1024, 'pssBytes': int(fields['Pss']) * 1024,
            'privateBytes': (int(fields['Private_Clean']) + int(fields['Private_Dirty'])) * 1024}


def mapped_memory(text):
    totals, name = {}, None
    for line in text.splitlines():
        header = re.match(r'^[0-9a-f]+-[0-9a-f]+\s+\S+\s+[0-9a-f]+\s+\S+\s+\d+\s*(.*)$', line)
        if header:
            name = header[1] or '[anonymous]'
        elif line.startswith('Pss:'):
            amount = re.fullmatch(r'Pss:\s+(\d+) kB', line)
            if name is None or amount is None:
                raise ValueError('Invalid mapped PSS')
            totals[name] = totals.get(name, 0) + int(amount[1]) * 1024
    if not totals:
        raise ValueError('Missing mapped PSS')
    return dict(sorted(totals.items(), key=lambda item: item[1], reverse=True))


def memory(root, mode):
    members = tree(root)
    if root not in members:
        raise RuntimeError('Memory host missing')
    values = []
    for pid, stamp in members.items():
        directory = Path('/proc') / str(pid)
        fields = rollup_memory((directory / 'smaps_rollup').read_text())
        name = (directory / 'comm').read_text().strip()
        maps = mapped_memory((directory / 'smaps').read_text())
        if identity(pid) != stamp:
            raise RuntimeError('Memory process identity changed')
        values.append({'pid': pid, 'startIdentity': stamp, 'name': name, **fields,
                       'mappedPssBytesByBacking': maps})
    if tree(root) != members:
        raise RuntimeError('Memory process tree changed during sample')
    if mode != 'native' and (not any(p['name'].startswith('WebKitWebProces') for p in values)
                             or not any(p['name'].startswith('WebKitNetworkPr') for p in values)):
        raise RuntimeError('Memory WebView processes missing')
    return {'processCount': len(values), 'processes': values,
            **{key: sum(p[key] for p in values) for key in ('rssBytes', 'pssBytes', 'privateBytes')}}


def command(*args, **kwargs):
    return subprocess.run(list(map(str, args)), check=True, capture_output=True, text=True, timeout=15, **kwargs)


def verify_acceleration(metadata, requested):
    if requested not in ('default', 'never'):
        raise ValueError('Invalid requested acceleration policy')
    if metadata.get('requestedAccelerationPolicy') != requested:
        raise ValueError('Acceleration request did not reach fixture')
    actual = metadata.get('actualAccelerationPolicy')
    if actual not in ('always', 'on-demand', 'never'):
        raise ValueError('Missing or invalid actual acceleration policy')
    if requested == 'never' and actual != 'never':
        raise ValueError('Requested acceleration policy was not applied')


def run_group(host, pixels, out, index, mode, xterm_dir=None, memory_only=False):
    log = out / f'{index}-{mode}.log'
    record = {'index': index, 'mode': mode, 'complete': False}
    app, owned = None, {}
    profile = Path(tempfile.mkdtemp(prefix='pulseterm-runtime-profile.', dir='/data/temp'))
    environment = dict(os.environ)
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        folder = profile / name
        folder.mkdir(mode=0o700)
        environment[key] = str(folder)
    environment.update(TMPDIR=str(profile), TMP=str(profile), TEMP=str(profile))
    try:
        with log.open('x') as stream:
            resources = [str(xterm_dir / "lib/xterm.js"), str(xterm_dir / "css/xterm.css")] if mode.startswith("xterm-") else []
            app = subprocess.Popen([str(host), mode, *resources], stdout=stream, stderr=subprocess.STDOUT, start_new_session=True, env=environment)
            deadline = time.monotonic() + 20
            metadata = None
            while time.monotonic() < deadline:
                owned.update(tree(app.pid))
                if app.poll() is not None:
                    raise RuntimeError('Fixture exited before readiness')
                messages = [json.loads(line) for line in log.read_text().splitlines() if line.startswith('{')]
                if messages:
                    if len(messages) != 1 or messages[0].get('ready') is not True:
                        raise ValueError('Invalid or duplicate fixture readiness')
                    metadata = messages[0]
                    break
                time.sleep(.02)
            if metadata is None:
                raise TimeoutError('Fixture readiness timeout')
            xid = metadata['xid']
            if type(xid) is not int or xid <= 0:
                raise ValueError('Invalid X11 window')
            command('xdotool', 'windowactivate', '--sync', xid)
            command('xdotool', 'windowfocus', '--sync', xid)
            time.sleep(.5 if memory_only else .2)
            if mode != "native":
                verify_acceleration(metadata, environment.get("PULSE_PRESENTATION_ACCELERATION", "default"))
            record['runtime'] = metadata
            if memory_only:
                readings = []
                for _ in range(3):
                    readings.append(memory(app.pid, mode))
                    owned.update(tree(app.pid))
                    time.sleep(.1)
                if any({(p['pid'], p['startIdentity']) for p in row['processes']} !=
                       {(p['pid'], p['startIdentity']) for p in readings[0]['processes']} for row in readings):
                    raise RuntimeError('Memory process tree changed between observations')
                record.update(memory=sorted(readings, key=lambda row: row['pssBytes'])[1], memoryReadings=readings)
            else:
                result = command(pixels, xid, 50, 100, 8, 20, 100)
                timing = json.loads(result.stdout)
                (out / f'{index}-{mode}-pixels.json').write_text(json.dumps(timing, indent=2) + '\n')
                record.update(summarize(timing))
            owned.update(tree(app.pid))
            if mode == 'native':
                command('xdotool', 'key', '--window', xid, 'q')
            else:
                app.terminate()
            app.wait(timeout=10)
            record['hostExitCode'] = app.returncode
            if app.returncode != 0:
                raise RuntimeError('Fixture exit was not zero')
            record['complete'] = True
    except Exception as error:
        if isinstance(error, subprocess.CalledProcessError):
            record["commandStderr"] = error.stderr
        record['error'] = f'{type(error).__name__}: {error}'
    finally:
        if app:
            owned.update(tree(app.pid))
            if app.poll() is None:
                record['forcedHostCleanup'] = True
                app.terminate()
                try:
                    app.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(app.pid, signal.SIGKILL)
                    app.wait(timeout=3)
            deadline = time.monotonic() + 5
            while any(identity(pid) == stamp for pid, stamp in owned.items()) and time.monotonic() < deadline:
                time.sleep(.05)
            remaining = [(pid, stamp) for pid, stamp in owned.items() if identity(pid) == stamp]
            record['nativeProcessesDrained'] = not remaining
            if remaining:
                record['forcedNativeCleanup'] = True
                for pid, stamp in remaining:
                    if identity(pid) == stamp:
                        os.kill(pid, signal.SIGKILL)
                # The run remains failed even when forced cleanup succeeds.
                deadline = time.monotonic() + 3
                while any(identity(pid) == stamp for pid, stamp in remaining) and time.monotonic() < deadline:
                    time.sleep(.05)
                record['remainingProcesses'] = [pid for pid, stamp in remaining if identity(pid) == stamp]
        if not record.get("remainingProcesses"):
            shutil.rmtree(profile)
            record["temporaryProfileCleaned"] = not profile.exists()
        else:
            record["remainingProfile"] = str(profile)
        (out / f'{index}-{mode}-result.json').write_text(json.dumps(record, indent=2) + '\n')
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--pixels', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument("--xterm-dir", type=Path, help="Optional local locked @xterm/xterm 6.0.0 resources for page-local glyph suite")
    parser.add_argument("--memory-only", action="store_true", help="Idle whole-process PSS/RSS baseline; no key/pixel latency samples")
    args = parser.parse_args()
    for binary in (args.host, args.pixels):
        if not binary.is_absolute() or not binary.is_file():
            parser.error('Existing absolute fixture binaries required')
    resources = {}
    modes = MODES
    if args.xterm_dir:
        if not args.xterm_dir.is_absolute():
            parser.error('Absolute local xterm directory required')
        manifest = json.loads((args.xterm_dir / 'package.json').read_text())
        if manifest.get('name') != '@xterm/xterm' or manifest.get('version') != '6.0.0':
            parser.error('Expected locked @xterm/xterm 6.0.0')
        resources = {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                     for path in (args.xterm_dir / 'lib/xterm.js', args.xterm_dir / 'css/xterm.css')}
        modes = ('webkit-immediate', 'xterm-dirty', 'xterm-full', 'xterm-full', 'xterm-dirty', 'webkit-immediate')
    if args.memory_only:
        modes = (("native", "webkit-immediate", "xterm-dirty", "xterm-dirty", "webkit-immediate", "native")
                 if args.xterm_dir else ("native", "webkit-immediate", "webkit-immediate", "native"))
    args.out.mkdir(parents=True, exist_ok=False)
    report = {'schemaVersion': 1, 'complete': False, 'groups': [],
              'sampling': 'Same unmodified product XTest/pixel helper, 8x20 region, 0.5ms sleep between reads, 100 samples per group',
              'environment': {key: os.environ.get(key) for key in ('GDK_BACKEND', 'LIBGL_ALWAYS_SOFTWARE', 'WEBKIT_DISABLE_DMABUF_RENDERER', 'PULSE_PRESENTATION_ACCELERATION')},
              'binarySha256': {binary.name: hashlib.sha256(binary.read_bytes()).hexdigest() for binary in (args.host, args.pixels)},
              'limitations': ['No PTY, backend, product IPC, recovery or product-authority path',
                              'Different content from product glyph; baseline is not subtracted from product percentiles',
                              'Xvfb software rendering only; XGetImage observations are not physical display/compositor receipts',
                              'Mirrored mode order controls coarse drift only; not randomized or simultaneous product A/B',
                              'No 16ms product acceptance or runtime choice inferred from this page']}
    report["xtermResourceSha256"] = resources
    report["suite"] = "xterm-local-echo" if args.xterm_dir else "native-dom-animation"
    if args.memory_only:
        report.update(suite="runtime-memory", sampling="Fixed0.5s ready settling, median of3 whole-process PSS observations100ms apart; all raw readings retained",
                      memoryUnit="bytes; MB=1000000bytes", productMemoryGateEvaluated=False)
        report["limitations"].extend(["Small C/GTK host is not the Tauri product; cannot subtract totals to isolate product overhead",
                                      "Mapped PSS is a separate observation after smaps_rollup, not exact allocation or additive to rollup",
                                      "Single mirrored sequence; not randomized repeated hardware or cross-platform acceptance"])
    def interrupted(signum, frame):
        raise InterruptedError("Presentation suite interrupted; retain failure and drain owned hosts")

    previous_term_handler = signal.signal(signal.SIGTERM, interrupted)
    try:
        for index, mode in enumerate(modes, 1):
            result = run_group(args.host, args.pixels, args.out, index, mode, args.xterm_dir, args.memory_only)
            report['groups'].append(result)
            if not result['complete'] or not result.get('nativeProcessesDrained'):
                raise RuntimeError('Incomplete fixture group; no remaining groups launched')
        report['complete'] = True
    finally:
        signal.signal(signal.SIGTERM, previous_term_handler)
        (args.out / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    fields = ("mode", "memory") if args.memory_only else ("mode", "sampleCount", "p50Ms", "p95Ms", "p99Ms")
    print(json.dumps({"complete": True, "groups": [{key: row[key] for key in fields} for row in report["groups"]]}))


if __name__ == '__main__':
    main()
