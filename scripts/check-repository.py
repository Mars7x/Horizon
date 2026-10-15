#!/usr/bin/env python3
"""Offline structural checks; these do not compile Rust or validate runtime behavior."""

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


def markdown_links(text):
    """Read inline/reference links outside fenced code, retaining local fragments."""
    lines = []
    fence = None
    for line in text.splitlines():
        marker = re.match(r'^\s{0,3}(`{3,}|~{3,})', line)
        if marker:
            token = marker[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            continue
        if fence is None:
            lines.append(line)
    body = '\n'.join(lines)
    inline = re.findall(r'!?\[[^\]\n]*\]\(\s*(<[^>\n]+>|[^\s)]+)(?:\s+["\'][^\n]*?["\'])?\s*\)', body)
    references = re.findall(r'^\s{0,3}\[[^\]]+\]:\s*(<[^>\n]+>|\S+)', body, re.M)
    return [target.strip('<>') for target in inline + references]


def anchors(text):
    result = set(re.findall(r'<a\s+(?:id|name)=["\']([^"\']+)', text))
    counts = {}
    for heading in re.findall(r'^#{1,6}\s+(.+?)\s*#*$', text, re.M):
        slug = re.sub(r'[^\w\-\s]', '', heading.lower()).replace(' ', '-')
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        result.add(f'{slug}-{count}' if count else slug)
    return result


def check_links(root, documents):
    errors = []
    for document in documents:
        path = root / document
        for target in markdown_links(path.read_text()):
            parsed = urlsplit(target)
            if parsed.scheme or parsed.netloc:
                continue
            destination = (path.parent / unquote(parsed.path)).resolve() if parsed.path else path.resolve()
            if not destination.is_relative_to(root.resolve()):
                errors.append(f'{document}: local link escapes repository: {target}')
                continue
            relative = destination.relative_to(root.resolve()).as_posix()
            if not destination.exists():
                errors.append(f'{document}: missing local link: {target}')
                continue
            if relative.startswith('docs/history/') and relative != 'docs/history/INDEX.md':
                errors.append(f'{document}: use history/INDEX.md rather than archived instructions: {target}')
            if parsed.fragment and destination.suffix == '.md':
                if unquote(parsed.fragment) not in anchors(destination.read_text()):
                    errors.append(f'{document}: missing heading: {target}')
    return errors


def check_index(root, directory, index):
    linked = set()
    for target in markdown_links((root / index).read_text()):
        parsed = urlsplit(target)
        if not parsed.scheme and parsed.path:
            linked.add(((root / index).parent / unquote(parsed.path)).resolve())
    return [f'{path.relative_to(root)}: current document missing from {index}'
            for path in sorted((root / directory).glob('*.md'))
            if path != root / index and path.resolve() not in linked]


def check_migrations(root, protected):
    directory = root / 'src/persistence/migrations'
    errors = []
    for name, checksum in protected.items():
        path = directory / name
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != checksum:
            errors.append(f'immutable migration changed or missing: {name}')
    paths = sorted(directory.glob('*.sql'))
    numbers = [int(p.name.split('_', 1)[0]) for p in paths]
    if numbers != list(range(1, len(paths) + 1)):
        errors.append('migration filenames must be contiguous from 0001')
    source = (root / 'src/persistence/migrations.rs').read_text()
    match = re.search(r'LATEST_SCHEMA_VERSION:\s*i64\s*=\s*(\d+)', source)
    latest = int(match[1]) if match else None
    if latest != len(paths):
        errors.append('LATEST_SCHEMA_VERSION does not match migration files')
    includes = re.findall(r'include_str!\("migrations/([^"\n]+)"\)', source.split('#[cfg(test)]')[0])
    if includes != [p.name for p in paths]:
        errors.append('migration registry does not match ordered SQL files')
    return errors, latest


def check_lockfile(manifest, lock):
    packages = lock['package']
    errors = []
    for package in packages:
        for reference in package.get('dependencies', []):
            parts = reference.split()
            matches = [p for p in packages if p['name'] == parts[0]
                       and (len(parts) < 2 or p['version'] == parts[1])
                       and (len(parts) < 3 or p.get('source') == ' '.join(parts[2:]).strip('()'))]
            if len(matches) != 1:
                errors.append(f'Cargo.lock: unresolved/ambiguous dependency: {reference}')
    project = next((p for p in packages if p['name'] == manifest['package']['name']), None)
    if project is None:
        return errors + ['Cargo.lock: project package missing']
    declared = {}
    for section in ('dependencies', 'build-dependencies', 'dev-dependencies'):
        declared.update(manifest.get(section, {}))
    wanted = {value.get('package', name) if isinstance(value, dict) else name
              for name, value in declared.items()}
    actual = {d.split()[0] for d in project.get('dependencies', [])}
    if actual != wanted:
        errors.append('Cargo.lock: project dependencies differ from Cargo.toml')
    if project['version'] != manifest['package']['version']:
        errors.append('Cargo.lock: project version differs from Cargo.toml')
    return errors


def check_dependency_usage(manifest, source, exceptions):
    """Catch obvious unused direct crates; Cargo still owns semantic resolution."""
    errors = []
    declared = {}
    for section in ('dependencies', 'build-dependencies', 'dev-dependencies'):
        declared.update(manifest.get(section, {}))
    for alias in declared:
        crate = alias.replace('-', '_')
        if not re.search(r'\b' + re.escape(crate) + r'\s*(?:::|;)', source):
            if not exceptions.get(alias):
                errors.append(f'no Rust/build consumer for direct dependency: {alias}')
    for alias, reason in exceptions.items():
        if alias not in declared or not isinstance(reason, str) or not reason.strip():
            errors.append(f'invalid dependency-usage exemption: {alias}')
    return errors


def finish_args(text):
    section = text.split('finish-args:', 1)[1].split('\nmodules:', 1)[0]
    return re.findall(r'^\s+-\s+(--\S+)\s*$', section, re.M)


def check_permissions(manifest, approved):
    actual = finish_args(manifest)
    return [] if sorted(actual) == sorted(approved) else [
        'Flatpak finish-args differ from the reviewed sandbox policy in repository-contracts.json']


def check_modules(root, files):
    errors = []
    for relative in files:
        path = root / relative
        if path.suffix != '.rs' or not path.is_file() or not relative.startswith('src/'):
            continue
        parent = path.parent if path.stem in ('lib', 'main', 'mod') else path.with_suffix('')
        for module in re.findall(r'^(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;', path.read_text(), re.M):
            if not (parent / f'{module}.rs').is_file() and not (parent / module / 'mod.rs').is_file():
                errors.append(f'{relative}: missing Rust module {module}')
    return errors


def check_file_hygiene(files):
    forbidden_parts = {'target', 'build-dir', '.flatpak-builder', '__pycache__', '.pytest_cache'}
    runtime_files = {'.env', 'steam-account.json', 'steamgriddb.json', '.DS_Store'}
    return [f'generated/runtime file must not be committed: {name}' for name in files
            if forbidden_parts.intersection(Path(name).parts)
            or Path(name).name in runtime_files or name.endswith(('.pyc', '.pyo', '.flatpak'))]


def check_repository(root):
    errors = []
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    contracts = json.loads((root / 'scripts/repository-contracts.json').read_text())
    packaging = (root / 'flatpak/io.github.Mars7x.Horizon.yml').read_text()
    # Include untracked non-ignored source, but never walk potentially huge build trees.
    result = subprocess.run(['git', '-C', str(root), 'ls-files', '-z', '--cached', '--others',
                             '--exclude-standard'], check=True, capture_output=True)
    files = sorted(set(filter(None, result.stdout.decode().split('\0'))))
    existing = [name for name in files if (root / name).is_file()]
    current = [name for name in existing if name.endswith('.md')
               and not name.startswith('docs/history/')]
    errors.extend(check_links(root, current))
    errors.extend(check_index(root, 'docs', 'docs/INDEX.md'))
    errors.extend(check_index(root, 'docs/adr', 'docs/adr/INDEX.md'))
    for path in sorted((root / 'docs').glob('*.md')):
        if re.search(r'^#{1,6}\s+.*\bPhase\s+\d', path.read_text(), re.M | re.I):
            errors.append(f'{path.relative_to(root)}: phase history belongs under docs/history')
    if (root / 'README.md').read_bytes():
        errors.append('README.md must remain blank')
    for name in contracts['retired_dependencies']:
        if name in manifest['dependencies']:
            errors.append(f'retired direct dependency reintroduced: {name}')
    for name in contracts['retired_files']:
        if (root / name).exists():
            errors.append(f'retired implementation reintroduced: {name}')
    for name in existing:
        if name.startswith('src/') and name.endswith('.rs'):
            for token in contracts['retired_source_tokens']:
                if token in (root / name).read_text():
                    errors.append(f'{name}: retired Steam/compositor integration: {token}')
    errors.extend(check_file_hygiene(files))
    errors.extend(check_modules(root, existing))
    source_text = '\n'.join((root / name).read_text() for name in existing
                            if name.endswith('.rs') and (name.startswith(('src/', 'tests/')) or name == 'build.rs'))
    errors.extend(check_dependency_usage(manifest, source_text, contracts['dependency_usage_exemptions']))
    errors.extend(check_lockfile(manifest, tomllib.loads((root / 'Cargo.lock').read_text())))
    errors.extend(check_permissions(packaging, contracts['flatpak_finish_args']))
    migration_errors, latest = check_migrations(root, contracts['immutable_migrations'])
    errors.extend(migration_errors)
    runtime = re.search(r"^runtime-version:\s*['\"]?([^'\"\s]+)", packaging, re.M)[1]
    development = (root / 'docs/DEVELOPMENT.md').read_text()
    for label, value in [('Minimum Rust', manifest['package']['rust-version']),
                         ('Rust edition', manifest['package']['edition']),
                         ('Flatpak runtime', runtime)]:
        if f'{label}: `{value}`' not in development:
            errors.append(f'DEVELOPMENT.md: {label} differs from manifest')
    if f'Latest schema: `{latest}`' not in (root / 'docs/DATABASE.md').read_text():
        errors.append('DATABASE.md: schema version differs from source')
    navigation = (root / 'src/navigation/mod.rs').read_text()
    utility_enum = re.search(r'pub enum UtilityPage\s*\{([^}]+)\}', navigation)[1]
    utilities = re.findall(r'^\s+([A-Z]\w*),', utility_enum, re.M)
    documented = ', '.join(utilities)
    if f'Utilities are **{documented}**' not in (root / 'docs/PROJECT_STATE.md').read_text():
        errors.append('PROJECT_STATE.md: utility order differs from Rust')
    annotations = tomllib.loads((root / 'REUSE.toml').read_text())['annotations']
    for annotation in annotations:
        paths = annotation['path']
        for pattern in paths if isinstance(paths, list) else [paths]:
            if not any(ch in pattern for ch in '*?[') and not (root / pattern).exists():
                errors.append(f'REUSE.toml: missing explicitly annotated path: {pattern}')
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    args = parser.parse_args()
    try:
        errors = check_repository(args.root.resolve())
    except (OSError, ValueError, KeyError, IndexError, subprocess.SubprocessError) as error:
        print(f'Repository checks could not complete: {error}', file=sys.stderr)
        return 1
    if errors:
        print('\n'.join(f'ERROR: {error}' for error in errors), file=sys.stderr)
        return 1
    print('Repository structure, current documentation and contract checks passed')
    return 0


if __name__ == '__main__':
    sys.exit(main())
