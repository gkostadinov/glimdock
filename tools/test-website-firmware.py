#!/usr/bin/env python3
"""Verify frozen website releases independently from collector firmware rebuilds."""
from pathlib import Path
import copy
import hashlib
import importlib.util
import json
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('public_export', ROOT/'tools/package-public-release.py')
exporter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exporter)
PREFIX = 'commercial/site/public'
MANIFEST = PREFIX+'/firmware/manifest.json'
FROZEN = PREFIX+'/firmware/release-manifest.json'


def fixture():
    paths = [MANIFEST, FROZEN, 'collector-web/assets/usb-flasher.js', 'collector-web/src/flash-plan.js', PREFIX+'/assets/usb-flasher.js', PREFIX+'/flash/flash-plan.js']
    manifest = json.loads((ROOT/MANIFEST).read_text())
    paths += [PREFIX+p['path'] for p in manifest['parts']]
    return {name: (ROOT/name).read_bytes() for name in paths}


class PublishedFirmware(unittest.TestCase):
    def test_pinned_release_is_verified(self):
        exporter.verify_website_firmware(fixture())

    def test_collector_rebuild_does_not_replace_published_installer(self):
        sources = fixture()
        rebuilt = copy.deepcopy(json.loads(sources[MANIFEST]))
        rebuilt['version'] = 'glimdock-'+'f'*12
        rebuilt['parts'][-1]['sha256'] = 'e'*64
        rebuilt['source_relink_sha256'] = 'd'*64
        sources['collector-web/firmware/manifest.json'] = json.dumps(rebuilt).encode()
        sources['collector-web/firmware/firmware.bin'] = b'A separate CI build'
        exporter.verify_website_firmware(sources)

    def test_changed_image_is_rejected(self):
        sources = fixture()
        sources[PREFIX+'/firmware/firmware.bin'] += b'changed'
        with self.assertRaises(AssertionError): exporter.verify_website_firmware(sources)

    def test_external_source_archive_is_rejected(self):
        sources = fixture();manifest = json.loads(sources[MANIFEST])
        manifest['source_relink'] = 'https://other.example/source.tar.gz'
        sources[MANIFEST] = json.dumps(manifest).encode()
        with self.assertRaises(AssertionError): exporter.verify_website_firmware(sources)

    def test_even_self_consistent_unsafe_layout_is_rejected(self):
        sources = fixture();manifest = json.loads(sources[MANIFEST]);frozen = json.loads(sources[FROZEN])
        frozen['parts'][1]['offset'] = 0x9000
        sources[FROZEN] = json.dumps(frozen).encode()
        manifest['release_manifest_sha256'] = hashlib.sha256(sources[FROZEN]).hexdigest()
        manifest['parts'] = frozen['parts'];sources[MANIFEST] = json.dumps(manifest).encode()
        with self.assertRaises(AssertionError): exporter.verify_website_firmware(sources)

    def test_unexpected_binary_is_rejected(self):
        sources = fixture();sources[PREFIX+'/firmware/extra.bin'] = b'unknown'
        with self.assertRaises(AssertionError): exporter.verify_website_firmware(sources)

    def test_changed_flash_safeguards_are_rejected(self):
        sources = fixture();sources[PREFIX+'/flash/flash-plan.js'] += b'changed'
        with self.assertRaises(AssertionError): exporter.verify_website_firmware(sources)


if __name__ == '__main__': unittest.main()
