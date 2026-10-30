#!/usr/bin/env python3
"""Import reviewed r45 WebP renders and bind them to the released CAD.

This optional standard-library tool changes product images and provenance only.
It does not regenerate the firmware demo or publish the website. Generate the
images with render-product-r45.py, review them, then supply the encoded images.
"""

import argparse
import hashlib
import json
import shutil
from pathlib import Path

RELEASE_COMMIT = "115aa457c2c804d5e48c1b866748d57f49429dc8"
RELEASE_PATH = "enclosure/public/portable-r45"
RELEASE_URL = f"https://github.com/gkostadinov/glimdock/tree/{RELEASE_COMMIT}/{RELEASE_PATH}"
SHOTS = ("hero", "side", "usb-detail", "cradle-detail", "exploded", "portrait")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case-root", type=Path, required=True,
                        help="Local copy of the immutable public portable-r45 bundle")
    parser.add_argument("--renders", type=Path, required=True,
                        help="Reviewed WebPs and r45-render-provenance.json")
    args = parser.parse_args()
    case, renders = args.case_root.resolve(), args.renders.resolve()
    site = Path(__file__).resolve().parents[1]
    public = site / "public"
    release = json.loads((case / "manifest.json").read_text())
    recipe = json.loads((renders / "r45-render-provenance.json").read_text())
    sources = recipe["source_files"]
    if isinstance(sources, list):
        sources = {item["path"]: item["sha256"] for item in sources}
    for relative, expected in sources.items():
        source = (case / relative).resolve()
        if not source.is_relative_to(case) or sha(source) != expected:
            raise ValueError(f"Render source does not match released CAD: {relative}")
    copied = {}
    for name in SHOTS:
        source = renders / f"{name}.webp"
        data = source.read_bytes()
        if data[:4] != b"RIFF" or data[8:12] != b"WEBP":
            raise ValueError(f"Expected a WebP image: {name}")
        details = recipe["renders"][name]
        if details["sha256"] != sha(source):
            raise ValueError(f"Reviewed image changed: {name}")
        if min(details["width"], details["height"]) < 800:
            raise ValueError(f"Image is too small: {name}")
        target = public / "assets" / source.name
        shutil.copyfile(source, target)
        copied[name] = {
            "source": "Released CAD rendered with scripts/render-product-r45.py",
            "source_sha256": details["source_sha256"],
            "public": f"commercial/site/public/assets/{source.name}",
            "sha256": sha(target), "bytes": target.stat().st_size,
            "width": details["width"], "height": details["height"],
        }
    for obsolete in ("rear", "ears", "antennas", "eyes"):
        (public / f"assets/{obsolete}.webp").unlink(missing_ok=True)
    renderer_manifest = json.loads((public / "demo/emulator/manifest.json").read_text())
    wasm = public / "demo/emulator/firmware.wasm"
    if renderer_manifest["assets"]["firmware.wasm"]["sha256"] != sha(wasm):
        raise ValueError("The existing firmware demo does not match its manifest")
    provenance = {
        "schema": 3,
        "case_revision": "Portable r45 — matching portable shells, passive stands and retained accessories",
        "release_commit": RELEASE_COMMIT,
        "geometry_source": RELEASE_URL,
        "design_sha256": sha(case / "manifest.json"),
        "parts_sha256": sha(case / "parts.json"),
        "geometry": release.get("geometry_status", release.get("geometry_validation")),
        "screen_artwork": "Synthetic dashboard illustration; not installed firmware or a physical product photo. Portrait artwork does not establish portrait firmware support.",
        "reference_hardware": "Generic display, glass, battery and speaker envelopes; no manufacturer PCB CAD",
        "render_recipe": "commercial/site/scripts/render-product-r45.py",
        "render_recipe_sha256": sha(site / "scripts/render-product-r45.py"),
        "source_files": dict(sorted(sources.items())),
        "renders": copied,
        "renderer": renderer_manifest,
        "fallback": {
            "sha256": sha(public / "assets/screens/all-nodes.png"),
            "data": "Existing synthetic firmware capture, unchanged by this enclosure refresh",
            "width": 320, "height": 240,
        },
    }
    (public / "assets/product-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    print(json.dumps({"case_revision": provenance["case_revision"], "images": len(copied),
                      "image_bytes": sum(v["bytes"] for v in copied.values())}))


if __name__ == "__main__":
    main()
