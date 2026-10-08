"""Flat-printing board retainers for the stock Waveshare metal standoffs.

Coordinates use build.py's body-local frame: X across the screen, Y up,
and Z into the case. The lower keeper is returned before the upper keeper.
These parts leave the board's M2.5 threads unused. Bars rest on the cavity
floor and slide in from the upper/lower edge after the board is placed.

The beam's rear face is set relative to the front metal mounting faces by
keeper_front_lash. These fingers support the PCB near the mounts; the
metal-tip gap is 0.10 mm greater than forward clearance to the PCB plane.
They occupy the LCD/PCB gap, so the hardware fit check must use the same
front_recess and standoff_shim values.
"""

from collections.abc import Mapping

import cadquery as cq


def _dimensions(p: Mapping) -> dict:
    """Resolve keeper parameters without changing the caller's dictionary."""
    metal_front = (
        float(p.get("front_recess", 1.1))
        + float(p.get("standoff_shim", 0.25))
        + 0.7
        + 3.8
    )
    rear_face = metal_front - float(p.get("keeper_front_lash", 0.15))
    return {
        "x_min": float(p.get("keeper_bar_x_min", -32.2)),
        "x_max": float(p.get("keeper_bar_x_max", 33.9)),
        "y_min": float(p.get("keeper_bar_y_min", 26.25)),
        "y_max": float(p.get("keeper_bar_y_max", 27.3)),
        "bar_front": float(p.get("keeper_bar_front_depth", 4.1)),
        "rear_face": rear_face,
        "beam_front": rear_face - float(p.get("keeper_beam_thickness", 0.60)),
        "root_y": float(p.get("keeper_root_y", 26.7)),
        "right_root_shift_x": float(p.get("keeper_right_root_shift_x", -3.0)),
        "left_root_shift_x": float(p.get("keeper_left_root_shift_x", 0.0)),
        "tip_width": float(p.get("keeper_tip_beam_width", 3.6)),
        "root_width": float(p.get("keeper_root_width", 8.0)),
        "tip_diameter": float(p.get("keeper_tip_diameter", 3.8)),
    }


def _bar(d: dict, sign: int):
    x_center = (d["x_min"] + d["x_max"]) / 2
    y_center = sign * (d["y_min"] + d["y_max"]) / 2
    height = d["rear_face"] - d["bar_front"]
    return (
        cq.Workplane("XY")
        .workplane(offset=d["bar_front"])
        .center(x_center, y_center)
        .rect(
            d["x_max"] - d["x_min"],
            d["y_max"] - d["y_min"],
        )
        .extrude(height)
    )


def create_keepers(p: Mapping) -> list[cq.Workplane]:
    """Create two one-piece keeper bars in their assembled local positions.

    Flip each part about X before translating its lowest point to Z=0 for
    printing. The common rear face then puts the complete fingers on the bed.
    """
    d = _dimensions(p)
    centers = p["board_standoff_centers"]
    keepers = []
    for sign in (-1, 1):
        keeper = _bar(d, sign)
        matches = [(float(x), float(y)) for x, y in centers if y * sign > 0]
        if len(matches) != 2:
            raise ValueError("Each keeper requires two stock mounting centres")
        for x, y in matches:
            root_y = sign * d["root_y"]
            root_x = x + d["right_root_shift_x" if x > 0 else "left_root_shift_x"]
            beam = (
                cq.Workplane("XY")
                .workplane(offset=d["beam_front"])
                .polyline(
                    [
                        (x - d["tip_width"] / 2, y),
                        (x + d["tip_width"] / 2, y),
                        (root_x + d["root_width"] / 2, root_y),
                        (root_x - d["root_width"] / 2, root_y),
                    ]
                )
                .close()
                .extrude(d["rear_face"] - d["beam_front"])
            )
            tip = (
                cq.Workplane("XY")
                .workplane(offset=d["beam_front"])
                .center(x, y)
                .circle(d["tip_diameter"] / 2)
                .extrude(d["rear_face"] - d["beam_front"])
            )
            keeper = keeper.union(beam).union(tip)
        if len(keeper.solids().vals()) != 1 or not keeper.val().isValid():
            raise ValueError("Keeper must be one valid CAD solid")
        keepers.append(keeper)
    return keepers


def _convex_hull(points: list[tuple[float, float]]) -> list[tuple[float, float]]:
    """Return the boundary of a convex footprint in counterclockwise order."""
    points = sorted(set(points))

    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

    lower = []
    for point in points:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], point) <= 0:
            lower.pop()
        lower.append(point)
    upper = []
    for point in reversed(points):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], point) <= 0:
            upper.pop()
        upper.append(point)
    return lower[:-1] + upper[:-1]


def _swept_polygon(points, sign, travel, front, rear):
    footprint = _convex_hull(points + [(x, y + sign * travel) for x, y in points])
    return (
        cq.Workplane("XY")
        .workplane(offset=front)
        .polyline(footprint)
        .close()
        .extrude(rear - front)
    )


def keeper_sweeps(p: Mapping, travel: float = 6.7) -> list[cq.Workplane]:
    """Exact volumes occupied during an upper/lower outward slide.

    Each polygonal prism is swept by taking the convex hull of its footprint
    and the translated footprint. Circular tips sweep into capsules. Their
    unions cover every intermediate position, rather than sampling positions.
    The result order matches create_keepers: lower, then upper. A 6.7 mm
    outward starting offset leaves 0.29 mm beyond the nominal PCB edge.
    """
    travel = float(travel)
    if travel < 0:
        raise ValueError("Keeper outward travel cannot be negative")
    if travel == 0:
        return create_keepers(p)
    d = _dimensions(p)
    sweeps = []
    for sign in (-1, 1):
        bar = [
            (d["x_min"], sign * d["y_min"]),
            (d["x_max"], sign * d["y_min"]),
            (d["x_max"], sign * d["y_max"]),
            (d["x_min"], sign * d["y_max"]),
        ]
        sweep = _swept_polygon(bar, sign, travel, d["bar_front"], d["rear_face"])
        matches = [(float(x), float(y)) for x, y in p["board_standoff_centers"] if y * sign > 0]
        if len(matches) != 2:
            raise ValueError("Each keeper requires two stock mounting centres")
        for x, y in matches:
            root_x = x + d["right_root_shift_x" if x > 0 else "left_root_shift_x"]
            beam = [
                (x - d["tip_width"] / 2, y),
                (x + d["tip_width"] / 2, y),
                (root_x + d["root_width"] / 2, sign * d["root_y"]),
                (root_x - d["root_width"] / 2, sign * d["root_y"]),
            ]
            sweep = sweep.union(_swept_polygon(beam, sign, travel, d["beam_front"], d["rear_face"]))
            tip = (
                cq.Workplane("XY")
                .workplane(offset=d["beam_front"])
                .center(x, y)
                .circle(d["tip_diameter"] / 2)
                .extrude(d["rear_face"] - d["beam_front"])
            )
            bridge = (
                cq.Workplane("XY")
                .workplane(offset=d["beam_front"])
                .center(x, y + sign * travel / 2)
                .rect(d["tip_diameter"], travel)
                .extrude(d["rear_face"] - d["beam_front"])
            )
            sweep = sweep.union(tip).union(tip.translate((0, sign * travel, 0))).union(bridge)
        if len(sweep.solids().vals()) != 1 or not sweep.val().isValid():
            raise ValueError("Keeper sweep must be one valid CAD solid")
        sweeps.append(sweep)
    return sweeps
