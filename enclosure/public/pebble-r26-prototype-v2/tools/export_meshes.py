# SPDX-FileCopyrightText: 2026 Glimdock contributors
# SPDX-License-Identifier: MIT
"""Export the public analytic CAD into geometry-only STL meshes.

Install cadquery and run: python tools/export_meshes.py --output generated-stl
Meshes retain source coordinates. Orient them and review supports in a slicer.
"""
from pathlib import Path
import argparse
import cadquery as cq


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('generated-stl'))
    parser.add_argument('--linear-tolerance', type=float, default=.025)
    parser.add_argument('--angular-tolerance', type=float, default=.08)
    args = parser.parse_args()
    if args.linear_tolerance <= 0 or args.angular_tolerance <= 0:
        parser.error('Tolerances must be positive.')
    args.output.mkdir(parents=True, exist_ok=True)
    cad = Path(__file__).resolve().parents[1] / 'cad'
    sources = {p.stem: p for p in cad.glob('*.step') if p.stem != 'pebble-own-parts-assembly'}
    sources.update({p.stem: p for p in cad.glob('*.brep')})
    for name, path in sorted(sources.items()):
        shape = (cq.Shape.importBrep(str(path)) if path.suffix == '.brep'
                 else cq.importers.importStep(str(path)).val())
        if not shape.isValid():
            raise ValueError(f'Invalid CAD: {path.name}')
        cq.exporters.export(shape, str(args.output / f'{name}.stl'),
                            tolerance=args.linear_tolerance,
                            angularTolerance=args.angular_tolerance)
        print(name)


if __name__ == '__main__':
    main()
