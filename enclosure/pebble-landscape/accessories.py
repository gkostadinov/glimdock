"""Optional, flat-printing Pebble Landscape ornaments and keyed fit coupons.

No import-time exports. ``accessory_parts()`` returns CadQuery geometry in the
body's local frame (X across, Y up, Z toward the rear). The main builder owns the
body/world transform, blank caps, and file export. ``print_shape`` is the actual
bed pose with explicit support metadata; it must not receive the body's
18-degree assembly tilt.

Pins and ornament roots lie in print XY, rather than being printed vertically.
All dimensions are millimetres. Fit is nominal until the supplied coupon has
been tried with the user's PLA and slicer profile.
"""

from dataclasses import dataclass
from typing import Any
import json
from pathlib import Path

import cadquery as cq


PURPLE = "#70479E"
WHITE = "#EFEFEB"


@dataclass(frozen=True)
class AccessoryInterface:
    centers_x: tuple[float, float] = (-15.0, 15.0)
    seat_y: float = 41.0
    center_z: float = 18.4
    pin_width: float = 6.0
    pin_thickness: float = 4.0
    pin_length: float = 4.5
    key_clip: float = 0.8
    socket_depth: float = 5.0
    default_clearance: float = 0.2


_DESIGN = json.loads((Path(__file__).parent / "design.json").read_text())["accessory"]
INTERFACE = AccessoryInterface(
    centers_x=tuple(_DESIGN["centers_x"]), seat_y=_DESIGN["seat_y"],
    center_z=_DESIGN["center_z"], pin_width=_DESIGN["pin_width"],
    pin_thickness=_DESIGN["pin_depth"], pin_length=_DESIGN["pin_insertion"],
    key_clip=_DESIGN["key_clip"], socket_depth=_DESIGN["socket_depth"],
    default_clearance=_DESIGN["side_clearance"],
)


def keyed_profile(clearance: float = 0.0) -> cq.Workplane:
    """XZ profile matching the main builder's +X/+Z clipped key corner."""
    p = INTERFACE
    wire = (cq.Workplane("XZ")
            .polyline([(-p.pin_width / 2, -p.pin_thickness / 2),
                       (p.pin_width / 2, -p.pin_thickness / 2),
                       (p.pin_width / 2, p.pin_thickness / 2 - p.key_clip),
                       (p.pin_width / 2 - p.key_clip, p.pin_thickness / 2),
                       (-p.pin_width / 2, p.pin_thickness / 2)])
            .close())
    # Match the main body's sharp parallel offset exactly, including the
    # intersections at the clipped key corner.
    return wire.offset2D(clearance, kind="intersection") if clearance else wire


def pin(length: float | None = None) -> cq.Workplane:
    # Positive extrusion on XZ points inward, along local -Y.
    return keyed_profile().extrude(length or INTERFACE.pin_length)


def _slab(w: float, h: float, t: float, x=0.0, y=0.0, z=-2.0,
          r: float = 0.0) -> cq.Workplane:
    result = (cq.Workplane("XY").workplane(offset=z).center(x, y)
              .rect(w, h).extrude(t))
    if r:
        result = result.edges("|Z").fillet(r)
    return result


def _disc(d: float, t: float, x=0.0, y=0.0, z=-2.0) -> cq.Workplane:
    return (cq.Workplane("XY").workplane(offset=z).center(x, y)
            .circle(d / 2).extrude(t))


def _stop() -> cq.Workplane:
    # Same lower/upper print planes as the pin. Width spans both socket edges,
    # so ornament loads bear against the shell rim, not the cavity floor.
    return _slab(10.0, 2.0, 4.0, y=1.0, r=0.45)


def _bed(shape: cq.Workplane, front_up: bool = True) -> cq.Workplane:
    # Back-face-down puts cosmetic recesses on top, fully open to the nozzle.
    result = shape.mirror("XY") if front_up else shape
    bb = result.val().BoundingBox()
    return result.translate((-(bb.xmin + bb.xmax) / 2,
                             -(bb.ymin + bb.ymax) / 2, -bb.zmin))


