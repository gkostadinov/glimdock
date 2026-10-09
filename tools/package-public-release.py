#!/usr/bin/env python3
"""Export the current Rust/console/firmware source tree with audited public assets.

The allowlist excludes private state, live telemetry, legacy runtimes, build caches,
manufacturer geometry and personal slicer projects. Only the independently built
credential-free public firmware images are included, with their source/relink bundle.
"""
from __future__ import annotations
import argparse, ast, hashlib, io, ipaddress, json, os, re, shutil, tarfile, tempfile, tomllib
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
FILES = (
    'Cargo.toml','Cargo.lock','.gitignore','README.md','SETUP.md','BUILDING.md','CONTRIBUTING.md','SECURITY.md','LICENSE','LICENSE_POLICY.md','THIRD_PARTY_NOTICES.md',
    '.github/workflows/build.yml','.github/ISSUE_TEMPLATE/bug-report.yml','.github/ISSUE_TEMPLATE/feature-request.yml',
    '.github/ISSUE_TEMPLATE/config.yml','.github/pull_request_template.md','docs/architecture.md','docs/DRIVER_PROVENANCE.md',
    'collector-rs/Cargo.toml','collector-rs/build.rs','collector-rs/helpers/qga-bridge.pl','collector-rs/docs/TELEMETRY.md',
    'device-agent-rs/Cargo.toml','firmware/platformio.ini','firmware/partitions.csv','firmware/build_and_run.sh','firmware/README.md',
    'firmware/include/config.example.h','firmware/include/lv_conf.h','firmware/include/lv_psram.h',
    'collector-web/index.html','collector-web/package.json','collector-web/package-lock.json',
    'deploy/install.sh','deploy/install-rust.sh','deploy/config.example.json',
    'deploy/homelab-monitor-config.service','deploy/homelab-monitor-collector.service','deploy/homelab-monitor-http.service',
    'deploy/rust/homelab-monitor-config.service','deploy/rust/homelab-monitor-collector.service','deploy/rust/homelab-monitor-http.service',
    'tools/build-release.sh','tools/build-web-firmware.py','tools/package-public-release.py','tools/package-binaries.py',
    'tools/cargo-notices.py','tools/test-pairing-policy.py','tools/configure_truenas_ssh.py',
    'integrations/truenas/probe.py','integrations/truenas/README.md',
    'examples/README.md','examples/collector.json','examples/nodes/proxmox.json',
    'examples/agents/host.json','examples/agents/snmp.json','examples/agents/snmp-credentials.json','examples/agents/json.json',
    'examples/snapshots/host.json','examples/snapshots/all-nodes.json','examples/snapshots/firmware-inventory.json',
    'enclosure/README.md','enclosure/requirements.txt',
    'enclosure/pebble-landscape/build.py','enclosure/pebble-landscape/design.json','enclosure/pebble-landscape/base_style.py',
    'enclosure/pebble-landscape/landscape_mechanics.py','enclosure/pebble-landscape/accessories.py',
    'enclosure/pebble-landscape/README.md','enclosure/pebble-landscape/ACCESSORIES.md',
)
# Dedicated source/public-asset directories only. A symlink or unexpected suffix fails the export.
TREES = {
    'collector-rs/src':{'.rs'}, 'collector-rs/tests':{'.rs'},
    'device-agent-rs/src':{'.rs'}, 'device-agent-rs/tests':{'.rs'},
    'firmware/src':{'.h','.cpp'}, 'public-docs':{'.md'},
    'collector-web/src':{'.js'}, 'collector-web/tests':{'.js'},
    'collector-web/assets':{'.js','.css','.txt','.svg','.woff2'},
    'collector-web/emulator':{'.js','.json','.wasm','.txt'},
    'collector-web/firmware':{'.json','.bin','.gz'},
    'branding':{'.svg','.txt','.md','.py','.woff2'}, 'LICENSES':{'.txt','.json','.md'},
    'tools/native-preview':{'.py','.cpp','.h','.md','.txt'},
    'tools/web-emulator':{'.py','.cpp','.h','.md','.mjs','.txt'},
}
IPV4 = re.compile(r'(?<![\d.])(?:\d{1,3}\.){3}\d{1,3}(?![\d.])')
PERSONAL = re.compile(r'/(?:Users|home)/[^\s\"\'`<>]+')
BINARY = {'.woff2','.wasm','.bin','.gz'}
# Emscripten's browser libc deliberately exposes this synthetic home directory.
VIRTUAL_PATHS = {'/home/web_user'}
ASSET_TREES = ('collector-web/assets/', 'collector-web/emulator/', 'collector-web/firmware/')
def private_address(value):
    try: address=ipaddress.ip_address(value)
    except ValueError: return False
    # Integer network bases keep the exporter unchanged by its own redaction.
    networks=((0x0a000000,8),(0xac100000,12),(0xc0a80000,16))
    return any(address in ipaddress.ip_network(n) for n in networks) and value!='192.168.4.1'
