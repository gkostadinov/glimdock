# Pebble Landscape prototype

The current enclosure is Pebble Landscape r19, a purple pillowed body over a
low white base with an 18° backward tilt. Its nominal body is approximately
88 × 64 × 23.8 mm; the compact USB base is approximately 90 × 48 mm.
The supported V1 board uses 320 × 240 artwork and rotation 3, with USB to the right.

The complete factory glass remains exposed, including its wider right-hand
black chin. A constant 2 mm front border follows the glass corners concentrically;
the support sits behind the glass. A separate carrier mounts the board through
its factory metal posts. The rear service panel and underside cable hatch are
removable. Blank caps, ears, antennas and eyes use interchangeable top sockets.

The right-side USB-C mouth connects to the board through a captured replaceable
clamp and provisional internal extension. Connector dimensions, assembly
clearance and power/data through that extension still need physical testing.
The factory BOOT/RESET buttons require opening the enclosure for access.

This is a prototype under physical validation. The purple body alone has been
sent for a prototype print; complete fitting is not established. The main model
is USB-powered. A larger future battery base is a separate unbuilt variant.

The authored CAD is defined by `design.json`, `build.py`, `base_style.py`,
`landscape_mechanics.py` and `accessories.py` in `enclosure/pebble-landscape/`.
Its final design parameters take precedence over these rounded dimensions.
A V1 manufacturer STEP is required under `enclosure/reference/`; obtain it
separately. Manufacturer geometry, personal fit-reference files, saved slicer
profiles, G-code and private review logs are excluded from the public source
package. The authored case source is CERN-OHL-P-2.0 licensed.

The complete local case-design folder retains the reviewed CAD/STL renders,
nominal checks, prototype print records and assembly guide. Earlier Serein and
portrait/round Pebble concepts remain historical work.
