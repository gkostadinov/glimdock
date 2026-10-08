# License scope

Original Glimdock firmware, Rust collector, Python compatibility tools, previews,
tests and documentation are MIT licensed under `LICENSE`. Third-party code and
fonts retain their own licenses: see `THIRD_PARTY_NOTICES.md` and `LICENSES/`.

Original enclosure design sources under `enclosure/` are licensed under the
CERN Open Hardware Licence Version 2 – Permissive (CERN-OHL-P-2.0), reproduced
in `LICENSES/CERN-OHL-P-2.0.txt`. Their copyright is held by the 2026 Glimdock
contributors. This applies to the authored case and keeper geometry and its
design scripts; it does not grant rights to the manufacturer's PCB/drawings.
Manufacturer STEP files, slicer profiles and G-code are excluded from this release.
Follow the enclosure license's notice requirements when distributing that hardware.

The original Glimdock graphic mark and logotype paths are MIT licensed. The
Space Grotesk font used to construct the logotype is covered by its SIL Open Font
License notice in `branding/Space-Grotesk-OFL.txt`. No trademark license or
claim of registration is implied. Describe modified products accurately and do
not suggest official endorsement.

No unlicensed BabyStory application or driver file is redistributed. The V1
driver provenance is documented in `docs/DRIVER_PROVENANCE.md`.

The release includes firmware source, not a redistributable ESP32 binary. A local
PlatformIO build links Arduino's LGPL components; distributing those binaries
requires the corresponding library source and practical relink materials, as
explained in `THIRD_PARTY_NOTICES.md`. Linux binary archives include the Rust
dependency license texts and a lockfile-based inventory.