def private_values():
    values=[]
    pattern=re.compile(r'^\s*#define\s+HOMELAB_DEFAULT_(?:SSID|PASSWORD|TOKEN|SETUP_TOKEN|ENDPOINT)\s+("(?:[^"\\]|\\.)*")',re.M)
    for name in ['local_credentials.h','private_defaults.h','secrets.h']:
        p=ROOT/'firmware/include'/name
        if p.exists():
            values.extend(ast.literal_eval(m.group(1)).encode() for m in pattern.finditer(p.read_text()) if ast.literal_eval(m.group(1)))
    return values

def candidate():
    selected=list(FILES)
    for folder, suffixes in TREES.items():
        for p in sorted((ROOT/folder).rglob('*')):
            if '__pycache__' in p.parts or p.name.startswith('.'): continue
            if p.is_symlink(): raise ValueError(f'Symlink rejected in {folder}')
            if p.is_file():
                if p.suffix not in suffixes: raise ValueError(f'Unexpected public source type: {p.relative_to(ROOT)}')
                selected.append(p.relative_to(ROOT).as_posix())
    sources={}; counts={'private_addresses_replaced':0,'personal_paths_replaced':0}
    for name in dict.fromkeys(selected):
        p=ROOT/name
        if not p.is_file() or p.is_symlink(): raise ValueError(f'Required public source missing: {name}')
        data=p.read_bytes()
        if p.suffix not in BINARY:
            text=data.decode('utf8')
            def replace_address(m):
                if not private_address(m.group()): return m.group()
                counts['private_addresses_replaced']+=1
                return '192.0.2.10'
            if name.startswith(ASSET_TREES):
                # Generated assets must retain their exact manifest-bound bytes.
                if any(private_address(m.group()) for m in IPV4.finditer(text)):
                    raise ValueError(f'Private address rejected in public asset: {name}')
                if any(m.group() not in VIRTUAL_PATHS for m in PERSONAL.finditer(text)):
                    raise ValueError(f'Personal path rejected in public asset: {name}')
            else:
                text=IPV4.sub(replace_address,text)
                def replace_personal(m):
                    if m.group() in VIRTUAL_PATHS: return m.group()
                    counts['personal_paths_replaced']+=1
                    return '/EXTERNAL/REFERENCE'
                text=PERSONAL.sub(replace_personal,text)
            data=text.encode()
        sources[name]=data
    sources['firmware/include/config.h']=sources['firmware/include/config.example.h']
    # Public summaries replace private prototype records and their local links.
    sources['docs/validation.md']=sources['public-docs/VALIDATION.md']
    sources['enclosure/pebble-landscape/README.md']=sources['public-docs/ENCLOSURE.md']
    sources['enclosure/reference/README.md']=b'''# Manufacturer fitting reference\n\nThe current Pebble Landscape CAD expects the V1 manufacturer STEP at\n`esp32-s3-touch-lcd-2_8.stp` here. Obtain it from the official Waveshare\nresources; manufacturer geometry, personal fit files and Orca projects are\nnot redistributed. The authored design parameters are in\n`enclosure/pebble-landscape/design.json`. Nominal CAD checks do not establish\nphysical fit. The current main design is USB-only.\n'''
    literals=private_values()
    for name,data in sources.items():
        if any(value in data for value in literals): raise ValueError(f'Private build default rejected in {name}')
        if re.search(rb'-----BEGIN (?:OPENSSH |RSA |EC |DSA )?PRIVATE KEY-----\r?\n(?:[A-Za-z0-9+/=]+\r?\n){2,}-----END ', data): raise ValueError(f'Private key rejected in {name}')
    public=json.loads(sources['collector-web/firmware/manifest.json'])
    assert (public['schema'], public['chip'], public['board'], public['rotation'], public['flash_size']) == (1, 'ESP32-S3', 'waveshare-v1', 3, 16777216)
    assert [(part['offset'],part['path']) for part in public['parts']] == [(0,'/firmware/bootloader.bin'),(0x8000,'/firmware/partitions.bin'),(0xe000,'/firmware/boot_app0.bin'),(0x10000,'/firmware/firmware.bin')]
    for part in public['parts']:
        data=sources['collector-web'+part['path']]
        assert len(data)==part['size'] and hashlib.sha256(data).hexdigest()==part['sha256']
        start=part['offset'];end=(start+len(data)+4095)&~4095
        assert not (start<0xe000 and end>0x9000)
    archive=sources['collector-web'+public['source_relink']]
    assert len(archive)==public['source_relink_size'] and hashlib.sha256(archive).hexdigest()==public['source_relink_sha256']
    with tarfile.open(fileobj=io.BytesIO(archive),mode='r:gz') as bundle:
        for member in bundle:
            path=Path(member.name)
            if not member.isfile() or path.is_absolute() or '..' in path.parts:
                raise ValueError('Non-regular or unsafe source/relink member rejected')
            if path.name in {'local_credentials.h','private_defaults.h','secrets.h'}:
                raise ValueError('Private source/relink header rejected')
            data=bundle.extractfile(member).read()
            if any(value in data for value in literals): raise ValueError('Private build default rejected in source/relink member')
    emulator=json.loads(sources['collector-web/emulator/manifest.json'])
    assert emulator['public_build'] and (emulator['width'],emulator['height'],emulator['rotation']) == (320,240,3)
    for name,asset in emulator['assets'].items():
        data=sources['collector-web/emulator/'+name]
        assert len(data)==asset['bytes'] and hashlib.sha256(data).hexdigest()==asset['sha256']
    for name,expected in emulator['sources'].items():
        assert hashlib.sha256(sources[name]).hexdigest()==expected, f'Emulator source is stale: {name}'
    version=tomllib.loads(sources['Cargo.toml'].decode())['workspace']['package']['version']
    manifest={'schema':2,'version':version,'status':'host-agnostic collector developer release','source_file_count':len(sources),'sanitization_counts':counts,'runtime':'native Rust','embedded_console':True,'public_firmware':True,'firmware_version':public['version'],'legacy_runtime':False,'manufacturer_references':False,'live_telemetry':False,'files':{name:hashlib.sha256(data).hexdigest() for name,data in sorted(sources.items())}}
    return sources,manifest

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    group=parser.add_mutually_exclusive_group(required=True);group.add_argument('--check-only',action='store_true');group.add_argument('--output',type=Path)
    args=parser.parse_args();sources,manifest=candidate()
    if args.output:
        destination=args.output.absolute()
        if destination.exists() or destination.is_symlink(): parser.error('Output must not exist')
        destination.parent.mkdir(parents=True,exist_ok=True)
        if any(p.is_symlink() for p in destination.parents): parser.error('Symlink output parent rejected')
        stage=Path(tempfile.mkdtemp(prefix='.glimdock-release-',dir=destination.parent))
        try:
            for name,data in sources.items():
                p=stage/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);p.chmod(0o755 if p.suffix=='.sh' else 0o644)
            (stage/'SOURCE_MANIFEST.json').write_text(json.dumps(manifest,indent=2)+'\n')
            os.rename(stage,destination)
        except BaseException: shutil.rmtree(stage,ignore_errors=True);raise
    print(json.dumps({k:v for k,v in manifest.items() if k!='files'},indent=2))
    return 0
if __name__=='__main__': raise SystemExit(main())
