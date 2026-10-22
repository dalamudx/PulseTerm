#!/usr/bin/env python3
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('floor', Path(__file__).with_name('probe-presentation-floor.py'))
floor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(floor)


def fixture():
    starts = [1_000_000_000 + i * 100_000_000 for i in range(100)]
    return {'startNs': starts, 'observedNs': [v + (i + 1) * 1_000_000 for i, v in enumerate(starts)],
            'samplesMs': list(range(1, 101))}


class PixelBaselineTests(unittest.TestCase):
    def test_exact_nearest_rank_without_discarding_tail_or_mutating(self):
        report = fixture()
        original = copy.deepcopy(report)
        self.assertEqual(floor.summarize(report), {'sampleCount': 100, 'p50Ms': 50, 'p95Ms': 95, 'p99Ms': 99, 'maxMs': 100})
        self.assertEqual(report, original)

    def test_incomplete_empty_or_extra_samples_reject(self):
        for key in ('startNs', 'observedNs', 'samplesMs'):
            for size in (0, 99, 101):
                report = fixture()
                report[key] = (report[key] + [1])[:size]
                with self.subTest(key=key, size=size), self.assertRaises(ValueError):
                    floor.summarize(report)

    def test_invalid_nan_bool_negative_and_uncorrelated_clocks_reject(self):
        for key, value in [('samplesMs', float('nan')), ('samplesMs', True), ('samplesMs', -1),
                           ('startNs', True), ('observedNs', 1), ('observedNs', 1_000_000_000)]:
            report = fixture()
            report[key][0] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                floor.summarize(report)
        report = fixture()
        report['startNs'][1] = report['startNs'][0]
        with self.assertRaises(ValueError):
            floor.summarize(report)


class FixtureFailureTests(unittest.TestCase):
    def test_real_early_exit_preserves_failure_and_never_claims_samples(self):
        import tempfile
        with tempfile.TemporaryDirectory(prefix='presentation-exit-', dir='/data/temp') as directory:
            root = Path(directory)
            host = root / 'host'
            host.write_text('#!/bin/sh\nexit 17\n')
            host.chmod(0o700)
            report = floor.run_group(host, root / 'missing-pixels', root, 1, 'native')
            self.assertFalse(report['complete'])
            self.assertNotIn('sampleCount', report)
            self.assertTrue(report['nativeProcessesDrained'])
            self.assertTrue(report["temporaryProfileCleaned"])
            self.assertIn('before readiness', report['error'])
            self.assertTrue((root / '1-native-result.json').is_file())

    def test_duplicate_readiness_rejects_and_cleans_exact_host_process(self):
        import tempfile
        with tempfile.TemporaryDirectory(prefix='presentation-duplicate-', dir='/data/temp') as directory:
            root = Path(directory)
            host = root / 'host'
            host.write_text('#!/usr/bin/python3\nimport time\nprint(\'{"ready":true,"xid":1}\\n{"ready":true,"xid":2}\',flush=True)\ntime.sleep(30)\n')
            host.chmod(0o700)
            report = floor.run_group(host, root / 'missing-pixels', root, 1, 'webkit-immediate')
            self.assertFalse(report['complete'])
            self.assertIn('duplicate', report['error'])
            self.assertTrue(report['forcedHostCleanup'])
            self.assertTrue(report['nativeProcessesDrained'])
            self.assertTrue(report["temporaryProfileCleaned"])

    def test_stale_results_directory_cannot_be_reused(self):
        import sys
        import tempfile
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(prefix='presentation-stale-', dir='/data/temp') as directory:
            root = Path(directory)
            binary = root / 'host'
            binary.write_text('fixed-test')
            out = root / 'results'
            out.mkdir()
            receipt = out / 'summary.json'
            receipt.write_text('preserved-old-failure')
            with patch.object(sys, 'argv', ['probe', '--host', str(binary), '--pixels', str(binary), '--out', str(out)]):
                with self.assertRaises(FileExistsError):
                    floor.main()
            self.assertEqual(receipt.read_text(), 'preserved-old-failure')


class ResourceIdentityTests(unittest.TestCase):
    def test_wrong_xterm_version_is_rejected_before_result_creation(self):
        import json
        import sys
        import tempfile
        from contextlib import redirect_stderr
        from io import StringIO
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(prefix='presentation-resource-', dir='/data/temp') as directory:
            root = Path(directory)
            binary = root / 'host'
            binary.write_text('fixed-test')
            package = root / 'xterm'
            package.mkdir()
            (package / 'package.json').write_text(json.dumps({'name': '@xterm/xterm', 'version': '0.0.0'}))
            out = root / 'results'
            with patch.object(sys, 'argv', ['probe', '--host', str(binary), '--pixels', str(binary), '--out', str(out), '--xterm-dir', str(package)]):
                with redirect_stderr(StringIO()), self.assertRaises(SystemExit) as error:
                    floor.main()
            self.assertEqual(error.exception.code, 2)
            self.assertFalse(out.exists())

    def test_missing_locked_bundle_cannot_create_a_result(self):
        import json
        import sys
        import tempfile
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(prefix='presentation-missing-', dir='/data/temp') as directory:
            root = Path(directory)
            binary = root / 'host'
            binary.write_text('fixed-test')
            package = root / 'xterm'
            package.mkdir()
            (package / 'package.json').write_text(json.dumps({'name': '@xterm/xterm', 'version': '6.0.0'}))
            out = root / 'results'
            with patch.object(sys, 'argv', ['probe', '--host', str(binary), '--pixels', str(binary), '--out', str(out), '--xterm-dir', str(package)]):
                with self.assertRaises(FileNotFoundError):
                    floor.main()
            self.assertFalse(out.exists())