def _placed(shape: cq.Workplane, x: float) -> cq.Workplane:
    p = INTERFACE
    return shape.translate((x, p.seat_y, p.center_z))


def _part(shape: cq.Workplane, *, x: float, color: str, variant: str,
          role: str = "accessory", front_up=True,
          description="", print_shape: cq.Workplane | None = None) -> dict[str, Any]:
    placed = _placed(shape, x)
    if variant == "eyes":
        placed = placed.translate((0, -.9, 0))
    return {
        "shape": placed,
        "print_shape": print_shape or _bed(shape, front_up),
        "color": color,
        "role": role,
        "variant": variant,
        "explode": [0.0, 22.0, 0.0],
        "description": description,
        "quantity": 1,
        "supports": variant == "eyes" and role == "accessory",
        "requires_glue": role == "accessory-accent" or variant == "eyes" and role == "accessory",
    }


def _ear(sign: int) -> tuple[cq.Workplane, cq.Workplane]:
    # Outward splay is in the ornament plane: the stem stays straight/keyed.
    angle = -sign * 10.0
    decoration = (_slab(8.8, 30.8, 4.0, y=17.0, r=4.35)
                  .rotate((0, 0, 0), (0, 0, 1), angle))
    root = _slab(5.8, 5.0, 4.0, y=2.5, r=0.5)
    white = decoration.union(root).union(_stop()).union(pin())
    cavity = (_slab(3.05, 21.1, 0.76, y=17.4, z=-2.01, r=1.5)
              .rotate((0, 0, 0), (0, 0, 1), angle))
    accent = (_slab(2.7, 20.75, 0.55, y=17.4, z=-1.93, r=1.32)
              .rotate((0, 0, 0), (0, 0, 1), angle))
    return white.cut(cavity), accent


def _antenna(sign: int) -> tuple[cq.Workplane, cq.Workplane]:
    angle = -sign * 25.0
    arm = _slab(3.8, 41.0, 4.0, y=20.4, r=1.85)
    tip = _disc(6.0, 4.0, y=39.5)
    arm = arm.union(tip).rotate((0, 0, 0), (0, 0, 1), angle)
    root = _slab(5.8, 4.4, 4.0, y=2.2, r=0.45)
    white = arm.union(root).union(_stop()).union(pin())
    cavity = (_disc(4.55, 0.76, y=39.5, z=-2.01)
              .rotate((0, 0, 0), (0, 0, 1), angle))
    accent = (_disc(4.2, 0.55, y=39.5, z=-1.93)
              .rotate((0, 0, 0), (0, 0, 1), angle))
    return white.cut(cavity), accent


def _eye(sign: int) -> tuple[cq.Workplane, cq.Workplane]:
    # The original concept uses a curved stem and a rounded eye. A hidden
    # flat rear cuts the sphere at Z=2, giving the whole part a stable,
    # print plane while retaining its rounded visible front. Only the hidden
    # glue peg below the root needs support.
    path = (cq.Workplane("XY").moveTo(0, 0)
            .spline([(sign*1.4, 3.2), (sign*1.7, 6.2),
                     (sign*.7, 8.8), (0, 11.8)], includeCurrent=True))
    stalk = cq.Workplane("XZ").circle(1.7).sweep(path)
    globe = cq.Workplane("XY").sphere(6.0).translate((0, 16, 0))
    rear_clip = _slab(30, 30, 22, y=16, z=-20)
    peg = cq.Workplane("XZ").circle(1.3).extrude(3.5)
    white = globe.intersect(rear_clip).union(stalk).union(peg)
    # The iris has a flat glue face and a gently rounded rim. The depressed
    # centre makes a shadow pupil using only the owned purple PLA.
    iris_x = .25
    cavity = _disc(5.0, 3.65, x=iris_x, y=16, z=-8.0)
    iris = _disc(4.7, .8, x=iris_x, y=16, z=-5.25)
    iris = iris.edges("<Z").fillet(.24)
    # A real blind well behind the iris creates the dark pupil with two PLA
    # colours. The iris remains an open ring rather than a shallow purple cup.
    iris = iris.cut(_disc(2.0, 1.0, x=iris_x, y=16, z=-5.35))
    pupil_well = _disc(2.0, 5.1, x=iris_x, y=16, z=-4.5)
    return white.cut(cavity).cut(pupil_well), iris


