#!/usr/bin/env python3
"""Separate, bed-grown mounting yoke for the failed r3 integral base posts.

Reuses the plinth, keeper bars and four cover inserts. The body is raised 5 mm.
For r5, exports a rear cover with the physically requested pillar relief.
Export is isolated from the printed r3 files. Install two inserts in this yoke;
remove the remnants of both failed integral posts before fitting it.
"""
from pathlib import Path
import hashlib
import json
import math
import sys
import cadquery as cq
import trimesh

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import build as b

R5 = b.P.get('rear_pillar_shortening', 0.0) > 0
R7 = bool(b.P.get('usb_baffle'))
REVISION = 'r7-internal-usb-baffle' if R7 else ('r5-rear-pillar-fit' if R5 else 'r4-separate-mount-repair')
COVER_NAME = 'rear-cover-internal-usb' if R7 else 'rear-cover-short-pillars'
OUT = ROOT / 'output' / ('internal-usb-r7' if R7 else ('rear-pillar-fit-r5' if R5 else 'mount-repair-r4'))
OUT.mkdir(exist_ok=True)
LIFT = 5.0
P = b.P

# A broad 4-mm foot grows directly from the bed. The rear panel grows out of
# this foot at the case's 18-degree lean; no floating cylinder starts remain.
yoke = b.rounded(90, 22, 3, P['base_height'], 4.0, y=10)
rear_outer = P['shell_depth'] + P['joint_gap'] + P['rear_thickness']
panel = b.rounded(88, 12, 2, rear_outer, 3.0, y=-33)
for x in (-P['case_mount_x'], P['case_mount_x']):
    panel = panel.cut(b.cyl(P['screw_clearance'], rear_outer-.1, 3.2, x, -P['case_mount_y']))
    panel = panel.cut(b.tapered_hole(P['screw_clearance'], P['screw_countersink_diameter'],
                                   rear_outer+3.0-b.csk_depth, b.csk_depth+.01,
                                   x, -P['case_mount_y']))
yoke = yoke.union(b.pose(panel).translate((0, 0, LIFT)))
for x in (-P['base_mount_x'], P['base_mount_x']):
    yoke = yoke.union(b.cyl(P['insert_boss_diameter'], P['base_height'], 11.1,
                          x, P['base_mount_y']))
    yoke = yoke.cut(b.cyl(P['insert_pilot'], P['base_height']-.1,
                        P['insert_socket_depth']+.1, x, P['base_mount_y']))
    relief_start=P['base_height']+P['insert_socket_depth']-.1
    yoke = yoke.cut(b.cyl(3.2, relief_start, P['base_tip_relief_end']-relief_start,
                        x, P['base_mount_y']))
    yoke = yoke.cut(b.tapered_hole(P['insert_pilot']+.7, P['insert_pilot'],
                                P['base_height']-.01, P['insert_leadin']+.01,
                                x, P['base_mount_y']))

front = b.pose(b.shell_without_base_mounts).translate((0,0,LIFT))
rear = b.pose(b.rear).translate((0,0,LIFT))
keepers = [b.pose(k).translate((0,0,LIFT)) for k in b.keepers]
native = cq.importers.importStep(str(ROOT/'reference/esp32-s3-touch-lcd-2_8.stp'))
hardware = b.pose(native.rotate((0,0,0),(1,-1,0),180)
                  .translate((0,0,.7+P['front_recess']+P['standoff_shim']))).translate((0,0,LIFT))
plug = b.pose(b.box(22,12.8,7.6,49.278,.06,4.47+P['standoff_shim'])).translate((0,0,LIFT))

# Leave 0.25-mm clearance around the surviving lower case wall. The sockets
# stay behind it; only their unused outer crown is relieved.
front_clear=front
for delta in ((.25,0,0),(-.25,0,0),(0,.25,0),(0,-.25,0),(0,0,.25),(0,0,-.25)):
    front_clear=front_clear.union(front.translate(delta))
yoke=yoke.cut(front_clear)

