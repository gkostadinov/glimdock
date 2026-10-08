"""Parametric enclosure geometry. Default build exports the current revision.
Use --legacy-r3 only to reproduce the superseded integral-post prototype.

Body local frame: X screen width, Y up, Z into case. STL files use print poses.
Assembly/world frame: X width, Y rearward, Z up. Manufacturer V1 STEP is rotated,
never scaled: local = (-nativeY, -nativeX, 0.7-nativeZ + front_recess).
"""
from pathlib import Path
import json, math, hashlib, sys, runpy
import cadquery as cq
import numpy as np
import trimesh
from tools.board_keepers import create_keepers, keeper_sweeps

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'output'
OUT.mkdir(exist_ok=True)
parameter_file=ROOT/('design-r3.json' if '--legacy-r3' in sys.argv else 'design.json')
P = json.loads(parameter_file.read_text())

def rounded(w, h, r, z, depth, x=0, y=0):
    return (cq.Workplane('XY').workplane(offset=z).center(x,y)
            .rect(w,h).extrude(depth).edges('|Z').fillet(r))

def box(w,h,d,x=0,y=0,z=0):
    return cq.Workplane('XY').box(w,h,d,centered=(True,True,False)).translate((x,y,z))

def cyl(d,z,depth,x=0,y=0):
    return cq.Workplane('XY').workplane(offset=z).center(x,y).circle(d/2).extrude(depth)

def tapered_hole(d1,d2,z,depth,x=0,y=0):
    return (cq.Workplane('XY').workplane(offset=z).center(x,y)
            .circle(d1/2).workplane(offset=depth).circle(d2/2).loft())

csk_depth=(P['screw_countersink_diameter']-P['screw_clearance'])/2/math.tan(math.radians(P['screw_countersink_angle']/2))
head_recess=(P['screw_countersink_diameter']-P['screw_head_diameter_max'])/2/math.tan(math.radians(P['screw_countersink_angle']/2))

def pose(part):
    # Local Z points into the housing, world Y points rearward. Reflect the
    # local depth frame before rotation so the rear cover lies behind the glass.
    return part.mirror('XY').rotate((0,0,0),(1,0,0),90-P['tilt_degrees']).translate((0,0,P['body_origin_height']))

def unpose(part):
    return part.translate((0,0,-P['body_origin_height'])).rotate((0,0,0),(1,0,0),P['tilt_degrees']-90).mirror('XY')

# A front shell with a shallow glass seat; lip rests on opaque glass border only.
W,H,R,D = P['body_width'],P['body_height'],P['corner_radius'],P['shell_depth']
shell = rounded(W,H,R,0,D).edges('<Z').fillet(1.0)
shell = shell.cut(rounded(W-2*P['wall'],H-2*P['wall'],R-P['wall'],4.1,D+2))
shell = shell.cut(rounded(P['glass_width']+2*P['glass_clearance'],P['glass_height']+2*P['glass_clearance'],2.35,P['front_recess'],4.2, P['glass_center_x']))
# Full active area plus 0.8 mm per edge: touch tabs are not hidden by the bezel.
shell = shell.cut(rounded(P['aperture_width'],P['aperture_height'],0.6,-1,3))

case_mounts = [(x,y) for x in (-P['case_mount_x'],P['case_mount_x']) for y in (-P['case_mount_y'],P['case_mount_y'])]
for x,y in case_mounts:
    shell = shell.union(cyl(P['insert_boss_diameter'],3.8,D-3.8,x,y))
    socket_bottom=D-P['insert_socket_depth']
    shell = shell.cut(cyl(P['insert_pilot'],socket_bottom,P['insert_socket_depth']+.1,x,y))
    shell = shell.cut(cyl(3.2,P['case_tip_relief_bottom'],socket_bottom-P['case_tip_relief_bottom']+.1,x,y))
    shell = shell.cut(tapered_hole(P['insert_pilot'],P['insert_pilot']+.7,D-P['insert_leadin'],P['insert_leadin']+.01,x,y))