class InterruptedSuiteTests(unittest.TestCase):
    def test_sigterm_retains_incomplete_receipt_and_restores_signal_handler(self):
        import json
        import signal
        import sys
        import tempfile
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(prefix='presentation-interrupted-', dir='/data/temp') as directory:
            root = Path(directory)
            binary = root / 'host'
            binary.write_text('fixed-test')
            out = root / 'results'
            previous = signal.getsignal(signal.SIGTERM)
            def interrupt(*args):
                signal.raise_signal(signal.SIGTERM)
            with patch.object(sys, 'argv', ['probe', '--host', str(binary), '--pixels', str(binary), '--out', str(out)]):
                with patch.object(floor, 'run_group', side_effect=interrupt):
                    with self.assertRaises(InterruptedError):
                        floor.main()
            self.assertIs(signal.getsignal(signal.SIGTERM), previous)
            self.assertFalse(json.loads((out / 'summary.json').read_text())['complete'])


class RuntimeMemoryTests(unittest.TestCase):
    def test_rollup_uses_pss_and_private_bytes_without_rss_double_count(self):
        self.assertEqual(floor.rollup_memory('Rss: 200 kB\nPss: 100 kB\nPrivate_Clean: 20 kB\nPrivate_Dirty: 30 kB\n'),
                         {'rssBytes': 204800, 'pssBytes': 102400, 'privateBytes': 51200})
        for text in ['', 'Rss: 200 kB\nPss: 100 kB\n', 'Rss: -1 kB\nPss: 100 kB\nPrivate_Clean: 1 kB\nPrivate_Dirty: 1 kB\n']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                floor.rollup_memory(text)

    def test_mapping_groups_shared_library_and_anonymous_pss(self):
        raw = ('1000-2000 r-xp 0000 00:01 1 /fixed/lib.so\nPss: 3 kB\nRss: 9 kB\n'
               '2000-3000 rw-p 1000 00:01 1 /fixed/lib.so\nPss: 4 kB\n'
               '3000-4000 rw-p 0000 00:00 0\nPss: 5 kB\n')
        self.assertEqual(floor.mapped_memory(raw), {'/fixed/lib.so': 7168, '[anonymous]': 5120})
        for invalid in ['', 'Pss: 3 kB', '1000-2000 rw-p 0000 00:00 0\nPss: -1 kB']:
            with self.subTest(raw=invalid), self.assertRaises(ValueError):
                floor.mapped_memory(invalid)

    def test_actual_host_memory_requires_real_webkit_children_for_webview_mode(self):
        import subprocess
        host = subprocess.Popen(['/bin/sleep', '30'])
        try:
            snapshot = floor.memory(host.pid, 'native')
            self.assertGreater(snapshot['pssBytes'], 0)
            self.assertEqual(snapshot['processCount'], 1)
            self.assertEqual(snapshot['pssBytes'], sum(p['pssBytes'] for p in snapshot['processes']))
            self.assertTrue(snapshot['processes'][0]['mappedPssBytesByBacking'])
            with self.assertRaisesRegex(RuntimeError, 'WebView processes missing'):
                floor.memory(host.pid, 'webkit-immediate')
        finally:
            host.terminate()
            host.wait(timeout=3)
        with self.assertRaisesRegex(RuntimeError, 'host missing'):
            floor.memory(host.pid, 'native')

    def test_memory_failure_retains_receipt_and_drains_host_without_claiming_pixels(self):
        import tempfile
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(prefix='runtime-memory-failure-', dir='/data/temp') as directory:
            root = Path(directory)
            host = root / 'host'
            host.write_text('#!/usr/bin/python3\nimport time\nprint(\'{"ready":true,"xid":1}\',flush=True)\ntime.sleep(30)\n')
            host.chmod(0o700)
            with patch.object(floor, 'command'), patch.object(floor, 'memory', side_effect=RuntimeError('missing memory')):
                report = floor.run_group(host, root / 'unused-pixels', root, 1, 'native', memory_only=True)
            self.assertFalse(report['complete'])
            self.assertNotIn('sampleCount', report)
            self.assertNotIn('memory', report)
            self.assertTrue(report['nativeProcessesDrained'])
            self.assertTrue(report["temporaryProfileCleaned"])
            self.assertIn('missing memory', report['error'])
            self.assertTrue((root / '1-native-result.json').is_file())


class AccelerationPolicyTests(unittest.TestCase):
    def test_supported_actual_policy_is_read_back_for_default_and_never(self):
        for actual in ['always', 'on-demand', 'never']:
            floor.verify_acceleration({'requestedAccelerationPolicy': 'default', 'actualAccelerationPolicy': actual}, 'default')
        floor.verify_acceleration({'requestedAccelerationPolicy': 'never', 'actualAccelerationPolicy': 'never'}, 'never')

    def test_missing_mismatched_or_unapplied_policy_rejects(self):
        for metadata, requested in [({}, 'never'),
                                    ({'requestedAccelerationPolicy': 'never'}, 'never'),
                                    ({'requestedAccelerationPolicy': 'never', 'actualAccelerationPolicy': 'always'}, 'never'),
                                    ({'requestedAccelerationPolicy': 'default', 'actualAccelerationPolicy': 'never'}, 'never'),
                                    ({'requestedAccelerationPolicy': 'default', 'actualAccelerationPolicy': 'unknown'}, 'default'),
                                    ({'requestedAccelerationPolicy': 'invalid', 'actualAccelerationPolicy': 'never'}, 'invalid')]:
            with self.subTest(metadata=metadata, requested=requested), self.assertRaises(ValueError):
                floor.verify_acceleration(metadata, requested)


if __name__ == '__main__':
    unittest.main()
