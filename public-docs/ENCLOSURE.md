# Portable r45 enclosure

The current public files are in
[Portable r45](../enclosure/public/portable-r45/README.md). The removable pod
contains the display, a battery space and two speakers. Landscape and portrait
stands are separate passive supports. Matching front and rear shells use a
centered four-screw closure; there is no separate underside battery hatch.

## Current files

The curated snapshot includes 23 print STLs, editable STEP/BREP and an original
parts assembly. A self-contained viewer shows the selected geometry, synthetic
screen content and generic measured hardware envelopes. Manufacturer PCB CAD
and live system data are excluded.

Portable OrcaSlicer layouts contain current parts and authored process settings.
Printer connections, vendor presets, macros, calibration data and G-code are
excluded. Select the intended printer and filament and slice again before
printing. Package notes distinguish retained layout geometry from newly
exported or unreviewed poses.

The current front uses its matching internal USB-C tunnel. The display carrier
is two rails; BAT_PWR uses a single button with integral springs and inside-wall
glue tabs. The landscape cradle has a continuous exterior front and a hidden
upward-open button well. Accessories include thicker ears, TV antennas, eyes
and four curved plugs with fuller keyed stems and shallow grip ribs.

## Source and validation

STEP/BREP provide analytic direct-edit source, not parametric feature history.
The snapshot preserves selected geometry and documented coordinate frames.
Nominal CAD and original toolpath checks are limited evidence: complete printed
assembly, surface quality, support removal, grip, spring life and battery
compatibility need physical checks. A portrait stand does not establish
firmware portrait support.

The measured battery envelope is 60.5 × 50.2 × 7.4 mm; each speaker envelope is
30 × 20 × 4.7 mm. Mechanical space does not establish connector polarity, pack
interchangeability or a suitable charging configuration. Qualify battery
operation against the actual board and cell documentation.

Original geometry uses CERN-OHL-P-2.0. Original viewer code, documentation,
metadata and generic tools use MIT; third-party code retains its notices.
Keep the full texts and adjacent asset notices with redistributed files.
See the package README, LICENSE, manifest and SHA256SUMS.

The earlier [Pebble r26 snapshot](../enclosure/public/pebble-r26-prototype-v2/README.md)
remains historical work with its original validation limits. Its separate hatch
and saddle assembly are not the current construction.
