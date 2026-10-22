"""Behavioral boundaries of candidate graph review, without compiling targets."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('distribution_review', Path(__file__).with_name('distribution-review.py'))
assert spec is not None and spec.loader is not None
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='distribution-review-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        files = {
            'Cargo.toml': '[workspace]\nmembers=["app"]\n[workspace.package]\nversion="7.1.0"\n',
            'app/Cargo.toml': '[package]\nname="pulseterm-desktop"\nversion.workspace=true\n',
            'apps/desktop/src-tauri/workspace/tauri.conf.json': '{"bundle":{"active":false}}',
            'Cargo.lock': '', 'pnpm-lock.yaml': '', 'ui/package.json': '{}',
            'third-party/inventory.json': '{}', 'scripts/distribution-review.py': '', '.mise.toml': '',
        }
        for name, content in files.items():
            p = self.root / name
            p.parent.mkdir(exist_ok=True, parents=True)
            p.write_text(content)
        self.inventory = {'releaseApproved': False, 'packages': [
            {'id': 'cargo:ordinary@1.0.0', 'ecosystem': 'cargo', 'textStatus': 'missing-from-published-package'},
            {'id': 'cargo:compiler@1.0.0', 'ecosystem': 'cargo', 'textStatus': 'collected'},
            {'id': 'npm:declared-dev@1.0.0', 'ecosystem': 'npm', 'textStatus': 'missing-from-published-package'},
            {'id': 'asset:JetBrains Mono@2.304', 'ecosystem': 'asset', 'textStatus': 'collected'},
        ]}
        self.commands = []

    def runner(self, command, root):
        self.commands.append(command)
        if command[:2] == ['cargo', 'fetch']:
            self.assertEqual(command, ['cargo', 'fetch', '--locked'])
            return ''
        if command[:2] == ['cargo', '--version']:
            return 'cargo 1.95.0\n'
        if command[0] == 'rustc':
            return 'rustc 1.95.0\nhost: x86_64-unknown-linux-gnu\n'
        self.assertIn('--frozen', command)
        self.assertIn('--no-default-features', command)
        text = 'pulseterm-desktop v7.1.0 (/fixed/app)\nordinary v1.0.0\n'
        if command[command.index('--edges') + 1] == 'normal,build':
            text += 'compiler v1.0.0 (proc-macro)\n'
        return text

    def review(self, runner=None, validate=None):
        return m.review(self.root, runner or self.runner, validate or (lambda root: self.inventory))

    def test_normal_build_and_conservative_frontend_remain_distinct(self):
        report, raw = self.review()
        fetch = self.commands.index(['cargo', 'fetch', '--locked'])
        self.assertLess(fetch, next(i for i, command in enumerate(self.commands) if command[:2] == ['cargo', 'tree']))
        self.assertFalse(report['releaseApproved'])
        self.assertTrue(report['completed'])
        self.assertEqual(len(raw), 10)
        self.assertEqual(report['resolverHost'], 'x86_64-unknown-linux-gnu')
        for profile in report['profiles']:
            self.assertEqual(profile['normalNonProcMacroCargo'], ['cargo:ordinary@1.0.0'])
            self.assertEqual(profile['additionalBuildOrProcMacroCargo'], ['cargo:compiler@1.0.0'])
            self.assertIn('npm:declared-dev@1.0.0', profile['allSelectedComponents'])
            self.assertEqual(len(profile['incompleteTextComponents']), 2)
            if not profile['name'].startswith('linux'):
                self.assertNotIn('workspace-cli', profile['features'])
        self.assertEqual(report['profiles'][-1]['purpose'], 'comparison-only')

    def test_unknown_component_rejected(self):
        with self.assertRaisesRegex(ValueError, 'absent'):
            self.review(lambda command, root: self.runner(command, root) + ('unknown v1\n' if command[0:2] == ['cargo', 'tree'] else ''))

    def test_unrecognized_tree_not_silently_skipped(self):
        with self.assertRaisesRegex(ValueError, 'unrecognized'):
            m.parse_tree('invalid tree data\n', {}, set())

    def test_empty_and_missing_root_rejected(self):
        for text in ('', 'ordinary v1.0.0\n'):
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, 'lacks'):
                m.parse_tree(text, {'cargo:ordinary@1.0.0'}, {'cargo:pulseterm-desktop@7.1.0'})

    def test_repeated_and_proc_macro_annotations(self):
        text = 'pulseterm-desktop v7.1.0 (/fixed/app)\ncompiler v1.0.0 (proc-macro)\ncompiler v1.0.0 (*)\n'
        self.assertEqual(len(m.parse_tree(text, {'cargo:compiler@1.0.0'}, {'cargo:pulseterm-desktop@7.1.0'})), 2)

    def test_failed_process_preserves_native_error(self):
        command = [sys.executable, '-c', 'import sys; sys.stderr.write("missing locked crate\\n"); sys.exit(23)']
        with self.assertRaisesRegex(RuntimeError, 'missing locked crate') as caught:
            m.run(command, self.root)
        self.assertIsInstance(caught.exception.__cause__, subprocess.CalledProcessError)
        self.assertEqual(caught.exception.__cause__.returncode, 23)

    def test_failed_fetch_never_resolves_graphs(self):
        def fail(command, root):
            if command[:2] == ['cargo', 'fetch']:
                raise subprocess.CalledProcessError(23, command)
            return self.runner(command, root)
        with self.assertRaises(subprocess.CalledProcessError):
            self.review(fail)
        self.assertFalse(any(command[:2] == ['cargo', 'tree'] for command in self.commands))

    def test_failed_cargo_does_not_retry_or_finish(self):
        def fail(command, root):
            if command[0:2] == ['cargo', 'tree']:
                raise subprocess.CalledProcessError(23, command)
            return self.runner(command, root)
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.review(fail)
        self.assertEqual(caught.exception.returncode, 23)

    def test_changed_inputs_rejected(self):
        def mutate(command, root):
            if command[0:2] == ['cargo', 'tree']:
                (root / 'Cargo.lock').write_text('changed')
            return self.runner(command, root)
        with self.assertRaisesRegex(ValueError, 'inputs changed'):
            self.review(mutate)

    def test_normal_not_subset_of_combined_rejected(self):
        def inconsistent(command, root):
            text = self.runner(command, root)
            if command[0:2] == ['cargo', 'tree'] and command[command.index('--edges') + 1] == 'normal,build':
                return text.replace('ordinary v1.0.0\n', '')
            return text
        with self.assertRaisesRegex(ValueError, 'not contained'):
            self.review(inconsistent)

    def test_stale_or_falsely_approved_inventory_rejected(self):
        with self.assertRaisesRegex(ValueError, 'stale'):
            self.review(validate=lambda root: (_ for _ in ()).throw(ValueError('stale inventory')))
        self.inventory['releaseApproved'] = True
        with self.assertRaisesRegex(ValueError, 'falsely approved'):
            self.review()

    def test_unknown_ecosystem_rejected(self):
        self.inventory['packages'][-1]['ecosystem'] = 'unrecognized'
        with self.assertRaisesRegex(ValueError, 'ecosystem'):
            self.review()

    def test_duplicate_inventory_identity_rejected(self):
        self.inventory['packages'].append(self.inventory['packages'][0])
        with self.assertRaisesRegex(ValueError, 'invalid'):
            self.review()

    def test_main_replaces_stale_success_on_failure(self):
        output = self.root / 'target/probes/distribution-review.json'
        output.parent.mkdir(exist_ok=True, parents=True)
        output.write_text('{"completed":true}')
        output.with_suffix('.md').write_text('Old successful report')
        before = {k: os.environ.get(k) for k in ('TMPDIR', 'TMP', 'TEMP')}
        with patch.object(m, 'ROOT', self.root), patch.object(m, 'review', side_effect=ValueError('injected failure')):
            with self.assertRaisesRegex(ValueError, 'injected'):
                m.main()
        result = json.loads(output.read_bytes())
        self.assertFalse(result['completed'])
        self.assertFalse(result['releaseApproved'])
        self.assertIn('injected failure', result['failure'])
        self.assertNotIn('Old successful', output.with_suffix('.md').read_text())
        self.assertEqual(before, {k: os.environ.get(k) for k in before})


if __name__ == '__main__':
    unittest.main()