def _eye_foot(x: float) -> cq.Workplane:
    """A removable purple socket cap lets the round white stem emerge directly.

    The hidden white peg glues into this keyed purple foot; the installed top
    is cut from the authoritative body surface, exactly like its blank cap.
    """
    import build as geometry
    p = geometry.P
    z = INTERFACE.center_z
    exterior = geometry.cap_exterior()
    region = (cq.Workplane("XZ").center(x, z).rect(7.8, 5.8)
              .extrude(4).edges("|Y").fillet(p["accessory"].get("cap_corner_radius", 2.0)).translate((0, 34, 0)))
    # Crop first: whole-loft subtraction can leave incorrect remote faces
    # after the front rim is filleted. The visible cap must stay in its region.
    skin = exterior.intersect(region).cut(exterior.translate((0, -.55, 0)))
    foot = skin.union(geometry.keyed_pin(x, 31.1, z, 3.9))
    # The stem bends immediately above its glue peg; give that shallow
    # swept root clearance without widening the structural peg bore.
    stem_clear = (cq.Workplane("XZ").center(x, z).circle(2.15)
                  .extrude(1.4).translate((0, 32.5, 0)))
    peg_clear = (cq.Workplane("XZ").center(x, z).circle(1.5)
                 .extrude(4.2).translate((0, 31.2, 0)))
    return foot.cut(stem_clear).cut(peg_clear).clean()


def _collar() -> cq.Workplane:
    # A cosmetic through-key shim fills the main builder's shallow cap pocket.
    # It remains 0.05 mm below the rim and cannot preload the socket bottom.
    plate = (cq.Workplane("XZ").rect(7.8, 5.8).extrude(0.55)
             .edges("|Y").fillet(_DESIGN.get("cap_corner_radius", 2.0)))
    bore = keyed_profile(0.2).extrude(0.65).translate((0, 0.05, 0))
    return plate.cut(bore).translate((0, -0.05, 0))


def _collar_bed(shape: cq.Workplane) -> cq.Workplane:
    rotated = shape.rotate((0, 0, 0), (1, 0, 0), 90)
    bb = rotated.val().BoundingBox()
    return rotated.translate((0, 0, -bb.zmin))


def accessory_parts() -> dict[str, dict[str, Any]]:
    """Return all optional individual print parts, with installed local poses.

    Select exactly one variant for a render or assembly: ears, antennas, eyes.
    White collars are shared by ears and antennas, one per occupied socket.
    Eyes use separate curved purple keyed feet with hidden glued stem pegs.
    Purple inner-ear pads, pupils and antenna tips are optional glued accents.
    """
    parts: dict[str, dict[str, Any]] = {}
    for side, sign, x in zip(("left", "right"), (-1, 1), INTERFACE.centers_x):
        for variant, factory in (("ears", _ear), ("antennas", _antenna),
                                 ("eyes", _eye)):
            white, purple = factory(sign)
            singular = {"ears": "bunny-ear", "antennas": "tv-antenna",
                        "eyes": "eye"}[variant]
            parts[f"{singular}-{side}"] = _part(
                white, x=x, color=WHITE, variant=variant,
                description=("Rounded eye and curved stalk; hidden flat back on the bed."
                             " Supports only beneath the hidden glue peg."
                             if variant == "eyes" else
                             "Flat, back-face-down; pin and ornament root lie in print XY."))
            parts[f"{singular}-accent-{side}"] = _part(
                purple, x=x, color=PURPLE, variant=variant,
                role="accessory-accent", front_up=False,
                description="Optional purple inset; small glue dots, no interference fit.")
        collar = _collar()
        parts[f"accessory-collar-{side}"] = _part(
            collar, x=x, color=WHITE, variant="shared-accessory",
            print_shape=_collar_bed(collar),
            description="Optional thin flush collar, threaded onto pin before insertion.")
        foot = _eye_foot(x)
        parts[f"eye-foot-{side}"] = {
            "shape": foot,
            "print_shape": _bed(foot.rotate((0, 0, 0), (1, 0, 0), 90), front_up=False),
            "color": PURPLE,
            "role": "accessory", "variant": "eyes", "quantity": 1,
            "supports": True, "requires_glue": False, "explode": [0, 12, 0],
            "description": "Curved purple keyed cap; glue the white stem peg into its bore.",
        }
    return parts


