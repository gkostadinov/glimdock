<!-- SPDX-License-Identifier: MIT -->
# Portable r45 print layouts

These are the current model layouts for OrcaSlicer. They contain the selected
r45 geometry, its plate positions, authored process choices and any retained
support modifiers. They contain **no G-code, printer connection settings, custom
machine macros, vendor printer presets or filament profiles**.

Open a layout in OrcaSlicer, choose your own printer and filament, then slice
and inspect it before printing. The coordinates use a nominal 220 × 220 mm
workspace; arrange the parts again if your plate requires it.

| Layout | Included parts | Notes |
| --- | --- | --- |
| [Front and USB tunnel](portable-r45-front-and-usb-tunnel.3mf) | Repaired front and its matching tunnel | Keeps the glass face up, the flat rear contact down, local layer ranges and the support exclusion/enforcement shapes. |
| [Rear cover](portable-r45-rear-cover.3mf) | Current whole rear shell | Symmetric four-screw pattern; no separate battery hatch. |
| [Display carrier rails](portable-r45-display-carrier-rails.3mf) | Left and right rails | Replaces the earlier one-piece carrier. The obsolete notched stand has been removed from this layout. |
| [Landscape stand](portable-r45-landscape-stand.3mf) | Closed-front landscape stand | Hidden upward-open button clearance; the old thin ears have been removed. |
| [Power button](portable-r45-power-button.3mf) | Compact button with integral springs and inside glue tabs | Fine layer setup retained. Glue only the fixed tabs after checking free movement. |
| [Accessories and plugs](portable-r45-accessories-and-plugs.3mf) | Two 6 mm ears, two TV antennas, two eyes and four curved plugs | An 8 mm outside brim on every piece; fuller keyed stems with shallow grip ribs. |
| [Rear internals](portable-r45-rear-internals.3mf) | Battery tray and four rear screw caps | The superseded separate plunger and guide are omitted. |
| [Portrait stand layout](portable-r45-portrait-stand-layout.3mf) | Retained passive portrait stand | **Model layout only.** No reviewed print recipe is supplied; choose process settings and review underside support before printing. |

The seven process layouts retain their selected model and modifier geometry and
poses. Unneeded objects were removed from combined plates. The portrait layout
uses the unchanged stand STL in its correct Z-up bed pose.

For the original PLA trials, the common reference was a 55°C bed, 20 mm/s outer
walls and 500 mm/s² acceleration. The compact button uses 0.08 mm layers; most
small parts and stands use 0.12 mm. The front includes its recorded local layer
ranges. These values are starting points rather than a calibration for another
printer or filament.

## Compatibility fields

OrcaSlicer requires a few context fields to open and save a project correctly.
The files use neutral printer/material labels, a nominal 0.4 mm nozzle marker and
a 250 mm height marker. They are compatibility placeholders, **not a printer
preset or a declaration of your machine limits**. Selecting your real printer
and filament is required. No printer or filament calibration is carried over.

All eight files were opened and exported again by OrcaSlicer 2.4.2. Their 23
normal model parts and retained support modifiers survived that round trip;
vertex positions and triangle counts were checked. This validates file import
and geometry preservation. It does not establish adhesion, support release,
printed tolerances, accessory holding force or battery electrical compatibility.

Original enclosure geometry in the 3MF files is licensed under
**CERN-OHL-P-2.0**. Preserve the adjacent notices and the package license texts.
Documentation and the public project index are MIT licensed.