validation = {'revision':REVISION, 'body_lift_mm':LIFT,
              'reuses':['r4 mounting yoke', 'r3 plinth', 'r3 keeper-lower', 'r3 keeper-upper'] if R5 else ['r3 rear-cover', 'r3 plinth', 'r3 keeper-lower', 'r3 keeper-upper'],
              'physical_fit':'r3 physically obstructed/compressed the device; requested 2.45 mm relief applied, r5 physical refit pending' if R5 else 'not yet tested', 'checks':[], 'parts':{}}
for x in (-P['base_mount_x'],P['base_mount_x']):
    wall_envelope=b.cyl(P['insert_outer_diameter_nominal']+3.2, P['base_height'],
                        P['insert_length_nominal'],x,P['base_mount_y'])
    overlap=wall_envelope.intersect(front_clear).val().Volume()
    validation['checks'].append({'name':f'base insert at x={x:g}, minimum 1.6-mm wall',
                                'interference_mm3':round(overlap,6)})
    if overlap>.1: raise RuntimeError(f'Insufficient base insert wall: {overlap}')

for name, part in [('bare front shell',front),('rear cover with shortened pillars' if R5 else 'unchanged rear cover',rear),
                   ('plinth',b.base),('lower keeper',keepers[0]),('upper keeper',keepers[1]),
                   ('official V1 hardware',hardware),('sample USB cable',plug)]:
    overlap = yoke.intersect(part).val().Volume()
    validation['checks'].append({'name':'repair yoke vs '+name, 'interference_mm3':round(overlap,6)})
    if overlap > .1:
        raise RuntimeError(f'Repair yoke overlaps {name}: {overlap:.4f} mm3')

for name, part in [('mount-repair-yoke',yoke.translate((0,0,-P['base_height']))),
                   ('front-shell-no-integral-posts',b.shell_without_base_mounts)]+([(COVER_NAME,b.rear.rotate((0,0,0),(1,0,0),180))] if R5 else []):
    bb=part.val().BoundingBox()
    printable=part.translate((-bb.xmin,-bb.ymin,-bb.zmin))
    cq.exporters.export(printable,str(OUT/(name+'.stl')),tolerance=.07,angularTolerance=.12)
    cq.exporters.export(printable,str(OUT/(name+'.step')))
    mesh=trimesh.load_mesh(OUT/(name+'.stl'))
    record={'valid_brep':bool(printable.val().isValid()),'solids':len(printable.solids().vals()),
            'watertight_stl':bool(mesh.is_watertight),'volume_cm3':round(float(mesh.volume)/1000,3),
            'size_mm':[round(float(v),3) for v in mesh.extents]}
    validation['parts'][name]=record
    if not record['valid_brep'] or record['solids']!=1 or not record['watertight_stl'] or mesh.volume<=0:
        raise RuntimeError(f'Invalid repair part: {name}: {record}')

# Confirm that the rear-cover change removes material only inside the four
# original round-pillar footprints. The rim, ribs, fences and holes stay exact.
if R5:
    original=cq.importers.importStep(str(ROOT/'output/rear-cover.step'))
    # Compare the pillar-only geometry here; the USB walls are checked below.
    rotated=b.rear_without_usb_baffle.rotate((0,0,0),(1,0,0),180)
    bb=rotated.val().BoundingBox()
    updated=rotated.translate((-bb.xmin,-bb.ymin,-bb.zmin))
    added=updated.cut(original).val().Volume()
    removed=original.cut(updated)
    footprint=None
    for x,y in P['board_standoff_centers']:
        volume=b.cyl(P['board_cup_outer_diameter']+.001,0,30,x,y)
        footprint=volume if footprint is None else footprint.union(volume)
    rotated=b.rear_without_usb_baffle.rotate((0,0,0),(1,0,0),180)
    bb=rotated.val().BoundingBox()
    footprint=footprint.rotate((0,0,0),(1,0,0),180).translate((-bb.xmin,-bb.ymin,-bb.zmin))
    elsewhere=removed.cut(footprint).val().Volume()
    expected=4*math.pi*(P['board_cup_outer_diameter']/2)**2*P['rear_pillar_shortening']
    removed_volume=removed.val().Volume()
    if added>.001 or elsewhere>.001 or abs(removed_volume-expected)>.01:
        raise RuntimeError(f'Unexpected rear-cover delta: added={added}, elsewhere={elsewhere}, removed={removed_volume}, expected={expected}')
    validation['checks'].append({'name':'only four round rear pillars shortened',
        'pillar_front_before_mm':8.2,'pillar_front_after_mm':8.2+P['rear_pillar_shortening'],
        'seating_stop_before_mm':P['standoff_rear_depth']+P['standoff_shim']+P['board_rear_stop_clearance'],
        'seating_stop_after_mm':P['standoff_rear_depth']+P['standoff_shim']+P['board_rear_stop_clearance']+P['rear_pillar_shortening'],
        'shortening_mm':P['rear_pillar_shortening'],'removed_mm3':round(removed_volume,6),
        'added_mm3':round(added,6),'changed_outside_round_pillars_mm3':round(elsewhere,6)})
    nominal_overlap=b.rear.intersect(b.unpose(hardware.translate((0,0,-LIFT)))).val().Volume()
    if nominal_overlap>.1: raise RuntimeError(f'New cover nominal hardware overlap: {nominal_overlap}')
    validation['checks'].append({'name':'shortened rear cover vs official V1 STEP, nominal only',
                                'interference_mm3':round(nominal_overlap,6)})

