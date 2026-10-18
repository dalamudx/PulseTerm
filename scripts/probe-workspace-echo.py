#!/usr/bin/env python3
"""Real XTest-to-PTY-to-glyph echo measurement, called by the native load driver."""
import csv
import io
import json
import math
import subprocess


def verify(s):
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
    result = subprocess.run([str(s['ROOT'] / 'target/probes/echo-pixels'), str(xid), str(x), str(y), '8', str(line_height), '100'],
                            check=True, capture_output=True, text=True, timeout=20)
    timing = json.loads(result.stdout)
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
    assert len(native) == len(samples)
    input_ms, output_ms = [], []
    for started, observed, sample in zip(timing['startNs'], timing['observedNs'], native, strict=True):
        assert started <= sample['receivedNs'] <= sample['writtenNs'] <= observed, 'echo.uncorrelated_glyph_change'
        input_ms.append((sample['receivedNs'] - started) / 1000000)
        output_ms.append((observed - sample['writtenNs']) / 1000000)
    report.update(echoInputP95Ms=sorted(input_ms)[94], echoOutputP95Ms=sorted(output_ms)[94],
                  echoInputSamplesMs=input_ms, echoOutputSamplesMs=output_ms, echoSamplesCorrelatedWithNativeWrites=True)
    run('/usr/bin/import', '-window', xid, out / 'echo-complete.png')
    report['measurementsComplete'] = True
    print(json.dumps({'echoP95Ms': p95, 'echoSamples': len(samples), 'targetMet': p95 < 16}))
