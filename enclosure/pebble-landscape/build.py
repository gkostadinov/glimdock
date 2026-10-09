#!/usr/bin/env python3
"""Editable landscape Pebble CAD; importing never writes files.

Local frame: X screen width, Y up, Z rearward. The manufacturer part is
rotated about Y by 180 degrees, translated behind the front glass, then
rotated about Z by 90 degrees. World Y is rearward and world Z is up.
Every fit result is nominal; physical gates in design.json remain open.
"""
from pathlib import Path
import json, math, hashlib, sys
import cadquery as cq
import numpy as np
from scipy.interpolate import PchipInterpolator
from OCP.Geom import Geom_BSplineSurface
from OCP.gp import gp_Pnt
from OCP.TColgp import TColgp_Array2OfPnt
from OCP.TColStd import TColStd_Array1OfReal,TColStd_Array1OfInteger,TColStd_Array2OfReal
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace,BRepBuilderAPI_Sewing,BRepBuilderAPI_MakeSolid
from OCP.TopoDS import TopoDS
from OCP.BRepLib import BRepLib
import trimesh

ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path: sys.path.insert(0,str(ROOT))
P = json.loads((ROOT/'design.json').read_text())
OUT = ROOT/'output'
M = P['front_recess'] + P['glass_to_post_rear']
A = math.radians(90-P['tilt_degrees'])
ORIGIN_Z = P['body_lowest_world_z'] - (P['body_center_y']-P['body_height']/2)*math.sin(A) + P['body_overall_depth']*math.cos(A)

def box(w,h,d,x=0,y=0,z=0):
    return cq.Workplane('XY').box(w,h,d,centered=(True,True,False)).translate((x,y,z))

def rounded(w,h,r,z,d,x=0,y=0):
    return cq.Workplane('XY').workplane(offset=z).center(x,y).rect(w,h).extrude(d).edges('|Z').fillet(r)

def rounded_wire(w,h,r,z,y=0,x=0):
    """Canonical eight-edge section, bottom tangent first, counterclockwise."""
    a,b=w/2,h/2
    pts=[(-a+r,-b),(a-r,-b),(a,-b+r),(a,b-r),
         (a-r,b),(-a+r,b),(-a,b-r),(-a,-b+r)]
    k=r/math.sqrt(2)
    mids=[(a-r+k,-b+r-k),(a-r+k,b-r+k),
          (-a+r-k,b-r+k),(-a+r-k,-b+r-k)]
    def v(p):return cq.Vector(p[0]+x,p[1]+y,z)
    edges=[]
    for i in range(4):
        start=2*i;end=(start+1)%8;after=(start+2)%8
        edges.append(cq.Edge.makeLine(v(pts[start]),v(pts[end])))
        edges.append(cq.Edge.makeThreePointArc(v(pts[end]),v(mids[i]),v(pts[after])))
    return cq.Wire.assembleEdges(edges)