if R7:
    baffle = b.pose(b.usb_baffle).translate((0,0,LIFT))
    for name, part in [('printed front shell',front),('official V1 electronics',hardware),
                       ('lower keeper',keepers[0]),('upper keeper',keepers[1]),
                       ('repair yoke',yoke),('plinth',b.base),('sample straight plug',plug)]:
        overlap=baffle.intersect(part).val().Volume()
        validation['checks'].append({'name':'Internal USB baffle vs '+name,'interference_mm3':round(overlap,6)})
        if overlap > .1: raise RuntimeError(f'Internal USB baffle overlaps {name}: {overlap:.4f} mm3')
    # A 25-mm continuous swept envelope covers axial insertion, not just the
    # plug's seated pose. It uses the same nominal mould as the r3 fit check.
    insertion=b.pose(b.box(47,12.8,7.6,61.778,.06,4.47+P['standoff_shim'])).translate((0,0,LIFT))
    overlap=baffle.intersect(insertion).val().Volume()
    if overlap>.1: raise RuntimeError('Internal USB baffle obstructs nominal plug insertion')
    added=b.rear.cut(b.rear_without_usb_baffle).val().Volume()
    removed=b.rear_without_usb_baffle.cut(b.rear).val().Volume()
    if removed>.01: raise RuntimeError('Internal USB baffle removed existing cover geometry')
    # All added material must stay inside the original enclosure envelope.
    inside=b.box(P['body_width'],P['body_height'],rear_outer,z=0)
    outside=b.usb_baffle.cut(inside).val().Volume()
    if outside>.01: raise RuntimeError('USB baffle extends outside the case')
    port_overlap=b.usb_baffle.intersect(b.usb_port).val().Volume()
    if port_overlap>.01: raise RuntimeError('USB baffle narrows the existing port passage')
    validation['checks'].append({'name':'Internal baffle preserves entire original USB opening passage',
                                'interference_mm3':round(port_overlap,6)})
    # Front edge is 0.55 mm above the shell floor and 0.62 mm beyond
    # the official module's widest glass edge; outer edge is 0.45 mm
    # inside the existing side wall. No press-fit contact is intended.
    validation['usb_baffle']={'wall_mm':P['usb_baffle']['wall'],
        'channel_length_mm':P['usb_baffle']['x_outer']-P['usb_baffle']['x_inner'],
        'passage_height_mm':P['usb_baffle']['passage_height'],
        'shell_side_clearance_mm':P['body_width']/2-P['wall']-P['usb_baffle']['x_outer'],
        'front_floor_clearance_mm':P['usb_baffle']['front_depth']-4.1,
        'glass_edge_clearance_mm':P['usb_baffle']['x_inner']-39.53,
        'outside_case_mm3':round(outside,6),
        'sample_plug_housing_mm':[12.8,7.6],'axial_insertion_sweep_mm':25,
        'insertion_interference_mm3':round(overlap,6),
        'added_material_mm3':round(added,3),'removed_material_mm3':round(removed,6),
        'attachment':'integral with rear cover; entirely internal; existing cover screws',
        'physical_fit':'nominal CAD clearance verified, physical dry fit pending'}

