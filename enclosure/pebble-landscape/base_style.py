"""Real pillowed white wedge with continuous rolling corner surfaces.

The lower roll, short middle wall and upper roll share rounded-perimeter
sections. Exact purple profile sections cut the close-fitting cradle. Joint
ports, underside hatch and saddle fasteners remain in landscape_mechanics.py.
"""
import math
import cadquery as cq
from OCP.BRepOffsetAPI import BRepOffsetAPI_ThruSections

UPPER_SIDE_RADIUS = 6.5
LOWER_SIDE_RADIUS = 4.0
FRONT_UPPER_WIDTH = 1.5
FRONT_UPPER_HEIGHT = 8.0
REAR_LOWER_WIDTH = 2.0


def _loft(wires, ruled=False):
    builder=BRepOffsetAPI_ThruSections(True,ruled,1e-6)
    builder.CheckCompatibility(False)
    builder.SetMaxDegree(3)
    for wire in wires:builder.AddWire(wire.wrapped)
    builder.Build()
    q=cq.Solid(builder.Shape())
    if not q.isValid():raise RuntimeError('Invalid pillowed base loft')
    return q


def _profile(P,rounded,width,front,rear,radius,zfront,zrear):
    wire=rounded(width,rear-front,radius,0,.05,
                 x=P.get('base_center_x',3),y=(front+rear)/2).faces('<Z').wires().val()
    slope=(zrear-zfront)/(rear-front)
    transform=cq.Matrix([[1,0,0,0],[0,1,0,0],
                         [0,slope,1,zfront-slope*front],[0,0,0,1]])
    return wire.transformGeometry(transform)


def _assemble_rolls(lower,upper):
    bottom=cq.Workplane(obj=_loft(lower))
    belt=cq.Workplane(obj=_loft([lower[-1],upper[0]],ruled=True))
    shoulder=cq.Workplane(obj=_loft(upper))
    q=bottom.union(belt).union(shoulder).clean()
    if not q.val().isValid() or len(q.solids().vals())!=1:
        raise RuntimeError('Invalid continuous rolled base envelope')
    return q


def _roof_envelope(P,rounded):
    front,rear=P['base_front_y'],P['base_rear_y']
    lower=[];upper=[]
    for z in (0,.15,.4,.8,1.3,2,2.8,3.5,4):
        factor=math.sqrt(max(0,1-((z-4)/4)**2))
        si=4*(1-factor); ri=REAR_LOWER_WIDTH*(1-factor)
        lower.append(_profile(P,rounded,P['base_width']-2*si,
                              front+si,rear-ri,P['base_radius']-si,z,z))
    crest=P.get('base_cradle_height',15.9)
    front_upper_height=min(FRONT_UPPER_HEIGHT,crest-LOWER_SIDE_RADIUS-.2)
    if front_upper_height<3:
        raise RuntimeError('Cradle crest too low for the selected front rolls')
    for degrees in (0,10,20,30,40,50,60,70,80,87,90):
        phi=math.radians(degrees);amount=1-math.cos(phi)
        si=UPPER_SIDE_RADIUS*amount
        fi=FRONT_UPPER_WIDTH*amount;ri=UPPER_SIDE_RADIUS*amount
        zfront=crest-front_upper_height+front_upper_height*math.sin(phi)
        zrear=P['base_rear_height']-UPPER_SIDE_RADIUS+UPPER_SIDE_RADIUS*math.sin(phi)
        upper.append(_profile(P,rounded,P['base_width']-2*si,
                              front+fi,rear-ri,P['base_radius']-si,zfront,zrear))
    return _assemble_rolls(lower,upper)


