# Pebble landscape — current public prototype

The current public snapshot is [Pebble r26 prototype v2](../enclosure/public/pebble-r26-prototype-v2/README.md):
**r25 fitted body and continuous white cradle**, compatible **r24 internal parts**,
and the **r26 angled body print trial**. r26 changes print orientation/supports,
not body CAD. The supported V1 display uses USB-right landscape, rotation 3,
at 320 × 240. The nominal purple envelope is approximately 88 × 64 × 15.64 mm;
the white cradle is approximately 95 × 52 × 15.8 mm. Exact bounds and coordinate
frames are in the package manifest.

The clean opening exposes the board's native USB-C port. The factory glass is
exposed, with its wider black chin to the right. A carrier attaches to the board's
factory posts; keyed saddles mount the pod at an 18° backward desk tilt. The rear
service cover and underside hatch are removable. Optional ears, antennas and
eyes use the top sockets. Follow the current package's hardware quantities and
assembly sequence; earlier extension-port/liner instructions do not apply to
this fitted body.

## Editable source and downloads

The package includes original analytic STEP/BREP parts, an own-parts STEP
assembly, 34 regenerated watertight meshes, five actual-CAD renders, a portable
mesh exporter, notices and SHA256SUMS. STEP/BREP support direct CAD editing;
they do not contain the original parametric construction history. Individual
STLs retain recorded source frames and are **not a released print recipe**.

The original CAD and geometry meshes use CERN-OHL-P-2.0. Generic exporter tools,
documentation and renders use MIT. Preserve the adjacent per-asset notices and
full license texts. Manufacturer board geometry, private fit references, printer
profiles, G-code, Orca projects, dispatch records and live telemetry are excluded.

The old `enclosure/pebble-landscape/` r19 parametric design remains historical
source. It is not the current fitted geometry. Use the v2 prototype package for
the current direct-edit source, fit notes and generated meshes.

## Validation boundary

CAD checks and mesh regeneration passed within the package's recorded scope.
The full assembly is physically unverified. The earlier upright body print
failed; the r26 45° trial was unverified when this snapshot was prepared. The
continuous cradle is unsliced and unprinted. A body print does not prove complete
fit, service access, switch travel/return, insert strength, saddle rigidity or
bed adhesion. Re-slicing needs new support and passage-clearance review; local
postprocessed trial toolpaths are not encoded in the STLs.

Renders show smooth nominal CAD with a synthetic dashboard, not a product photo,
a guaranteed print finish or the latest installed firmware. The exploded view
separates parts for visibility; it does not define their installation paths.

## Battery boundary

This remains a **USB-powered prototype**. Reserved battery space and a BAT_PWR
mechanical input do not qualify a battery product. The firmware's approximate
voltage indicator is not a fuel gauge or charging-state detector. Electrical
compatibility, connector polarity, actual pack envelope and assembly require
separate verification; follow the selected board and cell documentation.
