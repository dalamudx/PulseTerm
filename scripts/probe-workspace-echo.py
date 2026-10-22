#!/usr/bin/env python3
"""Real XTest-to-PTY-to-glyph echo measurement, called by the native load driver."""
import csv
import io
import json
import math
import subprocess


def verify(s):
    selected_yield = s['env'].get('PULSE_ECHO_DRAW_YIELD', '0')
    assert selected_yield in ('0', '1'), 'echo.invalid_draw_yield'
    if selected_yield == '1':
        assert s['env'].get('PULSE_ECHO_TIMING') == '1', 'echo.draw_yield_requires_diagnostics'
        s['xdo']('key', '--clearmodifiers', 'ctrl+shift+F9')
    run, xdo, wait = s['run'], s['xdo'], s['wait']
    out, fixture, xid = s['OUT'], s['fixture'], s['xid']
    report = s['report']
    xdo('type', '--clearmodifiers', '--delay', '0', 'E')
    image = out / 'echo-ready.png'

    def ready():
        run('/usr/bin/import', '-window', xid, image)
        result = run('/usr/bin/env', 'OMP_THREAD_LIMIT=1', '/usr/bin/tesseract', image, 'stdout', '--psm', '6', 'tsv')
        words = list(csv.DictReader(io.StringIO(result.stdout), delimiter='\t'))
        anchors = [word for word in words if word['text'].upper() == 'ECHO']
        return anchors if len(anchors) >= 2 else None

    anchors = wait(ready, 'echo_reference_lines', 15)
    anchors.sort(key=lambda word: int(word['top']))
    first, second = anchors[:2]
    line_height = int(second['top']) - int(first['top'])
    assert 8 <= line_height <= 40, 'echo.invalid_line_height'
    x, y = int(first['left']), int(first['top']) + 2 * line_height
    assert s['accessible']('Terminal input', 'focus').returncode == 0
    pacing = s["env"].get("PULSE_ECHO_PACING", "fixed")
    assert pacing in ("fixed", "jitter"), "echo.invalid_pacing"
    result = subprocess.run([s['env'].get('PULSE_ECHO_PIXELS', str(s['ROOT'] / 'target/probes/echo-pixels')), str(xid), str(x), str(y), '8', str(line_height), '100', pacing],
                            check=True, capture_output=True, text=True, timeout=20)
    timing = json.loads(result.stdout)
    (out / 'echo-pixels.json').write_text(json.dumps(timing, indent=2) + '\n')
    assert timing.get("pacingMode") == pacing and len(timing.get("requestedPauseNs", [])) == 100, "echo.unverified_pacing"
    report["echoPacingMode"] = pacing
    samples = timing['samplesMs']
    assert len(samples) == 100 and all(value > 0 for value in samples)
    ordered = sorted(samples)
    p95 = ordered[math.ceil(len(ordered) * .95) - 1]
    report.update(command='fixed-PTY-echo', textBytes=0, echoSampleCount=len(samples), echoSamplesMs=samples,
                  echoP50Ms=ordered[49], echoP95Ms=p95, echoMaxMs=ordered[-1], echoP95Under16Ms=p95 < 16,
                  echoObservation='XTest injection to first changed X11 glyph pixels, 0.5ms sampling; no IPC-only inference',
                  echoPixelRegion={'x': x, 'y': y, 'width': 8, 'height': line_height}, measurementsComplete=False,
                  limitations=['Xvfb/software rendering only; no hardware or cross-platform echo acceptance',
                               'First glyph change; no full terminal-state comparison or flood-load echo measurement'])
    xdo('type', '--clearmodifiers', '--delay', '0', 'Q')
    wait(lambda: s['count']('terminal_eof_acked') == 1, 'echo_native_eof', 10)
    native = json.loads((fixture / 'echo-native.json').read_text())
    (out / 'echo-native.json').write_text(json.dumps(native, indent=2) + '\n')
    assert len(native) == len(samples)
    input_ms, output_ms = [], []
    for started, observed, sample in zip(timing['startNs'], timing['observedNs'], native, strict=True):
        assert started <= sample['receivedNs'] <= sample['writtenNs'] <= observed, 'echo.uncorrelated_glyph_change'
        input_ms.append((sample['receivedNs'] - started) / 1000000)
        output_ms.append((observed - sample['writtenNs']) / 1000000)
    report.update(echoInputP95Ms=sorted(input_ms)[94], echoOutputP95Ms=sorted(output_ms)[94],
                  echoInputSamplesMs=input_ms, echoOutputSamplesMs=output_ms, echoSamplesCorrelatedWithNativeWrites=True)
    if s['env'].get('PULSE_ECHO_TIMING', '0') == '1':
        prefix = 'workspace.echo_timing:'
        dumps = [json.loads(line[len(prefix):]) for line in s['log_path'].read_text().splitlines() if line.startswith(prefix)]
        assert len(dumps) == 1, 'echo.missing_or_duplicate_browser_timing'
        dump = dumps[0]
        assert dump['schemaVersion'] == 1 and dump['overflow'] is False, 'echo.incomplete_browser_timing'
        assert dump['batches'] and dump['frames'], 'echo.empty_browser_timing'
        assert all("acknowledgeStartedMs" in frame for frame in dump["frames"]), "echo.missing_browser_ack_start"
        (out / 'echo-frontend.json').write_text(json.dumps(dump, indent=2) + '\n')
        report['echoBrowserTimingExported'] = True
        prefix = 'workspace.echo_native_timing:'
        transports = [json.loads(line[len(prefix):]) for line in s['log_path'].read_text().splitlines() if line.startswith(prefix)]
        assert len(transports) == 1, 'echo.missing_or_duplicate_native_transport_timing'
        transport = transports[0]
        assert transport['schemaVersion'] == 1 and transport['clockDomain'] == 'Linux CLOCK_MONOTONIC', 'echo.invalid_native_transport_clock'
        assert len(transport['frames']) == len(dump['frames']), 'echo.incomplete_native_transport_timing'
        (out / 'echo-transport.json').write_text(json.dumps(transport, indent=2) + '\n')
        report['echoNativeTransportExported'] = True
        report['limitations'].append('Browser timings use performance.now; no absolute clock alignment with the native helper or compositor acknowledgement')
    run('/usr/bin/import', '-window', xid, out / 'echo-complete.png')
    report['measurementsComplete'] = True
    print(json.dumps({'echoP95Ms': p95, 'echoSamples': len(samples), 'targetMet': p95 < 16}))
