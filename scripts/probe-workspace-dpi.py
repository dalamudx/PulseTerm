#!/usr/bin/env python3
"""Compare the existing native font matrix at controlled 1x and 2x GTK scale."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/probes/workspace-dpi'
ARTIFACTS = [
    'workspace-font-result.json', 'workspace-font-wm.log',
    'workspace-settings-native.log', 'workspace-settings-reopened-native.log',
    'workspace-font-reopened-native.log', 'workspace-settings-default.png',
    'workspace-settings-font-output.png', 'workspace-settings-metrics-output.png',
    'workspace-settings-metrics-reset-output.png', 'workspace-font-fallback-output.png',
    'workspace-font-reopened-output.png', 'workspace-font-license-state.txt',
    'workspace-font-license.png', 'workspace-font-control-failure.png',
]


def matrix(scale):
    directory = OUT / f'scale-{scale}'
    directory.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PULSE_FONT_SCALE=str(scale), PULSE_ALPHA_SETTINGS='0', PULSE_HISTORY_STOP='0')
    started = time.time_ns()
    process = None
    try:
        with (directory / 'task.log').open('w') as output:
            process = subprocess.Popen(['mise', 'run', 'probe-workspace-font'], cwd=ROOT,
                env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            result = process.wait(timeout=900)
    finally:
        if process is not None and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
        for name in ARTIFACTS:
            source = ROOT / 'target/probes' / name
            if source.is_file() and source.stat().st_mtime_ns >= started:
                shutil.copy2(source, directory / name)
        for source in (ROOT / 'target/probes').glob('workspace-font-failure-*.png'):
            if source.is_file() and source.stat().st_mtime_ns >= started:
                shutil.copy2(source, directory / source.name)
    assert result == 0, f'dpi.scale_{scale}_task_failed'
    path = directory / 'workspace-font-result.json'
    assert path.is_file() and path.stat().st_mtime_ns >= started, 'dpi.stale_receipt'
    receipt = json.loads(path.read_text())
    assert receipt['version'] == 2 and receipt['passed'] is True
    assert receipt['nativeExitConfirmed'] is True and receipt['nativeSessionCount'] == 3
    assert receipt['gdkScale'] == scale
    assert receipt['physicalWindow'] == {'width': 960 * scale, 'height': 640 * scale}
    assert receipt['ligatureMode'] is False, 'dpi.unexpected_gpu_mode'
    return receipt


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    result_path = OUT / 'result.json'
    state = {'version': 1, 'passed': False, 'stages': [], 'nativeExitConfirmed': False}
    result_path.write_text(json.dumps(state, indent=2)+'\n')
    receipts = []
    for scale in (1, 2):
        receipt = matrix(scale)
        receipts.append(receipt)
        state['stages'].append(receipt)
        result_path.write_text(json.dumps(state, indent=2)+'\n')
    grids = [receipt['grids'] for receipt in receipts]
    for name in ('default14', 'font18', 'height150', 'spacing2'):
        assert grids[0][name][0] == grids[1][name][0], 'dpi.css_native_columns_changed_with_scale.' + name
        row_difference = abs(grids[0][name][1] - grids[1][name][1])
        assert row_difference <= (1 if name in ('height150', 'spacing2') else 0), 'dpi.rows_outside_device_rounding.' + name
    for grid in grids:
        assert grid['height150'][1] == grid['spacing2'][1], 'dpi.spacing_changed_row_count'
    pids = [pid for receipt in receipts for pid in receipt['nativePids']]
    assert len(pids) == len(set(pids)) == 6
    state.update(passed=True, nativeExitConfirmed=True, nativeSessionCount=6,
        checks=['physical_window_doubles', 'logical_columns_unchanged', 'line_height_rounding_within_one_row',
                'explicit_save_refresh_and_reopen', 'restore_inheritance',
                'missing_primary_fallback', 'six_native_sessions_exited'],
        limitations=['Controlled GTK scale on Linux Xvfb software rendering',
                     'No live cross-monitor move, physical DPI, IME, hardware GPU or other-platform acceptance'])
    result_path.write_text(json.dumps(state, indent=2)+'\n')
    print('workspace.dpi_1x_2x_physical_and_logical_grid_native_exit_confirmed')


if __name__ == '__main__':
    main()
