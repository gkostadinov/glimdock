#!/usr/bin/env python3
"""Publish the released r45 viewer without embedding its CAD meshes in the Worker.

Run from the site checkout with --bundle pointing at the public portable-r45
directory and --esbuild pointing at an esbuild executable. Normal site builds
use the checked-in runtime bundle and do not require Python or esbuild.
"""
from pathlib import Path
from urllib.parse import urljoin
import argparse
import hashlib
import json
import re
import shutil
import subprocess

COMMIT = "115aa457c2c804d5e48c1b866748d57f49429dc8"
PACKAGE = "enclosure/public/portable-r45/"
RAW = f"https://raw.githubusercontent.com/gkostadinov/glimdock/{COMMIT}/{PACKAGE}"
WEB = f"https://github.com/gkostadinov/glimdock/tree/main/{PACKAGE}"
BLOB = f"https://github.com/gkostadinov/glimdock/blob/main/{PACKAGE}"
REPO = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--esbuild", default="esbuild")
    args = parser.parse_args()
    source = args.bundle.resolve() / "viewer"
    target = REPO / "public/hardware/viewer"
    editable = REPO / "viewer-src"
    target.mkdir(parents=True, exist_ok=True)
    (editable / "vendor").mkdir(parents=True, exist_ok=True)
    source_names = ["index.html", "files.html", "style.css", "app.js", "scene.json",
                    "vendor/three.module.js", "vendor/THREE-LICENSE.txt"]
    for name in ["app.js", "vendor/three.module.js", "vendor/THREE-LICENSE.txt"]:
        shutil.copyfile(source / name, editable / name)
    shutil.copyfile(source / "app.js.license", editable / "app.js.license")
    for name in ["app.js.license", "style.css.license", "scene.json.license", "index.html.license", "files.html.license"]:
        shutil.copyfile(source / name, target / name)
    shutil.copyfile(source / "vendor/THREE-LICENSE.txt", target / "THREE-LICENSE.txt")

    manifest = json.loads((source / "scene.json").read_text())
    for part in manifest["parts"]:
        part["file"] = urljoin(RAW + "viewer/", part["file"])
        if part.get("print_file"):
            part["print_file"] = urljoin(RAW + "viewer/", part["print_file"])
            part["step_file"] = RAW + "cad/" + part["print_file"].rsplit("/", 1)[-1].removesuffix(".stl") + ".step"
    manifest["source_commit"] = COMMIT
    manifest["notes"].extend([
        "This preview downloads the released geometry from public GitHub; the screen is illustrative.",
        "Portrait is a mechanical stand view, not confirmation of a portrait firmware layout.",
        "Battery and speaker volumes show mechanical space only; battery charging and electrical compatibility require separate verification.",
    ])
    (target / "scene.json").write_text(json.dumps(manifest, indent=2) + "\n")

    app = (editable / "app.js").read_text()
    app = app.replace("fetch(p.file,{cache:'no-store'})", "fetch(p.file,{cache:'force-cache'})")
    old = "step.href='../cad/'+link.download.replace(/\\.stl$/i,'.step');"
    assert old in app
    app = app.replace(old, "step.href=p.record.step_file;")
    old = """    // Sequential mesh reads keep local-server connections bounded and avoid
    // intermittent cached-response failures in the embedded WebKit browser.
    for(const part of manifest.parts)await loadPart(part);"""
    new = """    // Mesh URLs are pinned to the released Git commit. Four workers bound
    // GitHub connections while avoiding one network round trip per part.
    let next=0, completed=0;
    const loadQueue=async()=>{while(next<manifest.parts.length){const part=manifest.parts[next++];await loadPart(part);completed++;$('#loading').textContent=`Loading geometry from GitHub… ${completed}/${manifest.parts.length}`;}};
    await Promise.all(Array.from({length:Math.min(4,manifest.parts.length)},()=>loadQueue()));"""
    assert old in app
    app = app.replace(old, new)
    (editable / "app.js").write_text(app)
    subprocess.run([args.esbuild, str(editable / "app.js"), "--bundle", "--minify",
                    "--format=esm", "--target=es2020", "--legal-comments=linked",
                    "--outfile=" + str(target / "app.js")], check=True)

    extra_css = """
/* Hosted file list and release navigation. */
.site-back{display:inline-block;margin-bottom:18px;color:var(--purple);font-size:13px;text-decoration:none}
.host-note{font-size:12px;color:var(--muted);max-width:64ch;line-height:1.6}
.file-page{max-width:1000px}.file-page table{width:100%;border-collapse:collapse;background:#fff;border:1px solid var(--line)}
.file-page th,.file-page td{text-align:left;padding:12px;border-bottom:1px solid var(--line)}
.file-page th{font-size:11px;text-transform:uppercase;letter-spacing:.06em;color:var(--muted)}
.file-page a{color:var(--purple)}.file-page .intro{max-width:720px;color:var(--muted);margin:18px 0 24px}
.file-page .back{font-size:12px;text-decoration:none}.table-scroll{overflow-x:auto}
"""
    (target / "style.css").write_text((source / "style.css").read_text() + extra_css)

    def href(match):
        value = match[1]
        if value.startswith("../"):
            tail = value[3:]
            if tail == "README.md":
                return f'href="{BLOB}ASSEMBLY.md"'
            if tail.endswith(".md"):
                return f'href="{BLOB}{tail}"'
            return f'href="{RAW}{tail}"'
        return match[0]

    for name, title, description in [
        ("index.html", "Glimdock Portable r45 — interactive 3D enclosure",
         "Explore the current r45 portable enclosure, both stands, interchangeable accessories and exploded assembly. Download the released CAD and print files."),
        ("files.html", "Glimdock Portable r45 — CAD and print files",
         "The 23 current r45 printable components with STL, editable STEP and BREP downloads, plus portable OrcaSlicer project layouts."),
    ]:
        html = (source / name).read_text()
        html = re.sub(r"<title>.*?</title>", f"<title>{title}</title>", html)
        canonical = "https://glimdock.com/hardware/viewer/" + ("files.html" if name == "files.html" else "")
        metadata = f'<meta name="description" content="{description}"><link rel="canonical" href="{canonical}"><meta name="theme-color" content="#6644dd">'
        html = html.replace("</head>", metadata + "</head>")
        html = html.replace('href="data:,"', 'href="/assets/favicon.svg"')
        html = re.sub(r'href="([^"]+)"', href, html)
        if name == "index.html":
            html = html.replace("<main>", '<main>\n<a class="site-back" href="/hardware/">← Glimdock hardware</a>', 1)
            html = html.replace("<div class=\"workspace\">", '<p class="host-note">The 3D files load from the <a href="' + WEB + '">public r45 release on GitHub</a>. Screen artwork and hardware envelopes are illustrative. Portrait firmware and battery electrical setup require separate validation.</p>\n<div class="workspace">')
            html = html.replace("Loading enclosure…", "Loading released geometry from GitHub…")
            html = html.replace("../cad/", RAW + "cad/")
        else:
            html = re.sub(r"<style>.*?</style>", "", html)
            html = html.replace("<main>", '<main class="file-page">', 1)
            html = html.replace("<table>", '<div class="table-scroll"><table>', 1).replace("</table>", "</table></div>", 1)
        (target / name).write_text(html)

    provenance = {
        "schema": 1,
        "release": "portable-r45",
        "source_commit": COMMIT,
        "source_repository": "https://github.com/gkostadinov/glimdock",
        "geometry_origin": RAW,
        "geometry_embedded_in_worker": False,
        "source_assets": {f"{PACKAGE}viewer/{name}": sha(source / name) for name in source_names},
        "adaptations": ["Hosted navigation and canonical metadata", "Immutable public GitHub geometry URLs", "Four bounded mesh-load workers", "Minified runtime bundle; editable source in viewer-src/"],
        "runtime_assets": {name: sha(target / name) for name in ["index.html", "files.html", "app.js", "style.css", "scene.json", "THREE-LICENSE.txt"]},
    }
    (target / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    (editable / "README.md").write_text("# Hosted r45 viewer source\n\nAdapted from the released portable-r45 viewer at commit `" + COMMIT + "`. `app.js` is authored source; `vendor/three.module.js` is Three.js r166 under MIT. Preserve its adjacent license. Enclosure geometry remains unchanged and is loaded from immutable public GitHub URLs.\n\nRegenerate from the public enclosure bundle with `python3 scripts/sync-enclosure-viewer.py --bundle ../../enclosure/public/portable-r45 --esbuild /path/to/esbuild`. The generator stores source and output hashes in the hosted `provenance.json`. Normal website builds use the checked-in runtime and need no extra tooling.\n")
    print(json.dumps({"parts": len(manifest["parts"]), "runtime_bytes": (target / "app.js").stat().st_size, "geometry_embedded": False}))


if __name__ == "__main__":
    main()
