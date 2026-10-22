#!/usr/bin/env python3
"""Strict diagnostic receipts and cleanup; synthetic logs are not product acceptance."""
import importlib.util
import json
from pathlib import Path
import signal
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('tauri_memory', Path(__file__).with_name('probe-tauri-memory.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class PageReceiptTests(unittest.TestCase):
    def test_blank_and_confirmed_workspace_receipts(self):
        probe.verify_page('workspace.runtime_memory.page.blank\n', 'blank')
        probe.verify_page('workspace.runtime_memory.page.workspace\nworkspace.document_confirmed\nworkspace.profiles_listed\n', 'workspace')

    def test_requested_mode_is_not_actual_page_evidence(self):
        for raw in ['', 'PULSE_RUNTIME_MEMORY_PAGE=blank', 'workspace.runtime_memory.page.workspace',
                    'workspace.runtime_memory.page.blank\nworkspace.runtime_memory.page.blank',
                    'workspace.runtime_memory.page.blank\nworkspace.runtime_memory.page.workspace']:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                probe.verify_page(raw, 'blank')

    def test_blank_never_gets_document_profile_or_cli_authority(self):
        for event in ['document_confirmed', 'profiles_requested', 'profiles_listed', 'cli_presentation_ready']:
            with self.subTest(event=event), self.assertRaises(ValueError):
                probe.verify_page('workspace.runtime_memory.page.blank\nworkspace.' + event, 'blank')

    def test_both_modes_reject_sessions_input_and_failure(self):
        for mode in ['blank', 'workspace']:
            base = 'workspace.runtime_memory.page.' + mode + '\n'
            if mode == 'workspace':
                base += 'workspace.document_confirmed\nworkspace.profiles_listed\n'
            for event in ['session_opened', 'terminal_input_ready', 'shutdown_failed', 'terminal_failure']:
                with self.subTest(mode=mode, event=event), self.assertRaises(ValueError):
                    probe.verify_page(base + 'workspace.' + event, mode)

    def test_workspace_requires_single_confirmation_and_profiles_loaded(self):
        for suffix in ['', '\nworkspace.profiles_listed', '\nworkspace.document_confirmed',
                       '\nworkspace.document_confirmed\nworkspace.profiles_listed\nworkspace.profiles_listed']:
            with self.subTest(suffix=suffix), self.assertRaises(ValueError):
                probe.verify_page('workspace.runtime_memory.page.workspace' + suffix, 'workspace')


class SuiteFailureTests(unittest.TestCase):
    def test_actual_early_exit_records_failure_and_cleans_private_profile(self):
        with tempfile.TemporaryDirectory(prefix='tauri-memory-test.', dir='/data/temp') as directory:
            out = Path(directory)
            row = probe.run_group(Path('/bin/false'), out, 1, 'blank')
            self.assertFalse(row['complete'])
            self.assertNotIn('memory', row)
            self.assertIn('before actual page readiness', row['error'])
            self.assertTrue(row['nativeProcessesDrained'])
            self.assertTrue(row['temporaryProfileCleaned'])
            self.assertEqual(json.loads((out / '1-blank-result.json').read_text()), row)

    def test_existing_failure_directory_never_overwritten(self):
        with tempfile.TemporaryDirectory(prefix='tauri-memory-test.', dir='/data/temp') as directory:
            out = Path(directory)
            (out / 'summary.json').write_text('old-failure')
            with patch.object(sys, 'argv', ['probe', '--binary', '/bin/false', '--out', str(out)]):
                with self.assertRaises(FileExistsError):
                    probe.main()
            self.assertEqual((out / 'summary.json').read_text(), 'old-failure')

    def test_first_failed_group_stops_remaining_groups_and_retains_summary(self):
        with tempfile.TemporaryDirectory(prefix='tauri-memory-test.', dir='/data/temp') as directory:
            out = Path(directory) / 'out'
            with patch.object(sys, 'argv', ['probe', '--binary', '/bin/false', '--out', str(out)]):
                with self.assertRaisesRegex(RuntimeError, 'Incomplete Tauri memory group'):
                    probe.main()
            result = json.loads((out / 'summary.json').read_text())
            self.assertFalse(result['complete'])
            self.assertEqual(len(result['groups']), 1)
            self.assertFalse(result['productMemoryGateEvaluated'])
            self.assertTrue(result['groups'][0]['temporaryProfileCleaned'])

    def test_interrupt_retains_incomplete_summary_and_restores_handler(self):
        with tempfile.TemporaryDirectory(prefix='tauri-memory-test.', dir='/data/temp') as directory:
            out = Path(directory) / 'out'
            previous = signal.getsignal(signal.SIGTERM)
            def interrupt(*_args):
                signal.raise_signal(signal.SIGTERM)
            with patch.object(sys, 'argv', ['probe', '--binary', '/bin/false', '--out', str(out)]):
                with patch.object(probe, 'run_group', side_effect=interrupt):
                    with self.assertRaises(InterruptedError):
                        probe.main()
            self.assertIs(signal.getsignal(signal.SIGTERM), previous)
            self.assertFalse(json.loads((out / 'summary.json').read_text())['complete'])


if __name__ == '__main__':
    unittest.main()
