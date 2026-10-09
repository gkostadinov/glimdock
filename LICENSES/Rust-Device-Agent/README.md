# Rust dependency notices

Exact license/copyright texts for `device-agent-rs/Cargo.toml` and these target graphs:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

`inventory.json` records versions, SPDX expressions, registry checksums, target membership and retained text hashes.
Notices come from the published crates; where a tarball omits its licensing document,
the inventory records the exact audited upstream commit and source URL instead.
It includes normal and build dependencies conservatively, excludes dev-only/unselected-target crates,
and does not include toolchain or operating-system runtime licenses.

Regenerate from the locked source in a new directory:

```sh
python3 tools/cargo-notices.py --manifest device-agent-rs/Cargo.toml --targets x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc --output NEW_DIRECTORY
```