# Separate flat-printing keepers retain the board without using its M2.5 threads.
keepers=create_keepers(P)
# End guides and a low inner stop leave the outward slide path open. Bars rest
# on the cavity floor; they are installed after the display is in the shell.
bar_min=-32.2
bar_max=P['keeper_bar_x_max']
bar_width=bar_max-bar_min
bar_center=(bar_max+bar_min)/2
for sign in (-1,1):
    shell=shell.union(box(bar_width+.7,.45,.7,bar_center,sign*25.875,4.1))
    shell=shell.union(box(.95,8.3,1.6,bar_min-.15-.95/2,sign*30.25,4.1))
    shell=shell.union(box(.90,8.3,.7,bar_max+.15+.90/2,sign*30.25,4.1))

# A compact rounded USB port preserves the continuous right-side silhouette.
# An ordinary cable plugs through it; a right-angle lead can turn rearward.
usb_port=(cq.Workplane('YZ').center(.06,8.27+P['standoff_shim'])
          .rect(14.4,9.0).extrude(18).edges('|X').fillet(1.8).translate((38,0,0)))
shell = shell.cut(usb_port)
# Three restrained rear-facing vent slots near the opposite upper edge.
for x in (-18,-9,0):
    shell = shell.cut(box(5.5,7,3.0,x,H/2,13.4))

# Preserve the shell before the integral posts for a separately printed mount.
shell_without_base_mounts = shell

# Concealed, vertical base attachment columns. They stay below the glass/PCB.
shell_world = pose(shell)
for x in (-P['base_mount_x'],P['base_mount_x']):
    shell_world = shell_world.union(cyl(P['insert_boss_diameter'],P['base_height'],13.0,x,P['base_mount_y']))
    shell_world = shell_world.cut(cyl(P['insert_pilot'],P['base_height']-.1,P['insert_socket_depth']+.1,x,P['base_mount_y']))
    relief_start=P['base_height']+P['insert_socket_depth']-.1
    shell_world = shell_world.cut(cyl(3.2,relief_start,P['base_tip_relief_end']-relief_start,x,P['base_mount_y']))
    shell_world = shell_world.cut(tapered_hole(P['insert_pilot']+.7,P['insert_pilot'],P['base_height']-.01,P['insert_leadin']+.01,x,P['base_mount_y']))
shell = unpose(shell_world)

# Back plate, locating tongue, and loose cups around the stock metal barrels.
# The first physical fit required 2.45 mm less projection from each round
# pillar. Move both the mouth and blind seating face toward the rear cover;
# shortening only the rim would leave the compressive seating face unchanged.
rear_z = D+P['joint_gap']
rear = rounded(W,H,R,rear_z,P['rear_thickness']).edges('>Z').fillet(.8)
tongue = rounded(W-2*P['wall']-.5,H-2*P['wall']-.5,R-P['wall']-.25,D-1.8,2.15)
tongue = tongue.cut(rounded(W-2*P['wall']-3.0,H-2*P['wall']-3.0,R-P['wall']-1.5,D-2,3))
rear = rear.union(tongue)
for x,y in P['board_standoff_centers']:
    pillar_shortening=P.get('rear_pillar_shortening',0.0)
    stop=P['standoff_rear_depth']+P['standoff_shim']+P['board_rear_stop_clearance']+pillar_shortening
    cup_start=8.2+pillar_shortening
    rear=rear.union(cyl(P['board_cup_outer_diameter'],cup_start,rear_z+P['rear_thickness']-cup_start,x,y))
    rear=rear.cut(cyl(P['board_cup_inner_diameter'],cup_start-.1,stop-cup_start+.1,x,y))
# Rear ribs capture the keeper bars in their front seats, with a little free play.
for keeper in keepers:
    bb=keeper.val().BoundingBox()
    bar_y=(P['keeper_bar_y_min']+27.3)/2 * (1 if bb.ymax>0 else -1)
    start=bb.zmax+P['keeper_rear_capture_clearance']
    rear=rear.union(box(bar_width,27.3-P['keeper_bar_y_min'],rear_z+P['rear_thickness']-start,bar_center,bar_y,start))
    # Outer fences prevent the retainers sliding back out after closing.
    rear=rear.union(box(bar_width,.65,rear_z+P['rear_thickness']-4.2,bar_center,(27.45+.325)*(1 if bb.ymax>0 else -1),4.2))