def _coupon_socket(clearance: float) -> cq.Workplane:
    # Socket points down print Z: no suspended roof. 5.6 mm frame gives a
    # closed 0.6 mm floor, plus the specified 5 mm socket depth.
    frame = _slab(12.0, 10.0, 5.6, z=0.0, r=1.0)
    cut = keyed_profile(clearance).extrude(5.0)
    cut = cut.rotate((0, 0, 0), (1, 0, 0), 90).translate((0, 0, 5.6))
    return frame.cut(cut)


def fit_coupon_parts() -> dict[str, dict[str, Any]]:
    """One labelled socket strip and one identical 4.5 mm pin for trial fit."""
    strip = _slab(67.0, 24.0, 1.2, y=-3.5, z=0.0, r=1.5)
    clearances = (0.10, 0.15, 0.20, 0.25)
    for x, clearance in zip((-25.5, -8.5, 8.5, 25.5), clearances):
        socket = _coupon_socket(clearance).translate((x, 0, 1.2))
        strip = strip.union(socket)
        # Positive embossed labels sit outside the fit surfaces. Values name
        # per-side outward offsets, not the total change in socket dimensions.
        label = (cq.Workplane("XY").workplane(offset=1.2).center(x, -10.0)
                 .text(f"{clearance:.2f}", 3.1, 0.4, combine=False))
        strip = strip.union(label)
    test_pin = pin().union(_stop())
    return {
        "accessory-fit-socket-strip": {
            "shape": strip, "print_shape": strip, "color": WHITE,
            "role": "coupon", "variant": "coupon", "quantity": 1,
            "supports": False, "explode": [0, 0, 0],
            "description": "0.10 / 0.15 / 0.20 / 0.25 mm PER-SIDE socket offsets."},
        "accessory-fit-test-pin": {
            "shape": test_pin, "print_shape": _bed(test_pin),
            "color": PURPLE, "role": "coupon", "variant": "coupon",
            "quantity": 1, "supports": False, "explode": [0, 0, 0],
            "description": "Same 6×4 keyed, 4.5 mm deep pin as all ornaments."},
    }


def validate_accessories() -> dict[str, Any]:
    """Small direct checks used by the main validation/build entrypoint."""
    result: dict[str, Any] = {"interface": INTERFACE.__dict__, "parts": {}}
    for name, part in {**accessory_parts(), **fit_coupon_parts()}.items():
        shape, print_shape = part["shape"], part["print_shape"]
        solids = shape.solids().vals()
        bb = print_shape.val().BoundingBox()
        valid = bool(shape.val().isValid() and len(solids) == 1 and abs(bb.zmin) < 1e-6)
        if not valid:
            raise ValueError(f"Invalid accessory or bed pose: {name}")
        result["parts"][name] = {
            "valid": valid, "solids": len(solids),
            "volume_mm3": round(shape.val().Volume(), 3),
            "print_bounds_mm": [round(bb.xlen, 3), round(bb.ylen, 3), round(bb.zlen, 3)],
            "print_z_min_mm": round(bb.zmin, 6),
            "color": part["color"], "variant": part["variant"],
            "supports": part["supports"],
        }
    result["physical_fit_required"] = True
    return result
