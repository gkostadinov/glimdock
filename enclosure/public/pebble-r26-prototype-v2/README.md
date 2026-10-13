# Pebble landscape enclosure — prototype CAD snapshot v2

A purple landscape display pod in a white, continuous cradle, with an 18° backward desk tilt. The exposed factory glass sits on a flat seat behind its opaque border. The body has a clean native USB-C opening, a rounded matching rear cover and two keyed top sockets for optional accessories.

Package v2 corrects the license metadata; the geometry, renders and physical validation status are unchanged. It supersedes the original package, which incorrectly applied MIT to geometry.

This snapshot combines the **r25 body and continuous cradle**, compatible **r24 internal parts**, and the **r26 body print trial**. r26 changes the print setup rather than the body geometry. These are editable prototype geometry and illustrative renders; the full assembly has not passed physical validation. The r26 angled body test was started on 2026-10-13, and its physical result was unverified when this package was prepared.

## Included files

| Directory | Contents |
| --- | --- |
| `cad/` | Exact editable STEP parts, original BREP shapes for the main parts, and an assembly containing only original enclosure geometry. |
| `stl/` | Individual geometry meshes, including optional bunny ears, TV antennas, eyes and fit coupons. |
| `assets/` | Actual CAD hero, side, USB and cradle views, plus an exploded illustration. The screen shows synthetic demonstration data. |
| `tools/` | Portable mesh export from the included BREP/STEP shapes. |
| `manifest.json` | Part revisions, coordinate frames, geometry checks and validation limits. |
| `SHA256SUMS` | Integrity hashes for the complete allowlisted package. |

STEP and BREP are the editable CAD source in this snapshot. They retain analytic solids and surfaces for direct editing in CAD applications; they do not contain the original parametric construction history. Open `cad/pebble-own-parts-assembly.step` to inspect the assembled enclosure. The display board, battery and fasteners are references in the illustrations and are not included as third-party CAD downloads.

Dimensions use millimetres. Pod-local parts and assembled-world parts are identified in the manifest. The assembly applies the recorded transform to pod-local parts. Individual STLs retain their source frames: position and orient them in your slicer before use. Their orientation is not a released print recipe.

## Mechanical construction

The nominal purple envelope is approximately **88 × 64 × 15.64 mm**. The nominal white cradle is approximately **95 × 52 × 15.8 mm**; the manifest records actual exported bounds. The native USB port and accessory sockets share the pod's central depth plane. The USB opening is **12.4 × 7.4 mm with a 2 mm corner radius** and an integrated inner throat. The fitted glass seat and native-port registration are preserved from earlier fitting work. Do not add the obsolete separate USB liner to this body.

A separate carrier attaches to the display's factory posts. Two broad keyed saddles secure the pod at a fixed angle. The continuous cradle hides the saddle installation clearances behind a smooth rim. The underside battery hatch is removable independently of the display mounting. The BAT_PWR prototype uses a recessed underside input and a separate direct follower, with printed return leaves and an assembled travel stop.

| Connection | Quantity | Hardware |
| --- | ---: | --- |
| Display to carrier | 4 | M2.5 × 6 machine screws, DIN 965 style |
| Carrier to pod | 2 | M3 × 6 countersunk screws and captive M3 nuts |
| Rear cover | 2 | M3 × 6 countersunk screws and M3 heat inserts |
| Power follower frame | 2 | M3 × 6 screws and captive M3 nuts |
| Pod to saddles | 2 | M3 × 8 countersunk screws and M3 heat inserts |
| Saddles to cradle | 2 | M3 × 8 countersunk screws and M3 heat inserts |
| Battery hatch | 2 | M3 × 8 countersunk screws and M3 heat inserts |

The insert geometry assumes **4.6 mm outer diameter × 5.7 mm length**. Verify the actual inserts and screw heads with coupons. Pointed coarse-thread screws are unsuitable for the factory metal posts. The totals are six M3 × 6 screws, six M3 × 8 screws, four M3 nuts, eight inserts and four M2.5 × 6 machine screws.

