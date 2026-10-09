#!/usr/bin/env python3
"""Package target-matched Rust executables with audited public guides and licenses."""
from __future__ import annotations
import argparse, hashlib, importlib.util, json, os, re, shutil, struct, tarfile, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = re.compile(r"(?:x86_64|aarch64)-(?:unknown-linux-(?:musl|gnu)|apple-darwin|pc-windows-msvc)\Z")
PUBLIC_FILES = {"README.md", "SETUP.md", "LICENSE", "LICENSE_POLICY.md", "THIRD_PARTY_NOTICES.md", "Cargo.lock", "docs/architecture.md", "docs/validation.md"}
PUBLIC_TREES = ("examples/", "public-docs/", "LICENSES/", "deploy/")


def public_export():
    spec = importlib.util.spec_from_file_location("glimdock_public_export", ROOT/"tools/package-public-release.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def include_linked_documents(files: dict[str, bytes], sources: dict[str, bytes]) -> None:
    """Retain the public references used by the packaged setup and license guides."""
    inspected = set()
    while pending := [name for name in files if name.endswith('.md') and name not in inspected]:
        for name in pending:
            inspected.add(name)
            for target in re.findall(r'\]\(([^\s)]+)', files[name].decode()):
                if target.startswith(('https:', 'http:', 'mailto:', '#')):
                    continue
                relative = Path(os.path.normpath(Path(name).parent/target.split('#')[0])).as_posix()
                if relative in sources:
                    files[relative] = sources[relative]
                else:
                    children = {key: data for key, data in sources.items() if key.startswith(relative.rstrip('/')+'/')}
                    if not children:
                        raise ValueError(f'Missing public guide reference in {name}')
                    files.update(children)


def verify_executable(path: Path, target: str, private: list[bytes]) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise ValueError("Executable must be a regular file")
    data = path.read_bytes()
    architecture = target.split("-", 1)[0]
    if "linux" in target:
        valid = (len(data) >= 20 and data[:6] == b"\x7fELF\x02\x01" and
                 struct.unpack_from("<H", data, 18)[0] == {"x86_64": 62, "aarch64": 183}[architecture])
    elif "apple" in target:
        valid = (len(data) >= 8 and data[:4] == b"\xcf\xfa\xed\xfe" and
                 struct.unpack_from("<I", data, 4)[0] == {"x86_64": 0x1000007, "aarch64": 0x100000c}[architecture])
    else:
        offset = struct.unpack_from("<I", data, 60)[0] if len(data) >= 64 else len(data)
        valid = (data[:2] == b"MZ" and offset+6 <= len(data) and data[offset:offset+4] == b"PE\0\0" and
                 struct.unpack_from("<H", data, offset+4)[0] == {"x86_64": 0x8664, "aarch64": 0xaa64}[architecture])
    if not valid:
        raise ValueError("Executable OS/architecture does not match --target")
    if any(value in data for value in private):
        raise ValueError("Private build default rejected in executable")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--collector", type=Path)
    parser.add_argument("--agent", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not args.collector and not args.agent:
        parser.error("Supply --collector, --agent, or both")
    if not TARGET.fullmatch(args.target):
        parser.error("Unsupported OS/architecture target")
    if args.collector and "windows" in args.target:
        parser.error("The central collector is supported on Linux/macOS; package only the Windows agent")
    destination = args.output.absolute()
    if destination.exists() or destination.is_symlink():
        parser.error("Output directory must not exist")
    if any(p.is_symlink() for p in destination.parents):
        parser.error("Symlink output parent rejected")
    try:
        exporter = public_export()
        sources, source_manifest = exporter.candidate()
        binaries = {}
        suffix = ".exe" if "windows" in args.target else ""
        for path, name in [(args.collector, "glimdock-collector"), (args.agent, "glimdock-agent")]:
            if path:
                binaries["bin/"+name+suffix] = verify_executable(path, args.target, exporter.private_values())
    except (ValueError, AssertionError) as error:
        parser.error(str(error))
    files = {name: data for name, data in sources.items() if name in PUBLIC_FILES or name.startswith(PUBLIC_TREES)}
    include_linked_documents(files, sources)
    if args.collector:
        # Keep the exact embedded firmware's corresponding source available offline.
        for name in ["collector-web/firmware/manifest.json", "collector-web/firmware/source-relink.tar.gz", "collector-web/emulator/LICENSES.txt"]:
            files[name] = sources[name]
    files.update(binaries)
    version = source_manifest["version"]
    name = f"glimdock-v{version}-{args.target}"
    launcher = "glimdock-agent.exe" if suffix else "glimdock-agent"
    usage = [f"# Glimdock {version} — {args.target}", "", "These binaries contain the native Rust runtime. Run commands from this extracted package.", ""]
    if args.collector:
        usage += ["Start the collector, node management and embedded web console together:", "", "```sh", "./bin/glimdock-collector run --state-dir ./glimdock-state --bind 127.0.0.1 --port 8765", "```", "", "Open http://127.0.0.1:8765/. Follow SETUP.md for credentials, LAN access, pairing and services.", ""]
    if args.agent:
        command = f".\\bin\\{launcher}" if suffix else f"./bin/{launcher}"
        agent_state = "C:\\Private\\glimdock-agent" if suffix else "/ABSOLUTE/PRIVATE/glimdock-agent"
        usage += ["Pair this device with the collector and push telemetry (download pairing.key from Nodes → Pair a device):", "", "```", command+f" --collector-url https://collector.example --state-dir {agent_state} --enrollment-key-file ./pairing.key", "```", "", "Use an absolute private state directory. Later starts use the same collector URL and state directory; enrollment is retained.", "For an HTTP collector on your trusted LAN, add --allow-insecure-http explicitly.", ""]
        usage += ["Sample this device without opening a network listener:", "", "```", command+" --config examples/agents/host.json --once", "```", "", "public-docs/DEVICES.md explains agent services, SNMP/JSON adapters and node registration.", ""]
    usage += ["The repository-build sections of the guides apply to a separate matching source export.", "PACKAGE_MANIFEST.json records every packaged file's SHA-256. Firmware source/relink material", "and applicable library notices accompany collector packages; dependency licenses are under LICENSES/.", ""]
    files["PACKAGE_README.md"] = "\n".join(usage).encode()
    manifest = {"schema": 1, "version": version, "target": args.target,
                "firmware_version": source_manifest["firmware_version"] if args.collector else None,
                "files": {name: hashlib.sha256(data).hexdigest() for name, data in sorted(files.items())}}
    files["PACKAGE_MANIFEST.json"] = (json.dumps(manifest, indent=2)+"\n").encode()
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=".glimdock-package-", dir=destination.parent))
    try:
        package = temporary/name
        for relative, data in files.items():
            path = package/relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            path.chmod(0o755 if relative.startswith("bin/") or path.suffix == ".sh" else 0o644)
        archive = temporary/(name+".tar.gz")
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.add(package, arcname=package.name)
        result = {"archive": str(destination/archive.name), "bytes": archive.stat().st_size,
                  "sha256": hashlib.sha256(archive.read_bytes()).hexdigest(), "version": version, "target": args.target}
        os.rename(temporary, destination)
    except BaseException:
        shutil.rmtree(temporary, ignore_errors=True)
        raise
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
