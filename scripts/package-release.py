#!/usr/bin/env python3
"""Package an already-built native release executable without user data."""
import argparse
import hashlib
import pathlib
import platform
import plistlib
import re
import shutil
import subprocess
import tarfile
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expected-platform')
    parser.add_argument('--version', action='store_true', help='Print the Cargo package version')
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parent.parent
    package = (root / 'Cargo.toml').read_text().split('[package]', 1)[1].split('\n[', 1)[0]
    version = re.search(r'^version\s*=\s*"([^"]+)"', package, re.MULTILINE).group(1)
    if args.version:
        print(version)
        return
    if not args.expected_platform:
        parser.error('--expected-platform is required when packaging')
    system = platform.system()
    machine = platform.machine().lower()
    architecture = {'amd64': 'x86_64', 'aarch64': 'arm64'}.get(machine, machine)
    operating_system = {'Darwin': 'macos', 'Linux': 'linux', 'Windows': 'windows'}[system]
    native = f'{operating_system}-{architecture}'
    if native != args.expected_platform:
        raise SystemExit(f'Expected {args.expected_platform}, but this runner is {native}')
    output = root / 'target/release-dist'
    output.mkdir(parents=True, exist_ok=True)
    name = f'Nano-Music-Player-{version}-{native}'
    staging = root / 'target/release-staging' / name
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir(parents=True)

    if system == 'Darwin':
        subprocess.run(['sh', 'scripts/package-macos.sh'], cwd=root, check=True)
        bundle = staging / 'Nano Music Player.app'
        shutil.copytree(root / 'target/Nano Music Player.app', bundle, symlinks=True)
        info_path = bundle / 'Contents/Info.plist'
        info = plistlib.loads(info_path.read_bytes())
        info['CFBundleShortVersionString'] = version
        info_path.write_bytes(plistlib.dumps(info))
        licenses = bundle / 'Contents/Resources/licenses'
    else:
        executable = 'nano-music-player.exe' if system == 'Windows' else 'nano-music-player'
        shutil.copy2(root / 'target/release' / executable, staging / executable)
        licenses = staging / 'licenses'
    shutil.copytree(root / 'licenses', licenses, dirs_exist_ok=True)
    for font in ('maple-mono', 'jetbrains-mono'):
        destination = licenses / font
        destination.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / 'ui/fonts' / font / 'LICENSE.txt', destination / 'LICENSE.txt')
    shutil.copy2(root / 'README.md', staging / 'README.md')
    shutil.copy2(root / 'docs/releases' / f'v{version}.md', staging / 'RELEASE-NOTES.md')

    if system == 'Darwin':
        subprocess.run(['codesign', '--force', '--deep', '--sign', '-', str(bundle)], check=True)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(bundle)], check=True)
    if system == 'Linux':
        archive = output / f'{name}.tar.gz'
        with tarfile.open(archive, 'w:gz') as package:
            package.add(staging, arcname=name)
    else:
        archive = output / f'{name}.zip'
        if system == 'Darwin':
            subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(staging), str(archive)], check=True)
        else:
            with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as package:
                for path in sorted(staging.rglob('*')):
                    package.write(path, path.relative_to(staging.parent))
    digest = hashlib.sha256()
    with archive.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    checksum = digest.hexdigest()
    (output / f'{archive.name}.sha256').write_bytes(f'{checksum}  {archive.name}\n'.encode('utf-8'))
    print(f'Packaged {archive.name}: {checksum}')


if __name__ == '__main__':
    main()