for x,y in case_mounts:
    # Registration tongue clears the front shell's screw pillars.
    rear = rear.cut(cyl(P['insert_boss_diameter']+.5,D-2.0,2.05,x,y))
    rear = rear.cut(cyl(P['screw_clearance'],D-2,10,x,y))
    rear = rear.cut(tapered_hole(P['screw_clearance'],P['screw_countersink_diameter'],rear_z+P['rear_thickness']-csk_depth,csk_depth,x,y))
for x in (-18,-9,0):
    rear = rear.cut(box(5.5,4.5,5,x,29,D-2))
for x in (-P['base_mount_x'],P['base_mount_x']):
    # Hidden underside relief clears the two body-to-base attachment columns.
    rear = rear.cut(unpose(cyl(P['insert_boss_diameter']+.5,0,20.2,x,P['base_mount_y'])))

rear_without_usb_baffle = rear
if P.get('usb_baffle'):
    from tools.usb_baffle import create_baffle
    usb_baffle = create_baffle(P, box)
    rear = rear.union(usb_baffle)

# Low accent plinth, underside screw access, feet, and rear cord saddle.
base = rounded(P['base_width'],P['base_depth'],P['base_radius'],0,P['base_height'],y=P['base_center_y']).edges('>Z').fillet(1.4)
for x in (-P['base_mount_x'],P['base_mount_x']):
    base = base.cut(cyl(P['screw_clearance'],-.1,9,x,P['base_mount_y']))
    base = base.cut(tapered_hole(P['screw_countersink_diameter'],P['screw_clearance'],0,csk_depth,x,P['base_mount_y']))
feet=[]
for x in (-36,36):
    for y in (-8,28):
        base = base.cut(cyl(8.2,-.1,.7,x,y))
        feet.append(cyl(8,-1.6,2.2,x,y))
# Broad shallow groove at the right rear, not a friction clamp on the cable.
base = base.cut(box(9,20,3.5,35,36,3.4))

# Thin fitting gauge proves glass fit and aperture alignment before the full case.
gauge = shell.intersect(box(120,100,4.6,z=-.1))

# Six actual blind sockets with labels; choose fit using an insert from the kit.
coupon=rounded(70,16,2,0,9)
for x,pilot in zip((-28,-16.8,-5.6,5.6,16.8,28),(3.8,4.0,4.1,4.2,4.4,4.6)):
    coupon=coupon.cut(cyl(pilot,9-P['insert_socket_depth'],P['insert_socket_depth']+.1,x,2.7))
    coupon=coupon.cut(tapered_hole(pilot,pilot+.7,9-P['insert_leadin'],P['insert_leadin']+.01,x,2.7))
    label=(cq.Workplane('XY').workplane(offset=8.5).center(x,-4.3)
           .text(f'{pilot:.1f}',2.6,1,font='Arial',combine=False))
    coupon=coupon.cut(label)

parts = {'front-shell':shell_world, 'rear-cover':pose(rear), 'plinth':base,
         'keeper-lower':pose(keepers[0]),'keeper-upper':pose(keepers[1])}
printparts = {'front-shell':shell, 'rear-cover':rear.rotate((0,0,0),(1,0,0),180), 'plinth':base,
              'keeper-lower':keepers[0].rotate((0,0,0),(1,0,0),180),
              'keeper-upper':keepers[1].rotate((0,0,0),(1,0,0),180),
              'bezel-fit-gauge':gauge,'insert-fit-coupon':coupon}
# Default exports use the corrected separate mounting design. Legacy exports
# remain reproducible only when explicitly requested for comparison.
if __name__ == '__main__' and '--legacy-r3' not in sys.argv:
    runpy.run_path(str(ROOT/'tools/repair_mount.py'), run_name='__main__')