def rounded_loft(profiles,y=0,x=0):
    """Exact monotone rounded skin, without global loft approximation.

    Width, height and radius are shape-preserving cubic Hermite functions
    of actual depth. Eight rational tensor-product surfaces express the
    straight sides and exact circular corners; their depth coordinate is
    linear, so intermediate sections cannot fold or ring past the envelope.
    """
    p=np.asarray(profiles,dtype=float)
    # A constant front land needs only its endpoint sections. Removing its
    # redundant knots also leaves the front-edge fillet on a simple patch.
    first=0
    while first+1<len(p) and np.max(np.abs(p[first+1,1:]-p[0,1:]))<1e-10:first+=1
    if first>1:p=np.concatenate([p[:1],p[first:]],axis=0)
    interp=PchipInterpolator(p[:,0],p[:,1:],axis=0)
    deriv=interp.derivative()
    controls=[]
    for i in range(len(p)-1):
        z0,z1=p[i,0],p[i+1,0];step=z1-z0
        f0,f1=p[i,1:],p[i+1,1:]
        bez=[(z0,f0),(z0+step/3,f0+deriv(z0)*step/3),
             (z1-step/3,f1-deriv(z1)*step/3),(z1,f1)]
        controls.extend(bez if i==0 else bez[1:])
    vk=TColStd_Array1OfReal(1,len(p));vm=TColStd_Array1OfInteger(1,len(p))
    for i,z in enumerate(p[:,0],1):
        vk.SetValue(i,float(z));vm.SetValue(i,4 if i in (1,len(p)) else 3)
    sew=BRepBuilderAPI_Sewing(1e-6)
    # CCW profile edge order matches rounded_wire: line, arc, repeated.
    for edge in range(8):
        count=3 if edge%2 else 2;degree=count-1
        poles=TColgp_Array2OfPnt(1,count,1,len(controls))
        weights=TColStd_Array2OfReal(1,count,1,len(controls))
        for j,(z,dims) in enumerate(controls,1):
            w,h,r=map(float,dims);a,b=w/2,h/2
            ends=[(-a+r,-b),(a-r,-b),(a,-b+r),(a,b-r),
                  (a-r,b),(-a+r,b),(-a,b-r),(-a,-b+r)]
            if edge%2:
                corner=[(a,-b),(a,b),(-a,b),(-a,-b)][edge//2]
                points=[ends[edge],corner,ends[(edge+1)%8]]
            else:points=[ends[edge],ends[(edge+1)%8]]
            for k,(px,py) in enumerate(points,1):
                poles.SetValue(k,j,gp_Pnt(px+x,py+y,float(z)))
                weights.SetValue(k,j,math.sqrt(.5) if count==3 and k==2 else 1.0)
        uk=TColStd_Array1OfReal(1,2);um=TColStd_Array1OfInteger(1,2)
        uk.SetValue(1,0);uk.SetValue(2,1);um.SetValue(1,degree+1);um.SetValue(2,degree+1)
        surface=Geom_BSplineSurface(poles,weights,uk,vk,um,vm,degree,3,False,False)
        sew.Add(BRepBuilderAPI_MakeFace(surface,1e-7).Face())
    for i in (0,-1):
        z,w,h,r=p[i];face=BRepBuilderAPI_MakeFace(rounded_wire(float(w),float(h),float(r),float(z),y=y,x=x).wrapped).Face()
        if i==0:face.Reverse()
        sew.Add(face)
    sew.Perform()
    shell=TopoDS.Shell_s(sew.SewedShape())
    solid=BRepBuilderAPI_MakeSolid(shell).Solid()
    BRepLib.OrientClosedSolid_s(solid)
    q=cq.Workplane(obj=cq.Solid(solid))
    if not q.val().isValid():raise RuntimeError('Invalid exact rounded profile surface')
    return q

def shoulder_offset(z):
    t=max(0,min(1,z/P['front_roll_depth']))
    return P['front_roll_inset']*(3*t*t-2*t*t*t)

def body_exterior(depth):
    """Continuous crowned exterior with a flat, self-supporting front edge.

    The authoritative profile table includes the broad front shoulder and
    crowned rear contour. The complete factory glass remains planar.
    """
    profiles=P['outer_sections']
    # One exact rational skin expresses the crowned shoulder directly.
    # Its monotone depth profile avoids global loft interpolation ringing.
    q=rounded_loft(profiles,P['body_center_y'],P['body_center_x']).clean()
    if P.get('front_edge_fillet'):
        q=q.faces('<Z').edges().fillet(P['front_edge_fillet'])
    return q

def cap_exterior():
    """Exact unchanged crown surface without unrelated front-edge blends.

    Crop this source before thin skin Booleans. The top fit regions occupy
    local Z13.8–19.6, far away from the front edge and concealed tail.
    """
    return rounded_loft(P['outer_sections'],P['body_center_y'],P['body_center_x']).clean()

def body_cavity(depth):
    wall=P['wall']
    profiles=[]
    for z,w,h,r in P['outer_sections']:
        if 2<=z<=depth:
            profiles.append((z,max(w-2*wall,P['rear_open_width']) if z>=20 else w-2*wall,max(h-2*wall,P['rear_open_height']) if z>=20 else h-2*wall,r-wall))
    return rounded_loft(profiles,P['body_center_y'],P['body_center_x']).clean()

def cyl(d,z,dz,x=0,y=0):
    return cq.Workplane('XY').workplane(offset=z).center(x,y).circle(d/2).extrude(dz)

def cone(d1,d2,z,dz,x=0,y=0):
    return cq.Workplane('XY').workplane(offset=z).center(x,y).circle(d1/2).workplane(offset=dz).circle(d2/2).loft()

def pose(q):
    return q.mirror('XY').rotate((0,0,0),(1,0,0),90-P['tilt_degrees']).translate((0,0,ORIGIN_Z))

def unpose(q):
    return q.translate((0,0,-ORIGIN_Z)).rotate((0,0,0),(1,0,0),P['tilt_degrees']-90).mirror('XY')

def trim_hidden_tail(q):
    """Crop only the concealed tail against the fixed white-base floor."""
    floor=P.get('body_hidden_tail_trim_z')
    if floor is None:return q
    return q.intersect(unpose(box(200,200,200,3,20,floor))).clean()

def key_profile():
    return [(-3,-2),(3,-2),(3,1.2),(2.2,2),(-3,2)]

def keyed_pin(x,y,z,depth=4.5,clearance=0):
    w=cq.Workplane('XZ').center(x,z).polyline(key_profile()).close()
    if clearance: w=w.offset2D(clearance,kind='intersection')
    return w.extrude(depth).translate((0,y,0))

def hex_prism(af,z,dz,x,y):
    return cq.Workplane('XY').workplane(offset=z).center(x,y).polygon(6,af/math.cos(math.pi/6)).extrude(dz)

def base_height(y):
    return P['base_front_height']+(y-P['base_front_y'])*(P['base_rear_height']-P['base_front_height'])/P['base_depth']

def extent_shape(spec):
    x,y,z=(spec[k] for k in ('x','y','z'))
    return box(x[1]-x[0],y[1]-y[0],z[1]-z[0],sum(x)/2,sum(y)/2,z[0])

def union_all(items):
    it=iter(items); result=next(it)
    for q in it: result=result.union(q)
    return result.clean()

def saddle_tab(x,y):
    """Actual fastening tab, shared by the saddle and screw-head fit coupon."""
    face=P['lower_boss_face']
    q=box(P['saddle_tab_width'],9.6,2.4,x,y,face)
    return q.cut(cyl(3.4,face-.1,2.6,x,y)).cut(cone(3.4,P['saddle_countersink'],face+1.2,1.21,x,y))

def bed(q,floor_z=None):
    b=q.val().BoundingBox()
    # The sculpted base has an exact planar floor at world Z0. OCC's
    # conservative B-spline box extends below it and must not lift the print.
    return q.translate((-b.xmin,-b.ymin,-b.zmin if floor_z is None else -floor_z))

def part_color(name):
    white={'base','cable-hatch','battery-hatch','battery-tray','usb-clamp','battery-dummy','saddle-left','saddle-right'}
    return P['finish']['base_color'] if name in white or name.startswith('saddle-port-cap') else P['finish']['body_color']

def landscape_shell():
    exterior=trim_hidden_tail(body_exterior(P['body_overall_depth']))
    body=exterior.cut(body_cavity(P['rear_inner']))
    rear_open=rounded(P['rear_open_width'],P['rear_open_height'],P['rear_open_radius'],P['rear_inner'],5,x=P['body_center_x'])
    body=body.cut(rear_open)
    # BabyStory's proven locating opening exposes the complete factory lens.
    # Only a small ledge behind its rear face supports the opaque perimeter.
    seat=rounded(P['glass_front_opening_width'],P['glass_front_opening_height'],P['glass_front_opening_radius'],-1,1+P['glass_support_z'],x=P['glass_center_x'])
    aperture=rounded(P['glass_support_opening_width'],P['glass_support_opening_height'],P['glass_support_opening_radius'],P['glass_support_z'],1.2,x=P['glass_center_x'])
    bevel=rounded_loft([(-.01,P['glass_front_opening_width']+.4,P['glass_front_opening_height']+.4,P['glass_front_opening_radius']+.2),(.3,P['glass_front_opening_width'],P['glass_front_opening_height'],P['glass_front_opening_radius'])],0,P['glass_center_x'])
    body=body.cut(seat).cut(aperture).cut(bevel)
    # Small right-side mouth for a captured extension socket, never the old
    # exposed board-sized plug bay. The connector housing remains provisional.
    port=(cq.Workplane('YZ',origin=(45.0,0,P['usb_socket_center_z'])).rect(P['usb_socket_window_width'],P['usb_socket_window_height']).extrude(4).edges('|X').fillet(1.8))
    body=body.cut(port)
    rear=rounded(P['rear_panel_width'],P['rear_panel_height'],P['rear_panel_radius'],P['rear_inner'],P['rear_thickness'],x=P['body_center_x']).edges('>Z').chamfer(.3)
    for x,y in P['upper_cover_bosses']:
        capfloor=P['rear_outer']-P['rear_screw_cap_depth']
        rear=rear.cut(cyl(3.4,20.9,3.1,x,y)).cut(cone(3.4,P['rear_screw_countersink'],capfloor-1.2,1.21,x,y))
        rear=rear.cut(cyl(P['rear_screw_cap_seat_diameter'],capfloor,.6,x,y))
    tabs=P['rear_locating_tabs']
    for x in tabs['centers_x']:
        rear=rear.union(box(tabs['width'],tabs['height'],tabs['depth'],x,tabs['center_y'],P['rear_inner']))
        body=body.cut(box(tabs['pocket_width'],tabs['pocket_height'],3.2,x,tabs['pocket_center_y'],20.7))
    # The concealed rearcover secures the socket sled after it slides into
    # its rails. Insertion forces reach this stop instead of the PCB socket.
    stop=P['usb_cover_stop']
    rear=rear.union(box(stop['thickness'],17,2.4,stop['contact_local_x']-stop['thickness']/2,0,18.6))
    if P.get('cord_guide'):
        from landscape_mechanics import cord_guide_profile
        cg=P['cord_guide']; pocket_depth=cg['rear_panel_blind_pocket_depth']
        rear=rear.cut(cord_guide_profile(P,P['rear_outer']-pocket_depth,pocket_depth+.2,cg['top_local_y']))
    return body.clean(),trim_hidden_tail(rear),exterior

def make_visual_geometry():
    """Fast first product review from real sculpted CAD, before fit release."""
    body,rear,exterior=landscape_shell()
    native=cq.importers.importStep(str(ROOT.parent/'reference/esp32-s3-touch-lcd-2_8.stp'))
    hardware=native.rotate((0,0,0),(0,1,0),180).translate((0,0,.7+P['front_recess'])).rotate((0,0,0),(0,0,1),90)
    from base_style import make_base_inner
    base,body_clear=make_base_exterior_for_mechanics()
    base=base.cut(make_base_inner(P,rounded)).clean()
    from landscape_mechanics import add_cord_guide
    base=add_cord_guide(P,globals(),base,make_base_exterior_for_mechanics()[0])
    caps={}
    ac=P['accessory']
    crown=cap_exterior()
    for i,x in enumerate(ac['centers_x']):
        socket=keyed_pin(x,ac['seat_y']+.05,ac['center_z'],ac['socket_depth']+.05,ac['side_clearance'])
        body=body.cut(socket)
        region=box(7.8,4,5.8,x,32,ac['center_z']-2.9).edges('|Y').fillet(ac.get('cap_corner_radius',1.0))
        capskin=crown.intersect(region).cut(crown.translate((0,-.55,0)))
        cap=capskin.union(keyed_pin(x,31.1,ac['center_z'],3.9))
        caps['blank-cap-left' if i==0 else 'blank-cap-right']=cap
        pocket_region=box(ac['cap_pocket_width'],4,ac['cap_pocket_depth'],x,32,ac['center_z']-ac['cap_pocket_depth']/2).edges('|Y').fillet(ac.get('pocket_corner_radius',1.2))
        body=body.cut(crown.intersect(pocket_region).cut(crown.translate((0,-.7,0))))
    for i,(x,y) in enumerate(P['upper_cover_bosses']):
        cap=cyl(P['rear_screw_cap_nominal_diameter'],P['rear_outer']-.5,.5,x,y)
        for angle in (0,120,240):
            rad=math.radians(angle)
            cap=cap.union(cyl(.25,P['rear_outer']-.4,.25,x+3.075*math.cos(rad),y+3.075*math.sin(rad)))
        caps['rear-screw-cap-left' if i==0 else 'rear-screw-cap-right']=cap.clean()
    local={'body':body,'rear-cover':rear,**caps}
    assembled={**{n:pose(q) for n,q in local.items()},'base':base}
    # A nominal metal rim makes the small port understandable in the product
    # render. It is not a purchased connector model or a printable part.
    mouth=P['usb_socket_mouth_x']; z=P['usb_socket_center_z']
    shell=(cq.Workplane('YZ',origin=(mouth-.3,0,z)).rect(8.8,3.0).extrude(.3).edges('|X').fillet(1.3))
    bore=(cq.Workplane('YZ',origin=(mouth-.4,0,z)).rect(8.4,2.6).extrude(.5).edges('|X').fillet(1.1))
    shell=shell.cut(bore)
    visual_hardware={'provisional-USB-C-metal-rim':{'shape':pose(shell),'color':'#b3b7bf','role':'metal','status':'UNMEASURED nominal 8.8×3.0 rim only'},'provisional-USB-C-tongue':{'shape':pose(box(.2,7.2,.6,mouth-.15,0,z-.3)),'color':'#25252A','role':'pcb','status':'UNMEASURED visual placeholder'}}
    return {'local_parts':local,'world_parts':{'base':base},'assembly_parts':assembled,'printable_parts':{**{n:bed(q) for n,q in local.items()},'base':bed(base)},'hardware_local':hardware,'hardware_world':pose(hardware),'envelopes_local':{},'envelopes_world':{},'accessory_parts':{},'visual_hardware':visual_hardware,'metadata':{'revision':P['revision'],'status':'GEOMETRY_DRAFT','body_origin_world_z':ORIGIN_Z,'post_rear_plane':M,'physical_gates':P['measured_gates'],'sculpted_exterior':P['sculpted_exterior'],'screen_mode':'landscape320x240','screen_texture':str(OUT/'dashboard-landscape.png')}}

_base_mechanical_cache=None
def make_base_exterior_for_mechanics():
    global _base_mechanical_cache
    if _base_mechanical_cache is None:
        from base_style import make_base_exterior
        _base_mechanical_cache=make_base_exterior(P,pose,rounded,box,wire_builder=rounded_wire,loft_builder=rounded_loft)
    return _base_mechanical_cache

def make_geometry():
    before=geometry_source_hashes()
    from landscape_mechanics import make_geometry as complete_landscape_geometry
    g=complete_landscape_geometry(P,globals(),make_visual_geometry())
    if before!=geometry_source_hashes():raise RuntimeError('Geometry sources changed during CAD construction; rebuild before caching.')
    g['geometry_source_sha256']=before
    return g

def geometry_source_hashes():
    sources={name:ROOT/name for name in ('build.py','landscape_mechanics.py','design.json','base_style.py','accessories.py')}
    sources['esp32-s3-touch-lcd-2_8.stp']=ROOT.parent/'reference'/'esp32-s3-touch-lcd-2_8.stp'
    return {name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in sources.items()}

def export_geometry_cache(g):
    """Hash-bound exact solids for an independent audit without a rebuild."""
    if g.get('geometry_source_sha256')!=geometry_source_hashes():
        raise RuntimeError('CAD cache source mismatch; build the current geometry first.')
    directory=OUT/'mechanical-draft-brep';directory.mkdir(parents=True,exist_ok=True)
    groups={key:g[key] for key in ('local_parts','world_parts','envelopes_local','envelopes_world','printable_parts')}
    groups['reference']={name:g[name] for name in ('hardware_local','hardware_world')}
    groups['accessory_shapes']={name:r['shape'] for name,r in g['accessory_parts'].items()}
    groups['visual_reference']={name:r['shape'] for name,r in g.get('visual_hardware',{}).items()}
    outer,clear=make_base_exterior_for_mechanics()
    groups['helper']={'base_exterior':outer,'body_clear':clear}
    parts=[]
    for group,items in groups.items():
        for name,q in items.items():
            file=directory/(group+'--'+name+'.brep')
            q.val().exportBrep(str(file))
            parts.append({'group':group,'name':name,'file':str(file.relative_to(OUT)), 'sha256':hashlib.sha256(file.read_bytes()).hexdigest()})
    stamp={'geometry_source_sha256':g['geometry_source_sha256'],'parts':parts,'metadata':g['metadata'],'screw_axes':g['screw_axes'],'print_pose_metadata':g.get('print_pose_metadata',{}),
           'accessory_metadata':{name:{k:v for k,v in r.items() if k not in ('shape','print_shape')} for name,r in g['accessory_parts'].items()},
           'visual_metadata':{name:{k:v for k,v in r.items() if k!='shape'} for name,r in g.get('visual_hardware',{}).items()},
           'accessory_validation':g['accessory_validation'],'contact_exceptions':g.get('contact_exceptions',[])}
    (OUT/'geometry-cache.json').write_text(json.dumps(stamp,indent=2)+'\n')
    print('Exact source-stamped geometry cache ready:',OUT/'geometry-cache.json',flush=True)

def load_cached_geometry():
    """Restore exact frozen CAD for final exports, verifying every hash."""
    stamp=json.loads((OUT/'geometry-cache.json').read_text())
    if stamp['geometry_source_sha256']!=geometry_source_hashes():raise RuntimeError('Cached CAD sources changed; rebuild first.')
    groups={}
    for item in stamp['parts']:
        path=OUT/item['file']
        if hashlib.sha256(path.read_bytes()).hexdigest()!=item['sha256']:raise RuntimeError('Cached BREP hash changed: '+str(path))
        groups.setdefault(item['group'],{})[item['name']]=cq.Workplane(obj=cq.Shape.importBrep(str(path)))
    g={key:groups[key] for key in ('local_parts','world_parts','envelopes_local','envelopes_world','printable_parts')}
    g.update(groups['reference'])
    g['assembly_parts']={**{name:pose(q) for name,q in g['local_parts'].items()},**g['world_parts']}
    for key in ('metadata','screw_axes','print_pose_metadata','accessory_validation','contact_exceptions','geometry_source_sha256'):g[key]=stamp[key]
    g['accessory_parts']={name:{**r,'shape':groups['accessory_shapes'][name],'print_shape':g['printable_parts'][name]} for name,r in stamp['accessory_metadata'].items()}
    g['visual_hardware']={name:{**r,'shape':groups['visual_reference'][name]} for name,r in stamp['visual_metadata'].items()}
    global _base_mechanical_cache
    _base_mechanical_cache=(groups['helper']['base_exterior'],groups['helper']['body_clear'])
    return g

def _mesh_record(q,name,color,role='body',explode=(0,0,0)):
    vs,fs=q.val().tessellate(.04,.1)
    return {'name':name,'role':role,'color':color,'vertices':[round(float(a),4) for v in vs for a in v.toTuple()],'faces':[int(i) for f in fs for i in f],'explode':list(explode)}

def export_draft_meshes(g):
    """Early renderable actual-CAD mesh, before slow STEP and fit audits."""
    OUT.mkdir(exist_ok=True)
    manifest={'name':'Pebble Landscape','revision':P['revision'],'status':'GEOMETRY_DRAFT','finish':P['finish'],'screen_texture':str(OUT/'dashboard-landscape.png'),'parts':[],**g['metadata']}
    for name,q in g['assembly_parts'].items():
        manifest['parts'].append(_mesh_record(q,name,part_color(name)))
    for name,record in g.get('visual_hardware',{}).items():
        item=_mesh_record(record['shape'],name,record['color'],record['role'])
        item['status']=record['status'];manifest['parts'].append(item)
    hw=g['hardware_local'].solids().vals()
    for i,role,color in ((0,'glass','#111318'),(1,'pcb','#20252B'),(2,'pcb','#28594A'),(181,'metal','#9AA1AC'),(202,'metal','#C4C7CE')):
        manifest['parts'].append(_mesh_record(pose(cq.Workplane(obj=hw[i])),f'manufacturer-{i}',color,role))
    screen=pose(box(57.6,43.2,.012,0,0,P['front_recess']-.028))
    screenrecord=_mesh_record(screen,'Active display','#121821','screen')
    uv=[]
    vertices=screenrecord['vertices']
    for i in range(0,len(vertices),3):
        x,wy,wz=vertices[i:i+3]
        localy=wy*math.cos(A)+(wz-ORIGIN_Z)*math.sin(A)
        uv.extend([round((x+28.8)/57.6,6),round((localy+21.6)/43.2,6)])
    screenrecord['uv']=uv;manifest['parts'].append(screenrecord)
    (OUT/'draft-assembly-meshes.json').write_text(json.dumps(manifest,separators=(',',':')))
    print('Draft actual-CAD mesh ready:',OUT/'draft-assembly-meshes.json',flush=True)
    return manifest

def export_all(g,only=None):
    OUT.mkdir(exist_ok=True); (OUT/'assembly-parts').mkdir(exist_ok=True)
    validation={'revision':P['revision'],'status':P['status'],'physical_gates':P['measured_gates'],'parts':{},'checks':[]}
    assembly=cq.Assembly(name='Landscape-Pebble')
    manifest={'name':'Pebble Landscape','revision':P['revision'],'status':P['status'],'finish':P['finish'],'screen_texture':str(OUT/'dashboard-landscape.png'),'parts':[],'print_parts':[],**g['metadata']}
    printmanifest={'revision':P['revision'],'status':P['status'],'parts':[]}
    for name,q in g['printable_parts'].items():
        if only is None or name in only:
            cq.exporters.export(q,str(OUT/(name+'.stl')),tolerance=.04,angularTolerance=.1)
            cq.exporters.export(q,str(OUT/(name+'.step')))
        mesh=trimesh.load_mesh(OUT/(name+'.stl'))
        r={'valid_brep':q.val().isValid(),'solids':len(q.solids().vals()),'watertight_stl':bool(mesh.is_watertight),'volume_mm3':round(mesh.volume,4),'size_mm':[round(float(v),3) for v in mesh.extents]}
        validation['parts'][name]=r
        support_regions={'body':['hidden carrier bridge and socket rail undersides; rear glass-seat ledge'], 'rear-cover':['hidden locating tab undersides'], 'base':['hidden cradle ceiling and rim'], 'usb-clamp':['inside roof of provisional extension socket tunnel']}.get(name,[])
        if name.startswith('blank-cap'):support_regions=['hidden curved cap skirt with keyed pin end on the bed']
        if name.startswith('saddle-port-cap'):support_regions=['hidden underside of locating shoulder']
        if name in g['accessory_parts'] and g['accessory_parts'][name]['supports']:support_regions=['hidden accessory glue peg or curved keyed foot underside']
        requires_support=bool(support_regions)
        release='COUPON_ONLY' if any(t in name for t in ('coupon','fit-','dummy')) else 'BLOCKED_PHYSICAL_FIT'
        pose_meta=g.get('print_pose_metadata',{}).get(name,{})
        manifest['print_parts'].append({'name':name,'stl':str(OUT/(name+'.stl')),'print_step':str(OUT/(name+'.step')),'layer_height':.16 if name in ('body','rear-cover') else .2,'support_regions':support_regions,'release':release,**pose_meta})
        color=part_color(name)
        if name in g['accessory_parts']: color=g['accessory_parts'][name]['color']
        printmanifest['parts'].append({'name':name,'filename':name+'.stl','color':color,'requires_support':requires_support,'support_regions':support_regions,'release':release,**pose_meta})
        if not r['valid_brep'] or r['solids']!=1 or not r['watertight_stl'] or mesh.volume<=0: raise RuntimeError(f'Invalid printable part {name}: {r}')
    for name,q in g['assembly_parts'].items():
        if only is None or name in only:
            cq.exporters.export(q,str(OUT/'assembly-parts'/(name+'.step')))
        color=part_color(name)
        assembly.add(q,name=name,color=cq.Color(color))
        offsets={'body':(0,-28,28),'rear-cover':(0,38,12),'pcb-carrier':(0,14,4),'base':(0,0,-20),'battery-hatch':(0,0,-48),'battery-tray':(0,0,-35),'usb-clamp':(0,25,-8),'saddle-left':(-15,0,-5),'saddle-right':(15,0,-5),'saddle-port-cap-left':(-15,0,10),'saddle-port-cap-right':(15,0,10),'blank-cap-left':(0,0,28),'blank-cap-right':(0,0,28)}
        manifest['parts'].append(_mesh_record(q,name,color,explode=offsets.get(name,(0,0,0))))
    for name,record in g.get('visual_hardware',{}).items():
        item=_mesh_record(record['shape'],name,record['color'],record['role'])
        item['status']=record['status'];manifest['parts'].append(item)
    # Exact manufacturer hardware is retained as a reference, never a printable.
    assembly.add(g['hardware_world'],name='manufacturer-reference')
    assembly.save(str(OUT/'pebble-assembly.step'))
    hw=g['hardware_local'].solids().vals()
    for i,role,color in ((0,'glass','#111318'),(1,'pcb','#20252B'),(2,'pcb','#28594A'),(181,'metal','#9AA1AC'),(202,'metal','#C4C7CE')):
        manifest['parts'].append(_mesh_record(pose(cq.Workplane(obj=hw[i])),f'manufacturer-{i}',color,role,explode=(0,-10,8)))
    for i,q in enumerate(hw):
        b=q.BoundingBox()
        if abs(b.zmax-M)<.001 and 5.0<b.xlen<5.6 and 5.0<b.ylen<5.6:
            manifest['parts'].append(_mesh_record(pose(cq.Workplane(obj=q)),f'factory-metal-post-{i}','#B1B7BD','metal',explode=(0,-10,8)))
    screen=pose(box(57.6,43.2,.012,0,0,P['front_recess']-.028))
    screenrecord=_mesh_record(screen,'Active display','#121821','screen',explode=(0,-10,8))
    # UV references the real active centre, not the offset outer glass centre.
    vertices=screenrecord['vertices']; uv=[]
    for i in range(0,len(vertices),3):
        x,wy,wz=vertices[i:i+3]
        localy=wy*math.cos(A)+(wz-ORIGIN_Z)*math.sin(A)
        uv.extend([round((x+28.8)/57.6,6),round((localy+21.6)/43.2,6)])
    screenrecord['uv']=uv
    manifest['parts'].append(screenrecord)
    for name,q in g['local_parts'].items():
        if name.startswith('blank-cap'):continue
        overlap=q.intersect(g['hardware_local']).val().Volume()
        validation['checks'].append({'name':name+' vs V1 STEP','interference_mm3':round(overlap,6)})
        if overlap>.01:raise RuntimeError(f'Hardware interference {name}: {overlap}')
    for name,q in g['envelopes_local'].items():
        overlap=q.intersect(g['hardware_local']).val().Volume()
        validation['checks'].append({'name':name+' vs V1 STEP','interference_mm3':round(overlap,6)})
        if overlap>.01:raise RuntimeError(f'Hardware envelope interference {name}: {overlap}')
    overlap=g['local_parts']['body'].intersect(g['local_parts']['pcb-carrier']).val().Volume()
    validation['checks'].append({'name':'body vs pcb-carrier','interference_mm3':round(overlap,6)})
    if overlap>.01:raise RuntimeError(f'Body/carrier interference: {overlap}')
    for reserve_name in g['envelopes_world']:
        overlap=g['world_parts']['base'].intersect(g['envelopes_world'][reserve_name]).val().Volume()
        validation['checks'].append({'name':'base vs '+reserve_name,'interference_mm3':round(overlap,6)})
        if overlap>.01:raise RuntimeError(f'Base invades reserved volume {reserve_name}: {overlap}')
    for part_name in ('base','usb-clamp'):
        overlap=g['world_parts'][part_name].intersect(g['envelopes_world']['female-usb-nominal-housing']).val().Volume()
        validation['checks'].append({'name':part_name+' vs nominal female housing','interference_mm3':round(overlap,6),'basis':'12×6.2mm published housing cross-section;18mm length remains unmeasured placeholder'})
        if overlap>.01:raise RuntimeError(f'Nominal female housing interference {part_name}: {overlap}')
    for side in ('left','right'):
        if 'saddle-port-cap-'+side not in g['world_parts']:continue
        cap=g['world_parts']['saddle-port-cap-'+side]
        for other in ('base','body','saddle-'+side):
            overlap=cap.intersect(g['assembly_parts'][other]).val().Volume()
            validation['checks'].append({'name':'saddle-port-cap-'+side+' vs '+other,'interference_mm3':round(overlap,6)})
            if overlap>.01:raise RuntimeError(f'Port cap interference {side} vs {other}: {overlap}')
        lifts=[]
        for dz in (.25,.5,1.0):
            overlap=cap.translate((0,0,dz)).intersect(g['assembly_parts']['body']).val().Volume()
            lifts.append({'upward_mm':dz,'body_block_mm3':round(overlap,6)})
        validation['checks'].append({'name':'saddle-port-cap-'+side+' body capture sweep','samples':lifts,'note':'Positive overlap after lifting means body blocks cap removal; slide body out first.'})
    (OUT/'assembly-meshes.json').write_text(json.dumps(manifest,separators=(',',':')))
    (OUT/'print-manifest.json').write_text(json.dumps(printmanifest,indent=2)+'\n')
    debug={'name':'Pebble clearance envelopes','revision':P['revision'],'parts':[]}
    for name,q in g['envelopes_world'].items():
        item=_mesh_record(q,name,'#9AABC1','reserve'); item['opacity']=.2; debug['parts'].append(item)
    (OUT/'clearance-meshes.json').write_text(json.dumps(debug,separators=(',',':')))
    accessorymanifest={'name':'Pebble accessory variants','parts':[]}
    for name,record in g['accessory_parts'].items():
        if record['variant']=='coupon': continue
        item=_mesh_record(pose(record['shape']),name,record['color'],record['role'])
        item['variant']=record['variant']; accessorymanifest['parts'].append(item)
    (OUT/'accessories-meshes.json').write_text(json.dumps(accessorymanifest,separators=(',',':')))
    (OUT/'accessory-validation.json').write_text(json.dumps(g['accessory_validation'],indent=2)+'\n')
    (OUT/'validation.json').write_text(json.dumps(validation,indent=2)+'\n')
    return validation

if __name__=='__main__':
    g=load_cached_geometry() if '--from-cache' in sys.argv else make_visual_geometry() if '--draft' in sys.argv else make_geometry()
    export_draft_meshes(g)
    if '--draft' in sys.argv:pass
    elif '--probe' in sys.argv:
        for n,q in g['printable_parts'].items():print(n,'solid',len(q.solids().vals()),'valid',q.val().isValid(),'volume',q.val().Volume(),flush=True)
        export_geometry_cache(g)
    else:
        r=export_all(g,only={'base','rear-cover'} if '--changed-only' in sys.argv else None)
        print(json.dumps({'revision':P['revision'],'status':P['status'],'parts':len(r['parts']),'checks':r['checks']},indent=2),flush=True)
