#!/usr/bin/env python3
"""Export new meshes from the included analytic shapes, without device access.

SPDX-FileCopyrightText: 2026 Glimdock contributors
SPDX-License-Identifier: MIT
"""
from pathlib import Path
import argparse
import json

import cadquery as cq
import numpy as np
import trimesh

ROOT = Path(__file__).resolve().parents[1]


def shape_path(relative):
    path = (ROOT / relative).resolve()
    if not path.is_relative_to(ROOT) or not path.is_file():
        raise ValueError('The CAD source must be an included package file.')
    return path


def export(part, output, tolerance, angular_tolerance, native):
    shape = cq.Shape.importBrep(str(shape_path(part['cad']['brep'])))
    if not shape.isValid() or len(shape.Solids()) != 1:
        raise ValueError('The CAD part must be one valid solid.')
    vertices, faces = shape.tessellate(tolerance, angular_tolerance)
    # Binary STL stores float32 coordinates. Quantize before welding, just as
    # loading an exported STL does, so analytic face seams share exact vertices.
    points = np.asarray([v.toTuple() for v in vertices], dtype=np.float32).astype(float)
    mesh = trimesh.Trimesh(points, faces, process=True)
    mesh.merge_vertices(digits_vertex=7)
    mesh.update_faces(mesh.nondegenerate_faces())
    mesh.update_faces(mesh.unique_faces())
    if not native:
        transform = np.asarray(part['native_to_print'], dtype=float)
        rotation = transform[:3, :3]
        if not np.allclose(rotation.T @ rotation, np.eye(3), atol=1e-7) or np.linalg.det(rotation) < .999999:
            raise ValueError('The print pose must be a proper rigid transform.')
        mesh.apply_transform(transform)
    if not mesh.is_watertight or not mesh.is_winding_consistent or mesh.volume <= 0 or len(mesh.split()) != 1:
        raise ValueError('Mesh regeneration did not produce one closed, consistently wound part.')
    destination = output / (part['name'] + '.stl')
    mesh.export(destination)
    return {'part': part['name'], 'mesh': str(destination),
            'frame': part['coordinate_frame'] if native else 'print-Z-up',
            'triangles': len(mesh.faces), 'watertight': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--part', action='append', help='Part name; repeat for several parts. Default: all.')
    parser.add_argument('--output', type=Path, default=ROOT/'regenerated')
    parser.add_argument('--tolerance', type=float, default=.018)
    parser.add_argument('--angular-tolerance', type=float, default=.08)
    parser.add_argument('--native', action='store_true', help='Keep each part in its documented CAD frame.')
    args = parser.parse_args()
    if args.tolerance <= 0 or args.angular_tolerance <= 0:
        parser.error('Tessellation tolerances must be positive.')
    manifest = json.loads((ROOT/'parts.json').read_text())
    names = args.part or list(manifest['parts'])
    unknown = set(names)-set(manifest['parts'])
    if unknown:
        parser.error('Unknown part: '+', '.join(sorted(unknown)))
    output = args.output.resolve()
    # Canonical checked STLs remain immutable. Use a separate output folder.
    if any(output.is_relative_to((ROOT/folder).resolve()) for folder in ['stl','assembly','cad']):
        parser.error('Choose a separate regeneration folder.')
    output.mkdir(parents=True, exist_ok=True)
    for name in names:
        print(json.dumps(export(manifest['parts'][name], output,
                                args.tolerance, args.angular_tolerance, args.native)))


if __name__ == '__main__':
    main()