elif __name__ == '__main__':
    validation = {'revision':P['revision'], 'parts':{}, 'fit_status':'nominal CAD verified; physical fit not yet tested', 'checks':[]}
    for name,part in printparts.items():
        bb=part.val().BoundingBox()
        part=part.translate((-bb.xmin,-bb.ymin,-bb.zmin))
        cq.exporters.export(part,str(OUT/(name+'.stl')),tolerance=.07,angularTolerance=.12)
        cq.exporters.export(part,str(OUT/(name+'.step')))
        mesh=trimesh.load_mesh(OUT/(name+'.stl'))
        record={'valid_brep':bool(part.val().isValid()),'solids':len(part.solids().vals()),'watertight_stl':bool(mesh.is_watertight),'positive_volume':bool(mesh.volume>0),'size_mm':[round(float(v),2) for v in mesh.extents],'volume_cm3':round(float(mesh.volume)/1000,2)}
        validation['parts'][name]=record
        if not record['valid_brep'] or record['solids']!=1 or not record['watertight_stl'] or not record['positive_volume']:
            raise RuntimeError(f'Invalid printable solid {name}: {record}')

    assembly=cq.Assembly(name='Glimdock')
    for name,part in parts.items(): assembly.add(part,name=name)
    assembly.save(str(OUT/'glimdock-assembly.step'))

    full=cq.Compound.makeCompound([p.val() for p in parts.values()]+[f.val() for f in feet])
    bb=full.BoundingBox()
    dimensions=[round(v) for v in (bb.xlen,bb.ylen,bb.zlen)]
    manifest={'name':P['name'],'revision':P['revision'],'finish':P['finish'],
              'dimensionsLabel':' × '.join(str(d) for d in dimensions)+f" mm · {P['tilt_degrees']:g}° tilt",
              'screen_texture':str(ROOT/'reference/dashboard-preview.png'),'parts':[]}
    def add_mesh(name,part,role,color,explode=(0,0,0),uv=False):
        verts,faces=part.val().tessellate(.22,.22)
        flat=[round(float(v),3) for p in verts for v in p.toTuple()]
        item={'name':name,'role':role,'color':color,'vertices':flat,'faces':[int(v) for f in faces for v in f],'explode':list(explode)}
        if uv:
            # Pose rotates only around X: recover local Y to keep the dashboard level.
            ang=math.radians(90-P['tilt_degrees'])
            item['uv']=[round(q,5) for v in verts for q in ((v.x+28.8)/57.6,(v.y*math.cos(ang)+(v.z-P['body_origin_height'])*math.sin(ang)+21.6)/43.2)]
        manifest['parts'].append(item)
    add_mesh('Front shell',shell_world,'body',P['finish']['body_color'],(0,-21,8))
    add_mesh('Rear cover',pose(rear),'body',P['finish']['body_color'],(0,27,12))
    add_mesh('Plinth',base,'accent',P['finish']['plinth_color'],(0,0,-8))
    for i,keeper in enumerate(keepers):
        add_mesh('Lower keeper' if i==0 else 'Upper keeper',pose(keeper),'body',P['finish']['body_color'],(0,-12,-5 if i==0 else 18))

    # Manufacturer STEP is the fit reference; simplification below is only for a
    # legible viewer and material rendering, and is never exported as a print part.
    shim=P['standoff_shim']
    board=rounded(69.02,49.92,1.4,5.5+shim,1.6,2.9,.05)
    for x,y in P['board_standoff_centers']: board=board.cut(cyl(2.5,5.3+shim,2,x,y))
    glass=rounded(73.06,50.54,2,1.1+shim,.7,3)
    screen=box(57.6,43.2,.03,z=1.06+shim)
    add_mesh('PCB reference',pose(board),'pcb','#1e514a',(0,0,10))
    add_mesh('Touch glass',pose(glass),'glass','#0b1115',(0,-9,10))
    add_mesh('Display',pose(screen),'screen','#c8d5dd',(0,-9,10),uv=True)
    for i,(x,y) in enumerate(P['board_standoff_centers']):
        add_mesh(f'Stock M2.5 standoff {i+1}',pose(cyl(5.5,7.1+shim,4,x,y).cut(cyl(2.5,7+shim,4.2,x,y))),'metal','#b1b7ba',(0,0,10))
    for i,foot in enumerate(feet): add_mesh(f'Foot {i+1}',foot,'feet','#15191b',(0,0,-13))
    (OUT/'assembly-meshes.json').write_text(json.dumps(manifest,separators=(',',':')))

    # Compare printable body and cap with the actual stock board assembly.
    native=cq.importers.importStep(str(ROOT/'reference/esp32-s3-touch-lcd-2_8.stp'))
    hardware=native.rotate((0,0,0),(1,-1,0),180).translate((0,0,.7+P['front_recess']+shim))
    localparts={'front-shell':shell,'rear-cover':rear,'keeper-lower':keepers[0],'keeper-upper':keepers[1]}
    for name,part in localparts.items():
        interference=part.intersect(hardware).val().Volume()
        validation['checks'].append({'name':name+' vs official V1 STEP','interference_mm3':round(interference,5)})
        if interference>.1: raise RuntimeError(f'Hardware interference: {name} {interference} mm³')
    for name,swept in zip(('lower keeper','upper keeper'),keeper_sweeps(P,P['keeper_slide_travel'])):
        for target_name,target in (('official V1 STEP',hardware),('complete front shell',shell)):
            overlap=swept.intersect(target).val().Volume()
            validation['checks'].append({'name':f"{name} {P['keeper_slide_travel']:g} mm continuous installation sweep vs {target_name}",
                                         'interference_mm3':round(overlap,5)})
            if overlap>.1: raise RuntimeError(f'Keeper insertion blocked {name}/{target_name}: {overlap}')
    for i,(name,part) in enumerate(parts.items()):
        for othername,other in list(parts.items())[i+1:]:
            overlap=part.intersect(other).val().Volume()
            validation['checks'].append({'name':name+' vs '+othername,'interference_mm3':round(overlap,5)})
            if overlap>.1: raise RuntimeError(f'Part overlap {name}/{othername}: {overlap}')
    plug=box(22,12.8,7.6,49.278,.06,4.47+shim)
    for name,part in [('front-shell',shell),('rear-cover',rear)]:
        overlap=part.intersect(plug).val().Volume()
        validation['checks'].append({'name':name+' vs sample 12.8 × 7.6 mm straight USB mould','interference_mm3':round(overlap,5)})
        if overlap>.1: raise RuntimeError(f'USB mould overlap {name}: {overlap}')
    validation['assembled_size_mm']=[round(v,2) for v in (bb.xlen,bb.ylen,bb.zlen)]
    # DIN965 length includes the head. A smaller head seats deeper in the 90°
    # cone; check that case closure and both ends of the head range remain clear.
    worst_recess=(P['screw_countersink_diameter']-P['screw_head_diameter_min_checked'])/2
    case_margin=rear_z+P['rear_thickness']-head_recess-P['screw_length']-.05-P['case_tip_relief_bottom']
    base_margin=P['base_tip_relief_end']-(head_recess+P['screw_length'])
    worst_case=rear_z+P['rear_thickness']-worst_recess-P['screw_length']-.05-P['case_tip_relief_bottom']
    worst_base=P['base_tip_relief_end']-(worst_recess+P['screw_length'])
    validation['fasteners']={'standard':P['screw_standard'],'quantity':6,
                             'insert':P['insert_model_nominal'],'insert_pilot_mm':P['insert_pilot'],
                             'countersink_depth_mm':round(csk_depth,3),'nominal_head_recess_mm':round(head_recess,3),
                             'nominal_rear_tip_clearance_after_seating_mm':round(case_margin,3),
                             'nominal_base_tip_clearance_mm':round(base_margin,3),
                             'minimum_tip_clearance_for_checked_head_range_mm':round(min(worst_case,worst_base),3),
                             'checked_head_diameter_range_mm':[P['screw_head_diameter_min_checked'],P['screw_head_diameter_max']],
                             'nominal_thread_engagement_mm':P['insert_length_nominal']}
    if min(worst_case,worst_base)<.5: raise RuntimeError('Selected screws may bottom in insert bores')
    (OUT/'validation.json').write_text(json.dumps(validation,indent=2)+'\n')
    exports=sorted(list(OUT.glob('*.stl'))+list(OUT.glob('*.step')))
    index={'revision':P['revision'],'units':'mm','stl_pose':'flat print orientation; minimum Z=0',
           'files':[{'file':f.name,'bytes':f.stat().st_size,'sha256':hashlib.sha256(f.read_bytes()).hexdigest()} for f in exports],
           'source_sha256':{str(f.relative_to(ROOT)):hashlib.sha256(f.read_bytes()).hexdigest()
                            for f in (ROOT/'build.py',ROOT/'design.json',ROOT/'tools/board_keepers.py')}}
    (OUT/'print-files.json').write_text(json.dumps(index,indent=2)+'\n')
    print(json.dumps(validation,indent=2))
