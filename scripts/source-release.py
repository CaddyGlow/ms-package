#!/usr/bin/env python3
"""Create and verify a self-contained, offline-buildable source release."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

REPOS = ('ms-package', 'archive-rs', 'cabinet', 'ms-compress', 'wim-rs', 'mkiso-rs', 'windows-uup')
TOOLCHAIN = '1.99.0'


def run(args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verify(root):
    recorded = json.loads((root / 'SOURCE-MANIFEST.json').read_text())
    actual = {str(p.relative_to(root)): digest(p) for p in root.rglob('*')
              if p.is_file() and p != root / 'SOURCE-MANIFEST.json'}
    if actual != recorded['files']:
        raise SystemExit('Source manifest verification failed')
    with tempfile.TemporaryDirectory(prefix='ms-package-cargo-home-') as cargo_home:
        env = dict(os.environ, CARGO_HOME=cargo_home, RUSTC_WRAPPER='', CARGO_NET_OFFLINE='true')
        # A separate target proves that verification compiles the extracted sources.
        with tempfile.TemporaryDirectory(prefix='ms-package-release-target-') as target:
            env['CARGO_TARGET_DIR'] = target
            run(['cargo', f'+{TOOLCHAIN}', 'test', '--manifest-path', str(root / 'ms-package/Cargo.toml'),
                 '--all-targets', '--all-features', '--frozen'], cwd=root, env=env)
            run(['cargo', f'+{TOOLCHAIN}', 'test', '--manifest-path', str(root / 'ms-package/Cargo.toml'),
                 '--doc', '--frozen'], cwd=root, env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--allow-dirty', action='store_true', help='Prepare a local preview, never an official release')
    parser.add_argument('--verify', type=Path, help='Verify an existing archive and build offline from its sources')
    args = parser.parse_args()
    if args.verify:
        with tempfile.TemporaryDirectory(prefix='ms-package-extracted-') as temporary:
            with tarfile.open(args.verify) as archive:
                archive.extractall(temporary, filter='data')
            roots = list(Path(temporary).iterdir())
            if len(roots) != 1 or not roots[0].is_dir():
                raise SystemExit('Expected exactly one source directory')
            verify(roots[0])
        return
    if not args.output:
        parser.error('--output is required when creating an archive')
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        raise SystemExit(f'Refusing to overwrite {output}')
    parent = Path(__file__).resolve().parents[2]
    import tomllib
    version = tomllib.loads((parent / 'ms-package/Cargo.toml').read_text())['package']['version']
    with tempfile.TemporaryDirectory(prefix='ms-package-sources-') as temporary:
        root = Path(temporary) / f'ms-package-{version}-source'
        root.mkdir()
        commits = {}
        for name in REPOS:
            source = parent / name
            dirty = run(['git', '-C', str(source), 'status', '--porcelain'], capture_output=True).stdout
            head = subprocess.run(['git', '-C', str(source), 'rev-parse', '--verify', 'HEAD'], text=True, capture_output=True)
            if not args.allow_dirty and (dirty or head.returncode):
                raise SystemExit(f'{name} needs a clean committed source tree for an official release')
            commits[name] = {'commit': head.stdout.strip() if head.returncode == 0 else None,
                             'dirty': bool(dirty)}
            paths = run(['git', '-C', str(source), 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], capture_output=True).stdout
            for relative in sorted(set(paths.split('\0')) - {''}):
                if 'target' in Path(relative).parts or '__pycache__' in Path(relative).parts:
                    continue
                item = source / relative
                if not item.exists():
                    continue
                if item.is_symlink():
                    raise SystemExit(f'Source symlink needs review: {item}')
                if not item.is_file():
                    continue
                target = root / name / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(item, target)
        vendor = run(['cargo', f'+{TOOLCHAIN}', 'vendor', '--locked', '--versioned-dirs',
                      '--manifest-path', str(root / 'ms-package/Cargo.toml'),
                      '--sync', str(root / 'archive-rs/Cargo.toml'),
                      '--sync', str(root / 'archive-rs/fuzz/Cargo.toml'), str(root / 'vendor')],
                     capture_output=True)
        (root / '.cargo').mkdir()
        config = vendor.stdout.replace(str(root / 'vendor'), 'vendor')
        (root / '.cargo/config.toml').write_text(config)
        (root / 'BUILD.md').write_text(
            '# Offline source release\n\nInstall Rust 1.99.0 and a native C/C++ toolchain.\n'
            'From this directory, run:\n\n'
            '```sh\ncargo +1.99.0 test --manifest-path ms-package/Cargo.toml --all-targets --all-features --frozen\n```\n\n'
            'Registry and Git dependencies are included in vendor/ with their licenses.\n'
            'SOURCE-MANIFEST.json records every file hash and source revision.\n')
        files = {str(p.relative_to(root)): digest(p) for p in root.rglob('*') if p.is_file()}
        (root / 'SOURCE-MANIFEST.json').write_text(json.dumps(
            {'version': version, 'toolchain': TOOLCHAIN, 'preview': args.allow_dirty,
             'repositories': commits, 'files': files}, indent=2, sort_keys=True) + '\n')
        def normalized(info):
            info.mtime = 0
            info.uid = info.gid = 0
            info.uname = info.gname = ''
            info.mode = 0o755 if info.isdir() or info.mode & 0o111 else 0o644
            return info

        with output.open('wb') as raw:
            with gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as archive:
                    archive.add(root, arcname=root.name, filter=normalized)
    print(f'{digest(output)}  {output.name}')


if __name__ == '__main__':
    main()
