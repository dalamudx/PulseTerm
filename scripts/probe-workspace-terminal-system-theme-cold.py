"""Real cold CLI palette checks; only pulse launches the private installed GUI."""
import ctypes
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import time


def verify(c, workers, assert_palette, finish_workers):
    assert ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) == 0, 'palette.cold_subreaper'
    fixture, out, env = c['fixture'], c['OUTPUT'], c['environment']
    run, wait, key = (c[name] for name in ('run', 'wait', 'key'))
    bundle = fixture / 'cold-bundle'
    bundle.mkdir(mode=0o700)
    binaries = Path(os.environ['CARGO_TARGET_DIR']) / 'debug'
    for name in ('pulse', 'pulseterm-workspace'):
        shutil.copy2(binaries / name, bundle / name)
        assert (bundle / name).stat().st_nlink == 1, 'palette.cold_hardlinked_binary'
    gui = bundle / 'pulseterm-workspace'
    runtime = Path(env['XDG_RUNTIME_DIR']) / 'pulseterm-workspace-cli'
    saved_config = c['config'].read_bytes()
    cases = []

    def owned_guis():
        found = []
        for proc in Path('/proc').iterdir():
            if not proc.name.isdigit():
                continue
            try:
                if (proc / 'exe').resolve() == gui and (
                    'XDG_RUNTIME_DIR=' + env['XDG_RUNTIME_DIR']
                ).encode() in (proc / 'environ').read_bytes().split(b'\0'):
                    found.append(int(proc.name))
            except (FileNotFoundError, PermissionError, ProcessLookupError):
                pass
        return found

    for mode, preset in (('dark', 'midnight'), ('light', 'paper')):
        c['change'](mode)
        assert not owned_guis() and not (runtime / 'control.sock').exists()
        previous = set(fixture.glob('palette-*'))
        pid = None
        reaped = False
        with subprocess.Popen([str(bundle / 'pulse'), 'new-tab', 'sh'], env=env,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              text=True, start_new_session=True) as client:
            try:
                def primary():
                    found = owned_guis()
                    assert len(found) <= 1, 'palette.cold_extra_gui'
                    assert client.poll() is None, 'palette.cold_cli_exited_before_approval'
                    return found[0] if found else None
                pid = wait(primary, 'cold_' + mode + '_spawn', 8)
                def window():
                    found = subprocess.run(['/usr/bin/xdotool', 'search', '--onlyvisible', '--all',
                                            '--pid', str(pid), '--name', '^PulseTerm · 工作区预览$'],
                                           capture_output=True, text=True, timeout=3).stdout.split()
                    return found[0] if len(found) == 1 else None
                xid = wait(window, 'cold_' + mode + '_window', 8)
                c['xid'] = xid
                assert int(run('/usr/bin/xdotool', 'getwindowpid', xid).stdout) == pid
                run('/usr/bin/xdotool', 'windowactivate', '--sync', xid)
                def approval():
                    assert client.poll() is None, 'palette.cold_request_lost'
                    return subprocess.run([c['helper'], str(pid), '允许 CLI 创建本地终端？', 'bounds'],
                                          capture_output=True, text=True, timeout=3).returncode == 0
                wait(approval, 'cold_' + mode + '_approval', 8)
                assert set(fixture.glob('palette-*')) == previous, 'palette.cold_pty_before_approval'
                run('/usr/bin/import', '-window', xid, out / ('cold-' + mode + '-approval.png'))
                assert run(c['helper'], pid, '创建终端', 'focus').returncode == 0
                key('space')
                output, error = client.communicate(timeout=12)
                (out / ('cold-' + mode + '-client.json')).write_text(json.dumps(
                    dict(pid=client.pid, returncode=client.returncode, stdout=output, stderr=error), indent=2) + '\n')
                assert client.returncode == 0 and re.fullmatch(r'session [1-9][0-9]*\n', output), 'palette.cold_cli_failed'
                found = wait(lambda: [path for path in fixture.glob('palette-*')
                                     if path not in previous and (path / 'ready.json').exists()], 'cold_worker')
                assert len(found) == 1 and owned_guis() == [pid], 'palette.cold_extra_native'
                entry = dict(base=found[0], pid=json.loads((found[0] / 'ready.json').read_text())['pid'],
                             round=0, origin='cold-cli')
                workers.append(entry)
                assert_palette(entry, preset, 'cold-' + mode + '-new')
                assert c['config'].read_bytes() == saved_config, 'palette.cold_changed_config'
                finish_workers()
                key('alt+F4')
                def exited():
                    nonlocal reaped
                    assert isinstance(pid, int) and pid > 0, 'palette.cold_missing_pid'
                    waited, status = os.waitpid(pid, os.WNOHANG)
                    if not waited:
                        return False
                    reaped = True
                    code = os.waitstatus_to_exitcode(status)
                    (out / ('cold-' + mode + '-gui-exit.json')).write_text(json.dumps(dict(pid=pid, exitCode=code)) + '\n')
                    assert code == 0, 'palette.cold_gui_exit'
                    return True
                wait(exited, 'cold_' + mode + '_gui_exit', 15)
                def native_group_gone():
                    # Reap this CLI's adopted WebKit children before testing its group.
                    while True:
                        try:
                            if os.waitpid(-client.pid, os.WNOHANG)[0] == 0:
                                break
                        except ChildProcessError:
                            break
                    try:
                        os.killpg(client.pid, 0)
                    except ProcessLookupError:
                        return True
                    return False
                wait(native_group_gone, 'cold_' + mode + '_native_group_exit', 8)
                assert not owned_guis() and not (runtime / 'control.sock').exists() and not (runtime / 'control.token').exists()
                c['windows_exited'] += 1
                cases.append(dict(system=mode, preset=preset, guiPid=pid, cliPid=client.pid,
                                  workerPid=entry['pid'], approvalBeforePty=True, guiExitCode=0,
                                  cliExitCode=0, endpointsReclaimed=True, nativeGroupExited=True))
            finally:
                if client.poll() is None:
                    client.terminate()
                    try:
                        client.communicate(timeout=3)
                    except subprocess.TimeoutExpired:
                        client.kill()
                        client.communicate(timeout=3)
                if pid is not None and not reaped:
                    if pid in owned_guis():
                        os.kill(pid, signal.SIGTERM)
                    deadline = time.monotonic() + 3
                    while True:
                        try:
                            if os.waitpid(pid, os.WNOHANG)[0]:
                                break
                        except ChildProcessError:
                            break
                        if time.monotonic() >= deadline:
                            if pid in owned_guis():
                                os.kill(pid, signal.SIGKILL)
                                os.waitpid(pid, 0)
                            break
                        time.sleep(.05)
                try:
                    os.killpg(client.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
    return cases
