#!/usr/bin/env python3
"""Regression tests for repository guardrails, using isolated temporary files."""

import hashlib
import importlib.util
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location('repository_checks', Path(__file__).with_name('check-repository.py'))
CHECKS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKS)


class RepositoryChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def test_links_skip_examples_and_allow_external_urls(self):
        self.write('docs/a.md', '# Guide\n\n[HTTP](https://example.org/missing)\n```sh\n[x](missing)\n```\n')
        self.assertEqual(CHECKS.check_links(self.root, ['docs/a.md']), [])

    def test_missing_links_and_heading_fragments_fail(self):
        self.write('docs/a.md', '[bad](missing.md)\n[heading](b.md#missing)\n[ok](b.md#title)\n')
        self.write('docs/b.md', '# Title\n')
        errors = CHECKS.check_links(self.root, ['docs/a.md'])
        self.assertEqual(len(errors), 2)
        self.assertTrue(any('missing local link' in error for error in errors))
        self.assertTrue(any('missing heading' in error for error in errors))

    def test_reference_links_are_validated(self):
        self.write('docs/a.md', '[link][ref]\n\n[ref]: missing.md\n')
        self.assertEqual(len(CHECKS.check_links(self.root, ['docs/a.md'])), 1)

    def test_history_is_entered_through_its_index(self):
        self.write('docs/a.md', '[bad](history/old.md)\n[ok](history/INDEX.md)\n')
        self.write('docs/history/old.md', '# Old\n')
        self.write('docs/history/INDEX.md', '# History\n')
        self.assertEqual(len(CHECKS.check_links(self.root, ['docs/a.md'])), 1)

    def test_links_cannot_escape_repository(self):
        self.write('docs/a.md', '[escape](../../outside.md)\n')
        errors = CHECKS.check_links(self.root, ['docs/a.md'])
        self.assertIn('escapes repository', errors[0])

    def test_unindexed_guide_fails(self):
        self.write('docs/INDEX.md', '[One](one.md)\n')
        self.write('docs/one.md', '# One\n')
        self.write('docs/two.md', '# Two\n')
        errors = CHECKS.check_index(self.root, 'docs', 'docs/INDEX.md')
        self.assertEqual(len(errors), 1)
        self.assertIn('two.md', errors[0])

    def test_migration_rewrite_fails_but_append_is_allowed(self):
        first = self.write('src/persistence/migrations/0001_initial.sql', 'SELECT 1;\n')
        protected = {first.name: hashlib.sha256(first.read_bytes()).hexdigest()}
        self.write('src/persistence/migrations.rs', 'const LATEST_SCHEMA_VERSION: i64 = 1;\ninclude_str!("migrations/0001_initial.sql");\n')
        self.assertEqual(CHECKS.check_migrations(self.root, protected), ([], 1))
        first.write_text('SELECT 2;\n')
        self.assertTrue(CHECKS.check_migrations(self.root, protected)[0])
        first.write_text('SELECT 1;\n')
        self.write('src/persistence/migrations/0002_next.sql', 'SELECT 2;\n')
        self.write('src/persistence/migrations.rs', 'const LATEST_SCHEMA_VERSION: i64 = 2;\ninclude_str!("migrations/0001_initial.sql");\ninclude_str!("migrations/0002_next.sql");\n')
        self.assertEqual(CHECKS.check_migrations(self.root, protected), ([], 2))

    def test_schema_drift_and_migration_gap_fail(self):
        self.write('src/persistence/migrations/0001_initial.sql', 'SELECT 1;\n')
        self.write('src/persistence/migrations/0003_gap.sql', 'SELECT 3;\n')
        self.write('src/persistence/migrations.rs', 'const LATEST_SCHEMA_VERSION: i64 = 1;\n')
        errors, _ = CHECKS.check_migrations(self.root, {})
        self.assertEqual(len(errors), 3)

    def test_extra_permission_requires_policy_review(self):
        manifest = 'finish-args:\n  - --share=network\nmodules:\n'
        self.assertEqual(CHECKS.check_permissions(manifest, ['--share=network']), [])
        broadened = manifest.replace('modules:', '  - --filesystem=host\nmodules:')
        self.assertTrue(CHECKS.check_permissions(broadened, ['--share=network']))

    def test_lockfile_checks_missing_and_ambiguous_references(self):
        manifest = {'package': {'name': 'demo', 'version': '1'}, 'dependencies': {'dep': '1'}}
        lock = {'package': [{'name': 'demo', 'version': '1', 'dependencies': ['dep']},
                            {'name': 'dep', 'version': '1'}]}
        self.assertEqual(CHECKS.check_lockfile(manifest, lock), [])
        lock['package'].append({'name': 'dep', 'version': '2'})
        self.assertTrue(CHECKS.check_lockfile(manifest, lock))
        lock['package'] = lock['package'][:1]
        self.assertTrue(CHECKS.check_lockfile(manifest, lock))

    def test_dependency_aliases_and_documented_feature_only_exceptions(self):
        manifest = {'dependencies': {'alias': {'package': 'upstream'}, 'feature-only': '1'}}
        errors = CHECKS.check_dependency_usage(manifest, 'use alias::Type;', {})
        self.assertEqual(len(errors), 1)
        self.assertEqual(CHECKS.check_dependency_usage(manifest, 'use alias::Type;',
                         {'feature-only': 'Enables required upstream feature unification'}), [])
        self.assertTrue(CHECKS.check_dependency_usage(manifest, 'use alias::Type;', {'absent': 'typo'}))

    def test_deleted_rust_module_is_caught(self):
        self.write('src/lib.rs', 'pub mod adapter;\n')
        self.assertTrue(CHECKS.check_modules(self.root, ['src/lib.rs']))
        self.write('src/adapter.rs', '')
        self.assertEqual(CHECKS.check_modules(self.root, ['src/lib.rs']), [])

    def test_iterator_zip_does_not_count_as_a_crate_consumer(self):
        manifest = {'dependencies': {'zip': '8'}}
        self.assertTrue(CHECKS.check_dependency_usage(manifest, 'a.zip(b)', {}))
        self.assertEqual(CHECKS.check_dependency_usage(manifest, 'use zip::ZipArchive;', {}), [])

    def test_source_generation_preserves_lockfile_and_validates_before_generation(self):
        script = Path(__file__).with_name('update-flatpak-sources.sh')
        helper = self.write('scripts/update-flatpak-sources.sh', script.read_text())
        lock = self.write('Cargo.lock', '# fixed lockfile\n')
        log = self.root / 'cargo-call.txt'
        cargo = self.write('bin/cargo', '#!/bin/sh\nprintf "%s" "$*" > "$HORIZON_TEST_CARGO_LOG"\n')
        cargo.chmod(0o755)
        generator = self.write('generator.py',
                               'import pathlib, sys\npathlib.Path(sys.argv[-1]).write_text("[]")\n')
        (self.root / 'flatpak').mkdir()
        environment = dict(os.environ, PATH=str(cargo.parent) + os.pathsep + os.environ['PATH'],
                           HORIZON_TEST_CARGO_LOG=str(log), FLATPAK_CARGO_GENERATOR=str(generator))
        subprocess.run(['bash', str(helper)], env=environment, check=True, capture_output=True)
        self.assertEqual(log.read_text(), 'metadata --locked --no-deps --format-version 1')
        self.assertEqual(lock.read_text(), '# fixed lockfile\n')
        output = self.root / 'flatpak/cargo-sources.json'
        self.assertTrue(output.exists())
        output.unlink()
        cargo.write_text('#!/bin/sh\nexit 42\n')
        result = subprocess.run(['bash', str(helper)], env=environment, capture_output=True)
        self.assertEqual(result.returncode, 42)
        self.assertFalse(output.exists())

    def test_generated_files_and_runtime_credentials_are_rejected(self):
        files = ['target/a', 'cache/__pycache__/a.pyc', 'config/steam-account.json', 'src/lib.rs']
        self.assertEqual(len(CHECKS.check_file_hygiene(files)), 3)


if __name__ == '__main__':
    unittest.main(verbosity=2)
