#!/usr/bin/env python3
"""Retain published license texts for the actual Linux Cargo dependency graphs."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGETS = ("x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl")

def metadata(target):
    return json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", target,
        "--manifest-path", str(ROOT / "collector-rs/Cargo.toml")]))

def reachable(data):
    nodes = {node["id"]: node for node in data["resolve"]["nodes"]}
    todo, seen = [data["resolve"]["root"]], set()
    while todo:
        identity = todo.pop()
        if identity in seen:
            continue
        seen.add(identity)
        for dep in nodes[identity]["deps"]:
            if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    return seen

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Output must be a new directory")
    graphs = {target: metadata(target) for target in TARGETS}
    ids = {target: reachable(data) for target, data in graphs.items()}
    packages = {p["id"]: p for data in graphs.values() for p in data["packages"]}
    lock = (ROOT / "collector-rs/Cargo.lock").read_bytes()
    checksums = {(p["name"], p["version"]): p.get("checksum") for p in tomllib.loads(lock.decode())["package"]}
    result = []
    for identity in sorted(set.union(*ids.values())):
        package = packages[identity]
        if not package["source"]:
            continue
        source = Path(package["manifest_path"]).parent
        files = [p for p in source.rglob("*") if p.is_file() and (
            p.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT"))
            or str(p.relative_to(source)) == package.get("license_file"))]
        if not files:
            raise RuntimeError(f"Missing published license for {package['name']} {package['version']}")
        retained = []
        for path in sorted(files):
            relative = str(path.relative_to(source))
            destination = f"{package['name']}-{package['version']}/{relative}.txt"
            raw = path.read_bytes()
            raw.decode("utf-8")  # Release source exporter is text-only.
            target = args.output / destination
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
            retained.append({"published_path": relative, "retained_path": destination,
                             "sha256": hashlib.sha256(raw).hexdigest()})
        result.append({"name": package["name"], "version": package["version"],
                       "license_expression": package["license"], "authors": package["authors"],
                       "repository": package["repository"],
                       "purl": f"pkg:cargo/{package['name']}@{package['version']}",
                       "registry_checksum": checksums[(package["name"], package["version"])],
                       "targets": [t for t in TARGETS if identity in ids[t]], "notices": retained})
    inventory = {"schema": 1, "scope": "Linux normal/build dependencies; excludes dev-only and other-platform packages",
                 "cargo_lock_sha256": hashlib.sha256(lock).hexdigest(), "packages": result}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
    (args.output / "README.md").write_text(
        "# Rust dependency notices\n\nExact published license/copyright texts for the two Linux dependency graphs.\n"
        "`inventory.json` records versions, SPDX expressions, registry checksums and retained text hashes.\n"
        "It includes build dependencies conservatively; dev-only and non-Linux target crates are not linked.\n"
        "Regenerate from the locked source with `python3 tools/cargo-notices.py --output NEW_DIRECTORY`.\n")
    print(f"Retained {len(result)} crate inventories and {sum(len(p['notices']) for p in result)} notice files.")

if __name__ == "__main__":
    main()
