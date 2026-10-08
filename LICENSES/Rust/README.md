# Rust dependency notices

Exact published license/copyright texts for the two Linux dependency graphs.
`inventory.json` records versions, SPDX expressions, registry checksums and retained text hashes.
It includes build dependencies conservatively; dev-only and non-Linux target crates are not linked.
Regenerate from the locked source with `python3 tools/cargo-notices.py --output NEW_DIRECTORY`.