def make_base_exterior(P,pose,rounded,box,wire_builder=None,loft_builder=None):
    outer=_roof_envelope(P,rounded)
    cx=P.get('body_center_x',3);cy=P.get('body_center_y',0)
    profiles=P.get('exterior_profiles',P.get('outer_sections'))
    if not profiles:raise RuntimeError('Exact purple exterior profile table missing')
    clear_wires=[]
    clear_profiles=[]
    for i,(z,w,h,r) in enumerate(profiles):
        if i==0:z-=.25
        elif i==len(profiles)-1:z+=.25
        # Inclined shoulders need a larger radial clearance to retain
        # the specified 0.25 mm measured normal gap after seating.
        radial=P.get('cradle_radial_margin',.33)
        wire=(wire_builder(w+2*radial,h+2*radial,r+radial,z,y=cy,x=cx)
              if wire_builder else
              rounded(w+2*radial,h+2*radial,r+radial,z,.05,x=cx,y=cy).faces('<Z').wires().val())
        clear_wires.append(wire)
        clear_profiles.append((z,w+2*radial,h+2*radial,r+radial))
    # Match body_exterior's degree3, compatible-wire loft exactly.
    clear=(loft_builder(clear_profiles,y=cy,x=cx) if loft_builder
           else cq.Workplane(obj=_loft(clear_wires)))
    front_roll=P.get('front_edge_fillet',0)
    if front_roll:
        # Offset the external round by the same 0.25 mm as its front plane
        # and perimeter; the extra 0.01 mm radial margin is retained.
        clear=clear.faces('<Z').edges().fillet(front_roll+.25)
    body_clear=pose(clear)
    # A low compact body has a concealed tail cut away below its seating
    # floor. Preserve that same floor when carving the white cradle.
    clip_z=P.get('body_hidden_tail_clearance_z')
    if clip_z is not None:
        keep=cq.Workplane('XY').box(200,200,150,centered=(True,True,False)).translate((0,20,clip_z))
        body_clear=body_clear.intersect(keep).clean()
    base=outer.cut(body_clear).clean()
    if not base.val().isValid() or len(base.solids().vals())!=1:
        raise RuntimeError('Invalid purple seating cut in rolled base')
    return base,body_clear


def make_base_inner(P,rounded):
    """Matching rolled interior: floor/side2.4, conservative curved roof."""
    floor=P['base_floor'];wall=P['base_roof']+.2
    front,rear=P['base_front_y'],P['base_rear_y']
    lower=[];upper=[]
    lower_radius=LOWER_SIDE_RADIUS-floor
    front_wall=front+4.8;rear_wall=rear-2.8
    for z in (floor,floor+.05,floor+.15,floor+.35,floor+.65,floor+1.0,4):
        factor=math.sqrt(max(0,1-((z-4)/lower_radius)**2))
        si=LOWER_SIDE_RADIUS-lower_radius*factor
        lower.append(_profile(P,rounded,P['base_width']-2*si,
                              front_wall,rear_wall,P['base_radius']-si,z,z))
    side_radius=UPPER_SIDE_RADIUS-wall
    rear_radius=UPPER_SIDE_RADIUS-2.8
    crest=P.get('base_cradle_height',15.9)
    outer_top_front=front+FRONT_UPPER_WIDTH
    outer_top_rear=rear-UPPER_SIDE_RADIUS
    top_slope=(P['base_rear_height']-crest)/(outer_top_rear-outer_top_front)
    inner_top_front=front_wall+side_radius
    # The forward outer sweep is an ellipse, so a lower inner ceiling
    # conservatively clears its steeper normals near the upper side roll.
    roof_drop=3.5
    top_front=crest+top_slope*(inner_top_front-outer_top_front)-roof_drop
    top_rear=P['base_rear_height']-roof_drop
    front_radius_height=min(side_radius,top_front-LOWER_SIDE_RADIUS-.1)
    if front_radius_height<2:
        raise RuntimeError('Compact front has insufficient cavity height')
    for degrees in (0,10,20,30,40,50,60,70,80,87,90):
        phi=math.radians(degrees);amount=1-math.cos(phi)
        si=wall+side_radius*amount
        f=front_wall+side_radius*amount
        b=rear_wall-rear_radius*amount
        zfront=top_front-front_radius_height+front_radius_height*math.sin(phi)
        zrear=top_rear-rear_radius+rear_radius*math.sin(phi)
        upper.append(_profile(P,rounded,P['base_width']-2*si,f,b,
                              P['base_radius']-si,zfront,zrear))
    return _assemble_rolls(lower,upper)
