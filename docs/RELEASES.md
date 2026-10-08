# Release downloads and verification

Download the official [v0.1.0 release](https://github.com/gkostadinov/glimdock/releases/tag/v0.1.0).
Use the Linux archive matching `uname -m`: `x86_64` for ordinary 64-bit Intel/AMD
hosts, `aarch64` for 64-bit ARM Linux. The source archive contains firmware and
enclosure sources, build instructions and all retained dependency notices.

| Asset | Purpose |
| --- | --- |
| `glimdock-v0.1.0-source.tar.gz` | Credential-free developer source; frozen r7 enclosure |
| `glimdock-v0.1.0-linux-x86_64.tar.gz` | Static musl collector, installer, setup guide and license inventory |
| `glimdock-v0.1.0-linux-aarch64.tar.gz` | Equivalent ARM64 package; not yet executed on ARM hardware |
| `SHA256SUMS` | SHA-256 digests for the three archives |

Download the checksum file and desired archives into one directory. Verify before
extracting. If you downloaded only one archive, select its exact checksum entry:

```sh
# When all three archives are present:
sha256sum -c SHA256SUMS

# Or verify only the downloaded x86_64 archive:
awk '$2 == "glimdock-v0.1.0-linux-x86_64.tar.gz"' SHA256SUMS | sha256sum -c -
```

On macOS, use `shasum -a 256 -c` instead of `sha256sum -c`. These hashes verify
download integrity; this release does not claim a detached signing certificate.

## v0.1.0 validation

- 70 Rust tests and 98 Python compatibility tests passed.
- 112 native LVGL pointer checks passed and credential-free firmware built.
- x86_64 collector, reader and configuration services were exercised on Proxmox.
- ARM64 was cross-compiled and checked for static ELF linkage; physical execution
  remains unverified.
- Source and binary packages were checked for known credentials, private lab
  addresses and workstation paths. Demo snapshots are authored synthetic fixtures.
- V1 hardware touch/power was verified; physical enclosure fit remains pending.

`SOURCE_MANIFEST.json` records the file hashes in the **frozen downloadable
v0.1.0 source archive**, not every later repository documentation change. The
public repository retains the same v0.1.0 application/enclosure code and adds
README images, navigation, issue templates and public release metadata. Its Git
history records those additions. The downloadable archives and their checksums
are preserved unchanged.

Firmware is distributed as source. No personalized or general ESP32 binary is
published: a redistributed Arduino-linked binary needs corresponding LGPL
library sources and practical relink materials. Read [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md).