# Every component in every 0.20-mm slice must grow from the previous layer.
# This catches the original posts' floating starts instead of relying on
# overall solid validity or the mere presence of slicer supports.
from shapely.geometry import Polygon
def check_layer_continuity(filename, layer_height):
    mesh=trimesh.load_mesh(OUT/filename)
    previous=None
    layers=0
    minimum_contact=None
    for z in __import__('numpy').arange(.10,float(mesh.bounds[1,2]),layer_height):
        section=mesh.section(plane_origin=[0,0,float(z)],plane_normal=[0,0,1])
        if section is None: continue
        footprint=None
        for path in section.discrete:
            polygon=Polygon(path[:,:2]).buffer(0)
            footprint=polygon if footprint is None else footprint.symmetric_difference(polygon)
        if footprint is None or footprint.is_empty: continue
        polygons=list(footprint.geoms) if footprint.geom_type=='MultiPolygon' else [footprint]
        if previous is not None:
            for polygon in polygons:
                contact=polygon.intersection(previous).area
                if contact<.01: raise RuntimeError(f'Floating island in {filename} at Z={z:.2f}')
                minimum_contact=contact if minimum_contact is None else min(minimum_contact,contact)
        previous=footprint
        layers+=1
    validation['checks'].append({'name':f'{filename} {layer_height:.2f}-mm layer components grow from preceding layer',
                                'layers_checked':layers,'floating_components':0,
                                'minimum_contact_area_mm2':round(minimum_contact,3)})
check_layer_continuity('mount-repair-yoke.stl', .20)
if R7: check_layer_continuity(COVER_NAME+'.stl', .16)

# Screw length is DIN965 head-inclusive. The added 3-mm panel moves the tips
# rearward, while still reaching past the front end of each 5.7-mm insert.
insert_front=P['shell_depth']-P['insert_length_nominal']
for head in (P['screw_head_diameter_min_checked'],P['screw_head_diameter_max']):
    recess=(P['screw_countersink_diameter']-head)/2
    tip=rear_outer+3-recess-P['screw_length']
    engagement=min(P['insert_length_nominal'],max(0,P['shell_depth']-tip))
    validation['checks'].append({'name':f'lower cover M3x16 engagement, head diameter {head:g}',
                                 'engagement_mm':round(engagement,3),
                                 'tip_clearance_mm':round(tip-P['case_tip_relief_bottom'],3)})
    if engagement < P['insert_length_nominal']-.01 or tip<P['case_tip_relief_bottom']+.5:
        raise RuntimeError('Lower cover screw engagement or blind-hole clearance failed')
base_clearance=P['base_tip_relief_end']-(P['screw_length']+
                  (P['screw_countersink_diameter']-P['screw_head_diameter_min_checked'])/2)
if base_clearance<.5: raise RuntimeError('Base screws may bottom out')
validation['checks'].append({'name':'unchanged M3x16 base screws', 'minimum_tip_clearance_mm':round(base_clearance,3),
                             'engagement_mm':P['insert_length_nominal']})

assembly = cq.Assembly(name='Glimdock-'+REVISION)
for name, part in [('front-shell-no-posts',front),('rear-cover-r7' if R7 else ('rear-cover-r5' if R5 else 'rear-cover-r3'),rear),('plinth-r3',b.base),
                   ('mount-repair-yoke',yoke),('keeper-lower-r3',keepers[0]),('keeper-upper-r3',keepers[1])]:
    assembly.add(part,name=name)
assembly.save(str(OUT/'glimdock-repaired-assembly.step'))
validation['source_sha256']={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in (ROOT/'build.py',ROOT/'design.json',ROOT/'tools/repair_mount.py',ROOT/'tools/board_keepers.py',ROOT/'tools/usb_baffle.py')}
validation['sha256']={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(OUT.glob('*.stl'))}
(OUT/'validation.json').write_text(json.dumps(validation,indent=2)+'\n')
print(json.dumps(validation,indent=2))

# Public candidate exports CAD only; no personal slicer profile is bundled.
