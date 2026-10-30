#!/usr/bin/env python3
"""Regenerate simple illustrative volumes; these are not manufacturer CAD.

SPDX-FileCopyrightText: 2026 Glimdock contributors
SPDX-License-Identifier: MIT
"""
from pathlib import Path
import cadquery as cq

OUT = Path(__file__).resolve().parent


def slab(name, size, center, radius=0):
    part = cq.Workplane('XY').box(*size)
    if radius:
        part = part.edges('|Z').fillet(radius)
    part = part.translate(center)
    cq.exporters.export(part, str(OUT/(name+'-envelope.stl')),
                        tolerance=.01, angularTolerance=.08)


def main():
    # Native illustration frame: X right, Y up, Z towards the rear.
    slab('glass', (73.06, 50.54, .7), (3, 0, .5), 2)
    slab('display', (57.6, 43.2, .01), (0, 0, .14))
    slab('battery', (60.5, 50.2, 7.4), (3, 0, 18.2), .6)
    slab('speaker-left', (20, 30, 4.7), (-18, 8, 26.9), .8)
    slab('speaker-right', (20, 30, 4.7), (24, 8, 26.9), .8)


if __name__ == '__main__':
    main()
