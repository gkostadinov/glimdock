# Hosted r45 viewer source

Adapted from the released portable-r45 viewer at commit `115aa457c2c804d5e48c1b866748d57f49429dc8`. `app.js` is authored source; `vendor/three.module.js` is Three.js r166 under MIT. Preserve its adjacent license. Enclosure geometry remains unchanged and is loaded from immutable public GitHub URLs.

Regenerate from the public enclosure bundle with `python3 scripts/sync-enclosure-viewer.py --bundle ../../enclosure/public/portable-r45 --esbuild /path/to/esbuild`. The generator stores source and output hashes in the hosted `provenance.json`. Normal website builds use the checked-in runtime and need no extra tooling.
