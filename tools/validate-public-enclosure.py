#!/usr/bin/env python3
"""Check the curated enclosure inventory, hashes, links and portable archives."""
from __future__ import annotations
import argparse
import hashlib
import ipaddress
import json
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET
import zipfile

REPO = Path(__file__).resolve().parents[1]
RELATIVE = Path('enclosure/public/portable-r45')
PRIVATE_PATH = re.compile(rb'/(?:Users|home|Volumes)/[^\s\"\'<>]+|[A-Za-z]:\\(?:Users|Documents)\\')
IPV4 = re.compile(rb'(?<![\d.])(?:\d{1,3}\.){3}\d{1,3}(?![\d.])')
SECRET = re.compile(rb'(?i)(?:api[_-]?key|auth[_-]?token|password|ssid|printhost_apikey)\s*[\"\']?\s*[:=]\s*[\"\'][^\"\']+')
TEXT = {'.md', '.json', '.txt', '.license', '.html', '.js', '.css', '.py', '.step', '.brep'}
FORBIDDEN = {'.gcode', '.log', '.pyc', '.blend', '.blend1', '.tmp', '.zip'}

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def inspect_bytes(data, label):
    assert not PRIVATE_PATH.search(data), f'Local path in {label}'
    assert not SECRET.search(data), f'Credential-like value in {label}'
    for match in IPV4.finditer(data):
        try: address = ipaddress.ip_address(match.group().decode())
        except ValueError: continue
        networks = ((0x0a000000,8), (0xac100000,12), (0xc0a80000,16))
        assert not any(address in ipaddress.ip_network(network) for network in networks), f'Private machine address in {label}'

def local_path(base, value, package):
    parsed = urlsplit(value)
    if parsed.scheme or parsed.netloc or not parsed.path: return None
    target = (base/unquote(parsed.path)).resolve()
    assert target.is_relative_to(package.resolve()), f'Link leaves package: {value}'
    assert target.exists(), f'Missing local link: {value}'
    return target

def inspect_3mf(path):
    with zipfile.ZipFile(path) as archive:
        assert archive.testzip() is None, f'Broken archive: {path.name}'
        names = archive.namelist()
        assert len(names) == len(set(names)), f'Duplicate archive entries: {path.name}'
        assert '[Content_Types].xml' in names and '3D/3dmodel.model' in names
        for name in names:
            pure = Path(name)
            assert not pure.is_absolute() and '..' not in pure.parts, f'Unsafe archive path: {path.name}'
            lower = name.lower()
            assert not lower.endswith(('.gcode', '.log', '.png', '.jpg', '.jpeg', '.ini')), f'Unexpected archive entry: {name}'
            assert not any(word in lower for word in ('printer_settings', 'filament_settings', 'dispatch', 'provenance', 'readiness', 'calibration')), f'Private/profile archive entry: {name}'
            data = archive.read(name)
            inspect_bytes(data, path.name+':'+name)
            assert not any(word in data for word in (b'PRINT_START', b'PRINT_END', b'ADAPTIVE_LINE_PURGE', b'/printer/print/start', b'bed_clear_evidence')), f'Printer operations in {path.name}'
            if lower.endswith(('.xml', '.model', '.rels')): ET.fromstring(data)
            if lower.endswith('.config') and data.lstrip().startswith(b'{'):
                config = json.loads(data)
                assert not any(k in config for k in (
                    'machine_start_gcode', 'machine_end_gcode', 'print_host', 'printhost_apikey',
                    'before_layer_change_gcode', 'layer_change_gcode', 'filament_start_gcode',
                    'filament_end_gcode')), f'Machine macros or connection in {path.name}'
    return len(names)

def validate(root=REPO):
    package = Path(root)/RELATIVE
    manifest = json.loads((package/'manifest.json').read_text())
    assert manifest['revision'] == 'portable-r45' and manifest['printable_part_count'] == 23
    files = manifest['files']
    actual = {p.relative_to(package).as_posix() for p in package.rglob('*') if p.is_file()}
    assert actual == set(files)|{'manifest.json', 'SHA256SUMS'}, 'Inventory has missing or extra files'
    archives = []
    for name, expected in files.items():
        path = package/name
        assert not path.is_symlink() and path.resolve().is_relative_to(package.resolve())
        assert not any(part.startswith('.') or part == '__pycache__' for part in Path(name).parts), f'Hidden/cache file: {name}'
        assert path.suffix.lower() not in FORBIDDEN, f'Development artifact: {name}'
        assert path.stat().st_size < 90*1024*1024, f'File too large for ordinary Git: {name}'
        assert digest(path) == expected, f'Hash mismatch: {name}'
        data = path.read_bytes()
        if path.suffix.lower() in TEXT:
            data.decode('utf8')
            inspect_bytes(data, name)
            if path.suffix.lower() == '.md':
                for value in re.findall(r'\]\(([^)]+)\)', data.decode()): local_path(path.parent, value, package)
        if path.suffix.lower() in {'.stl', '.step', '.brep', '.3mf'}:
            assert Path(str(path)+'.license').is_file(), f'Missing geometry notice: {name}'
        if path.suffix.lower() == '.3mf':
            archives.append({'file': name, 'entries': inspect_3mf(path)})
    sums = {}
    for line in (package/'SHA256SUMS').read_text().splitlines():
        h, name = line.split('  ', 1)
        assert name not in sums and re.fullmatch(r'[a-f0-9]{64}', h)
        sums[name] = h
    assert set(sums) == actual-{'SHA256SUMS'}
    assert all(digest(package/name) == h for name,h in sums.items()), 'SHA256SUMS mismatch'
    parts = json.loads((package/'parts.json').read_text())['parts']
    assert len(parts) == 23
    rows = parts.values() if isinstance(parts,dict) else parts
    for row in rows:
        paths = {'step': row['cad']['step'], 'brep': row['cad']['brep'],
                 'stl': row['stl'], 'assembly': row['assembly']}
        for kind,name in paths.items():
            assert digest(package/name) == row['sha256'][kind], f'Part source fingerprint: {row["name"]}'
        checks = row['geometry_checks']
        assert checks['valid_cad'] and checks['cad_solids'] == 1
        assert checks['watertight_mesh'] and checks['winding_consistent'] and checks['mesh_components'] == 1
        assert checks['positive_volume'] and checks['print_base_at_z0'] and checks['proper_rigid_print_pose']
    assert len(list((package/'stl').glob('*.stl'))) == 23
    assert len(archives) == 8
    scene = json.loads((package/'viewer/scene.json').read_text())
    for row in scene['parts']:
        for key in ['file', 'print_file']:
            if row.get(key): local_path(package/'viewer', row[key], package)
    for value in scene.get('downloads', {}).values():
        if isinstance(value,str): local_path(package/'viewer', value, package)
    page = (package/'viewer/index.html').read_text()
    for value in re.findall(r'(?:src|href)=[\"\']([^\"\']+)', page): local_path(package/'viewer', value, package)
    return {'passed': True, 'revision': 'portable-r45', 'files': len(actual),
            'printable_parts': 23, 'portable_layouts': len(archives)}

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=REPO)
    args = parser.parse_args()
    print(json.dumps(validate(args.root), indent=2))