Install the saddles before the pod and underside button cartridge: lower a saddle through the central opening with a 4.5 mm forward offset and 0.35 mm lift, slide sideways, align rearwards, slide into its keyed seat, then lower and fasten from underneath. The precise assembly positions are visible in the STEP assembly. Dry-fit the sequence before installing electronics. Clean the open button cartridge and test both moving components together before gluing the two small window covers onto fixed frame edges; keep glue clear of the moving head and return leaves.

## Print trial and unresolved fit checks

The earlier upright body detached from its build plate and failed. The r26 trial prints with the glass face upwards at **45°**, over a connected three-layer foundation of approximately **104 × 67 mm**. Its experimental settings are 0.16 mm layers, a 55°C bed, 20 mm/s outer walls, 500 mm/s² outer-wall acceleration and a 15 mm/s first layer. Fine exterior fuzzy texture uses 0.10 mm thickness and 0.30 mm point spacing. The glass seat and immediate rim, USB rim, internal cavity, accessory fits, screw seats and hidden cradle contacts stay smooth.

The locally reviewed body toolpath has zero floating model starts and zero extrusion intrusions into the tested passage cores. The strict roof screen still fails short concealed service-pocket spans. A separate adaptive-bead review permits a controlled trial with a longest span of 0.954 mm. Support backing remains in accessible nut-loading entrances and the ends of the screwdriver approaches. Two microscopic unsupported tangent starts were omitted from that local toolpath, totalling 0.0151 mm³. **These postprocessing changes are not encoded in the geometry STLs.** Re-slicing requires fresh support-clearance and layer-continuity review.

The r25 cradle has passed nominal CAD interference and saddle installation-route checks, but it has not been sliced or printed. The carrier, cover, button, hatch, accessories and complete assembly still need physical fitting. Confirm free cover and hatch removal, insert strength, saddle rigidity under touch loads, switch click and return, unobstructed cable insertion, glass flatness and support cleanup. Actual bed adhesion and resistance to warped edges remain unverified. The renders show smooth nominal CAD surfaces rather than guaranteed printed finish.

The cradle's curved seating rim can show layer terraces when printed underside-down. Its analytic surface is smooth; finer mesh export alone does not remove the terraces. Review print orientation and suitable rim coupons before choosing the final base recipe.

## Battery boundary

This revision is **USB powered**. The white cradle reserves the geometry of a PKCell LP503035 500 mAh pack and includes a removable tray and protected lead route. Pack capacity alone does not establish fit. Larger packs require a new complete envelope and cable-route check.

The battery and charger have not been electrically qualified together. Verify the actual board revision, charging current, connector family, adapter wiring and polarity against the selected pack before connecting it. Mechanical space and the BAT_PWR button prototype do not constitute approval for battery operation. Consult the [Waveshare board schematic](https://files.waveshare.com/wiki/ESP32-S3-Touch-LCD-2.8/ESP32-S3-Touch-LCD-2.8.pdf) and [ETA6098 charger datasheet](https://www.eta-semi.com/wp-content/uploads/2022/03/ETA6098_V1.1.pdf). The firmware must also implement the required power-control behaviour for the chosen board revision.

## License and public boundary

Original enclosure CAD and geometry meshes use **CERN-OHL-P-2.0**, with the full text in `LICENSES/CERN-OHL-P-2.0.txt` and adjacent asset notices. The generic exporter, documentation, metadata and renders use **MIT**, with the full text in `LICENSES/MIT.txt`. The root `LICENSE` records these separate scopes. Waveshare hardware appears in the renders as a fit reference; this package does not redistribute its board CAD. The synthetic dashboard illustration is demonstrative and does not promise a particular installed firmware version.

This package excludes machine addresses, printer configuration, G-code, Orca projects, dispatch records, task history, private model references and live telemetry. Do not substitute unfiltered working directories for this allowlisted snapshot.
