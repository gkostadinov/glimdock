#!/usr/bin/env python3
"""Create a macOS app identity for an unprivileged launchd device agent.

The bundle contains only the executable and public metadata. Configuration and
paired credentials stay in the user's existing private directory.
"""
import argparse, pathlib, plistlib, shutil, subprocess, tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=pathlib.Path, required=True)
parser.add_argument('--output', type=pathlib.Path, required=True)
parser.add_argument('--sign-identity', default='-', help='Apple-issued signing identity, or - for a local ad-hoc build')
args = parser.parse_args()
assert args.binary.is_file() and not args.binary.is_symlink()
assert args.output.suffix == '.app' and not args.output.exists(), 'Use a new bundle output directory'
version = tomllib.loads((ROOT/'Cargo.toml').read_text())['workspace']['package']['version']
contents = args.output/'Contents'
(contents/'MacOS').mkdir(parents=True)
executable = contents/'MacOS/glimdock-agent'
shutil.copy2(args.binary, executable)
executable.chmod(0o755)
info = {'CFBundleIdentifier':'com.glimdock.agent', 'CFBundleName':'Glimdock Agent',
        'CFBundleDisplayName':'Glimdock Agent', 'CFBundleExecutable':'glimdock-agent',
        'CFBundlePackageType':'APPL', 'CFBundleShortVersionString':version,
        'CFBundleVersion':version, 'LSUIElement':True,
        'NSLocalNetworkUsageDescription':'Send read-only CPU, memory and device readings to the Glimdock collector you paired on your local network.'}
(contents/'Info.plist').write_bytes(plistlib.dumps(info))
subprocess.run(['codesign','--force','--sign',args.sign_identity,'--identifier','com.glimdock.agent','--options','runtime',str(args.output)],check=True)
subprocess.run(['codesign','--verify','--deep','--strict',str(args.output)],check=True)
print(f'Created Glimdock Agent {version} bundle; private state was not copied.')
