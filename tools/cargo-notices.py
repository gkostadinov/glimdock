#!/usr/bin/env python3
"""Retain published license texts for locked Cargo dependency graphs."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
TARGETS = ("x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl")
UPSTREAM_NOTICES = {
    # These published framework crates omit the repository's licensing document.
    # Their own .cargo_vcs_info.json binds the audited document to an exact commit.
    ("objc2-core-foundation", "0.3.2"): ("https://github.com/madsmtm/objc2", "7b1abfd750a2cacaea71d6a56ecfb83cb7de560b", "LICENSE.md"),
    ("objc2-io-kit", "0.3.2"): ("https://github.com/madsmtm/objc2", "7b1abfd750a2cacaea71d6a56ecfb83cb7de560b", "LICENSE.md"),
}

def upstream_notice(package, source):
    known = UPSTREAM_NOTICES.get((package["name"], package["version"]))
    if not known:
        return None
    repository, commit, path = known
    vcs = json.loads((source / ".cargo_vcs_info.json").read_text())
    if package["repository"] != repository or vcs["git"]["sha1"] != commit:
        raise RuntimeError(f"Upstream notice provenance changed for {package['name']}")
    url = f"https://raw.githubusercontent.com/{repository.removeprefix('https://github.com/')}/{commit}/{path}"
    with urllib.request.urlopen(url, timeout=30) as response:
        raw = response.read(2*1024*1024 + 1)
    if len(raw) > 2*1024*1024:
        raise RuntimeError(f"Oversize upstream notice for {package['name']}")
    raw.decode("utf-8")
    return path, raw, url, commit

def metadata(target, manifest=ROOT / "collector-rs/Cargo.toml"):
    return json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", target,
        "--manifest-path", str(manifest)]))

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
    parser.add_argument("--manifest", type=Path, default=ROOT / "collector-rs/Cargo.toml",
                        help="Cargo.toml to audit (default: collector-rs/Cargo.toml)")
    parser.add_argument("--targets", nargs="+", default=TARGETS,
                        help="Target triples to include (default: Linux x86_64/aarch64 musl)")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Output must be a new directory")
    manifest = args.manifest.resolve()
    targets = tuple(dict.fromkeys(args.targets))
    graphs = {target: metadata(target, manifest) for target in targets}
    ids = {target: reachable(data) for target, data in graphs.items()}
    packages = {p["id"]: p for data in graphs.values() for p in data["packages"]}
    lock = Path(next(iter(graphs.values()))["workspace_root"]).joinpath("Cargo.lock").read_bytes()
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
        fallback = upstream_notice(package, source) if not files else None
        if not files and not fallback:
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
        if fallback:
            path, raw, url, commit = fallback
            destination = f"{package['name']}-{package['version']}/upstream-{path}.txt"
            target = args.output / destination
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
            retained.append({"upstream_path": path, "source_url": url, "source_commit": commit,
                             "retained_path": destination, "sha256": hashlib.sha256(raw).hexdigest()})
        result.append({"name": package["name"], "version": package["version"],
                       "license_expression": package["license"], "authors": package["authors"],
                       "repository": package["repository"],
                       "purl": f"pkg:cargo/{package['name']}@{package['version']}",
                       "registry_checksum": checksums[(package["name"], package["version"])],
                       "targets": [t for t in targets if identity in ids[t]], "notices": retained})
    default_graph = manifest == ROOT / "collector-rs/Cargo.toml" and targets == TARGETS
    scope = ("Linux normal/build dependencies; excludes dev-only and other-platform packages" if default_graph
             else "Selected-target normal/build dependencies; excludes dev-only and unselected-platform packages")
    inventory = {"schema": 1, "scope": scope,
                 "cargo_lock_sha256": hashlib.sha256(lock).hexdigest(), "packages": result}
    args.output.mkdir(parents=True, exist_ok=True)
    if default_graph:
        readme = (
            "# Rust dependency notices\n\nExact published license/copyright texts for the two Linux dependency graphs.\n"
            "`inventory.json` records versions, SPDX expressions, registry checksums and retained text hashes.\n"
            "It includes build dependencies conservatively; dev-only and non-Linux target crates are not linked.\n"
            "Regenerate from the locked source with `python3 tools/cargo-notices.py --output NEW_DIRECTORY`.\n")
    else:
        manifest_label = str(manifest.relative_to(ROOT)) if manifest.is_relative_to(ROOT) else manifest.name
        inventory.update({"manifest": manifest_label, "targets": list(targets)})
        readme = (
            "# Rust dependency notices\n\n"
            f"Exact license/copyright texts for `{manifest_label}` and these target graphs:\n\n"
            + "".join(f"- `{target}`\n" for target in targets)
            + "\n`inventory.json` records versions, SPDX expressions, registry checksums, target membership and retained text hashes.\n"
            "Notices come from the published crates; where a tarball omits its licensing document,\n"
            "the inventory records the exact audited upstream commit and source URL instead.\n"
            "It includes normal and build dependencies conservatively, excludes dev-only/unselected-target crates,\n"
            "and does not include toolchain or operating-system runtime licenses.\n\n"
            "Regenerate from the locked source in a new directory:\n\n```sh\n"
            f"python3 tools/cargo-notices.py --manifest {manifest_label} --targets {' '.join(targets)} --output NEW_DIRECTORY\n```\n")
    (args.output / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
    (args.output / "README.md").write_text(readme)
    print(f"Retained {len(result)} crate inventories and {sum(len(p['notices']) for p in result)} notice files.")

if __name__ == "__main__":
    main()
