"""Landscape Pebble mechanical construction; dimensions remain nominal.

All exterior geometry comes from build.py. This module adds separately
printable structural joints without changing the concept silhouette.
"""
import math
import cadquery as cq

def cord_guide_profile(P, z, depth, upper_y):
    """Exact U with a semicircular floor and short straight cheeks."""
    cg=P['cord_guide']; cx=P['body_center_x']; b=cg['bottom_local_y']; r=cg['width']/2
    return (cq.Workplane('XY').workplane(offset=z)
            .moveTo(cx-r,b+r).threePointArc((cx,b),(cx+r,b+r))
            .lineTo(cx+r,upper_y).lineTo(cx-r,upper_y).close().extrude(depth))

def add_cord_guide(P, api, base, base_exterior):
    """Rounded rear-joint guide with hidden load-bearing floor and cheeks."""
    if not P.get('cord_guide'):return base
    box,pose,unpose=(api[n] for n in ('box','pose','unpose'))
    cg=P['cord_guide']; bottom=cg['bottom_local_y']; top=cg['top_local_y']
    rz=cg['front_local_z']; depth=cg['depth']; cx=P['body_center_x']
    pad=pose(box(cg['reinforcement_width'],top-bottom+cg['floor_wall'],depth-.25,cx,(top+bottom-cg['floor_wall'])/2,rz+.25))
    base=base.union(pad.intersect(base_exterior))
    # A real half-circle, not a rounded rectangle: the short straight cheeks
    # leave a 4 mm visible mouth with a 3.5 mm radius bottom.
    guide=cord_guide_profile(P,rz-.05,depth+.1,bottom+10)
    base=base.cut(pose(guide)).clean()
    local=unpose(base)
    mouth=[]
    for edge in local.edges().vals():
        b=edge.BoundingBox()
        if (abs(b.zmin-(P['rear_outer']+.25))<.0001 and b.zlen<.0001
                and b.xmin>=cx-cg['width']/2-.001 and b.xmax<=cx+cg['width']/2+.001
                and b.ymin>=bottom-.001 and b.ymax<=top+1.0):mouth.append(edge)
    if mouth:
        base=pose(local.newObject(mouth).fillet(cg['mouth_roll'])).clean()
    else:
        raise RuntimeError('Cord-guide mouth edges not found')
    return base


def make_geometry(P, api, g):
    box,rounded,cyl,cone,pose,bed,keyed_pin,hex_prism,extent_shape = [api[n] for n in ('box','rounded','cyl','cone','pose','bed','keyed_pin','hex_prism','extent_shape')]
    M=api['M']; A=api['A']; ORIGIN_Z=api['ORIGIN_Z']
    cx=P['body_center_x']; exterior=api['trim_hidden_tail'](api['body_exterior'](P['body_overall_depth']))
    body=g['local_parts']['body']; rear=g['local_parts']['rear-cover']
    base=g['world_parts']['base']; base_exterior=api['make_base_exterior_for_mechanics']()[0]
    body_clear=api['make_base_exterior_for_mechanics']()[1]
    local={k:v for k,v in g['local_parts'].items() if k.startswith('blank-cap') or k.startswith('rear-screw-cap')}

    # Four manufacturer posts define the main3.2-mm plate seating plane.
    carrier=rounded(68,47,4,M,P['carrier_thickness'],x=2.9)
    carrier=carrier.cut(rounded(52,33,3,M-.1,P['carrier_thickness']+.2,x=2.9))
    # The right frame segment leaves room for the replaceable socket sled;
    # both right post footprints and all four M2.5 seats stay complete.
    carrier=carrier.cut(box(12.4,22.0,P['carrier_thickness']+.2,33.2,0,M-.1))
    carrier_nuts=[]; carrier_bolts=[]; carrier_washers=[]
    ear_top=M+4.0; nut_z=M+.6; bridge_rear=ear_top+2.5
    underhead=bridge_rear+.5
    for x,y in P['carrier_ears']:
        sign=1 if y>0 else -1
        ear=rounded(9.4,7.2,1.1,M,4.0,x=x,y=y)
        carrier=carrier.union(ear)
        pocket=hex_prism(5.8,nut_z,2.4,x,y)
        entry=box(6.8,12,2.4,x,y-sign*5,nut_z)
        carrier=carrier.cut(pocket).cut(entry).cut(cyl(3.4,M-.1,4.2,x,y))
        carrier_nuts.append(hex_prism(5.5,nut_z,2.4,x,y).cut(cyl(3.0,nut_z-.1,2.6,x,y)))
        carrier_bolts.append(cyl(3.0,underhead-8,8,x,y).union(cyl(6.0,underhead,2.52,x,y)))
        carrier_washers.append(cyl(7.0,bridge_rear,.5,x,y).cut(cyl(3.2,bridge_rear-.1,.7,x,y)))
    for x,y in P['stock_posts']:carrier=carrier.cut(cyl(2.9,M-.1,3.4,x,y))
    # A small outer-edge relief clears the lower insert bosses, retaining the
    # complete factory-post bearing circles beside it.
    for sign in (-1,1):
        x=35.1 if sign>0 else -29.3
        carrier=carrier.cut(box(3,9,3.4,x+sign*1.5,-13,M-.1))
    carrier=carrier.clean()
    carrier_clearance=rounded(68.3,47.3,4.15,M-.15,P['carrier_thickness']+.15,x=2.9)
    for x,y in P['carrier_ears']:
        carrier_clearance=carrier_clearance.union(rounded(9.7,7.5,1.25,M-.15,4.15,x=x,y=y))

    # Body bridges bear on the thicker carrier ears. The side-entry nut sits
    # in the carrier, so no blind front screw can press on the factoryglass.
    for x,y in P['carrier_ears']:
        sign=1 if y>0 else -1
        yp=sign*30.8; toe=y-sign*3.6
        profile=[(yp,8),(sign*31.9,8),(sign*31.9,bridge_rear),(toe,bridge_rear),(toe,ear_top)]
        ramp=cq.Workplane('YZ').polyline(profile).close().extrude(9.4).translate((x-4.7,0,0))
        # Clear the actual plate below its bearing face, preserving direct
        # face contact at ear_top. Hidden underside support is required.
        ramp=ramp.cut(carrier_clearance)
        bridge=rounded(9.4,7.2,1.1,ear_top,2.5,x=x,y=y)
        body=body.union(ramp).union(bridge).cut(cyl(3.4,ear_top-.1,2.8,x,y))

    # Eight owned Ruthex inserts: body/saddle, rear cover, fixed base/feet,
    # and independent hatch. The four shell pilots have broad grounded ramps.
    for positions,face,start in ((P['lower_mount_bosses'],P['lower_boss_face'],6.0),(P['upper_cover_bosses'],P['upper_boss_face'],8.0)):
        for x,y in positions:
            sign=1 if x>cx else -1
            wallx=cx+sign*(P['body_width']/2-P['wall']/2)
            floor=face-P['insert_depth']; toe=x-sign*P['boss_diameter']/2
            if positions is P['upper_cover_bosses']:
                pts=[(30.8,start),(30.8,face),(y-P['boss_diameter']/2,face),(y-P['boss_diameter']/2,floor)]
                ramp=cq.Workplane('YZ').polyline(pts).close().extrude(P['boss_diameter']).translate((x-P['boss_diameter']/2,0,0))
                for px,py in P['stock_posts']:
                    ramp=ramp.cut(cyl(5.6,M+3.1,2.5,px,py))
            else:
                pts=[(wallx,start),(wallx,face),(toe,face),(toe,floor)]
                ramp=cq.Workplane('XZ').polyline(pts).close().extrude(P['boss_diameter']).translate((0,y+P['boss_diameter']/2,0))
            # Carrier clearance is solid, including its captive-nut entries.
            # Insert roots must not grow through that separately fitted plate.
            ramp=ramp.cut(carrier_clearance)
            body=body.union(ramp).union(cyl(P['boss_diameter'],floor,P['insert_depth'],x,y))
            body=body.cut(cyl(P['insert_pilot'],floor,P['insert_depth']+.1,x,y))
            body=body.cut(cone(4,4.7,face-.35,.36,x,y)).cut(cyl(3.2,face-9,2.5,x,y))

    # Tabs enter rearward after the module is removed from the white cradle.
    face=P['lower_boss_face']
    for x,y in P['lower_mount_bosses']:
        body=body.cut(box(7.6,9.8,P['body_overall_depth']-face+.2,x,y,face))

    # Accessory receivers remain at the new TOP, not the rotated old rightside.
    ac=P['accessory']
    crown=api['cap_exterior']()
    for x in ac['centers_x']:
        receiver=box(9.6,7.4,6.4,x,28.3,14.0)
        root=cq.Workplane('YZ').polyline([(29.6,9),(32,9),(32,14),(24.6,14)]).close().extrude(9.6).translate((x-4.8,0,0))
        body=body.union(receiver).union(root)
        socket=keyed_pin(x,32.05,ac['center_z'],ac['socket_depth']+.05,ac['side_clearance'])
        body=body.cut(socket)
        region=box(ac['cap_pocket_width'],4,ac['cap_pocket_depth'],x,32,ac['center_z']-ac['cap_pocket_depth']/2).edges('|Y').fillet(ac.get('pocket_corner_radius',1.2))
        pocket=crown.intersect(region).cut(crown.translate((0,-.7,0)))
        body=body.cut(pocket)

    # Replaceable U sled captures the provisional female housing at the right
    # side. Its closed body mouth masks all electronics and its front wall
    # retains the housing; the rear stop carries cable insertion forces.
    mouth=P['usb_socket_mouth_x']; fl=P['female_usb_nominal']['length']; zc=P['usb_socket_center_z']
    housing=box(fl,12,6.2,mouth-fl/2,0,zc-3.1)
    housing_clear=box(fl+.25,12.5,6.6,mouth-fl/2-.125,0,zc-3.3)
    body=body.cut(housing_clear)
    clamp=box(fl+1.2,15.6,8.4,mouth-(fl+1.2)/2,0,zc-4.2)
    cavity=box(fl+.2,12.5,6.6,mouth-fl/2+.05,0,zc-3.3)
    clamp=clamp.cut(cavity)
    # Aperture through the rear stop passes the FPC instead of the plugbody.
    clamp=clamp.cut(box(1.5,10.3,3.3,mouth-fl-.7,0,zc-1.65))
    # Sidekeys sit in robust body rails; no screw load reaches the PCB socket.
    for sy in (-1,1):
        key=box(10,1.2,1.0,mouth-fl/2,sy*8.1,zc-4.2)
        clamp=clamp.union(key)
        rail=box(14.4,2.0,3.1,mouth-7.2,sy*9.0,zc-4.4)
        body=body.union(rail).cut(box(10.8,1.6,1.4,mouth-fl/2-.1,sy*8.1,zc-4.4))
    body=body.cut(box(fl+1.5,15.9,8.7,mouth-(fl+1.5)/2,0,zc-4.35))
    # The actual angled maleplug and strainreliefs are a physicalgate. The
    # route pocket is internal; it never opens a board-sized externalwindow.
    male_bay=box(9.4,13.0,8.2,41.8,0,4.8).intersect(exterior.translate((-1.2,0,0)))
    body=body.cut(male_bay)
    # A full sleeve around each foot insert is more important than the
    # decorative hidden purple tail. Relieve that tail along the separate
    # saddle's rearward assembly path, keeping the visible skin intact.
    for fx,fy in P['saddle_base_fasteners']:
        y0,y1=fy-6.4,fy+6.4; z0,z1=2.2,P['saddle_tail_clearance_top_z']
        dz=P['saddle_service']['rearward_mm']
        dy=dz*math.sin(A); drop=-dz*math.cos(A)
        swept=cq.Workplane('YZ').polyline([(y0,z0),(y0+dy,z0+drop),(y1+dy,z0+drop),(y1+dy,z1+drop),(y1,z1),(y0,z1)]).close().extrude(11.2).translate((fx-5.6,0,0))
        body=body.cut(api['unpose'](swept))
    body=body.intersect(exterior).clean()
    clamp=clamp.intersect(exterior).clean()
    local.update(body=body,rear_cover=rear)
    local.pop('rear_cover')
    local['rear-cover']=rear
    local['pcb-carrier']=carrier
    local['usb-clamp']=clamp

    # Broad separate saddle halves carry the display independent of the hatch.
    saddles={}; caps={}; bearing=P['saddle_port_caps'].get('bearing_z',12.5)
    feet=P['saddle_base_fasteners']
    for i,(x,y) in enumerate(P['lower_mount_bosses']):
        sign=1 if x>cx else -1; label='right' if sign>0 else 'left'
        fx,fy=feet[i]; foot_top=2.4+P.get('saddle_foot_height',8.3)
        foot=box(10.8,12.4,foot_top-2.4,fx,fy,2.4)
        foot=foot.cut(cyl(4,2.4,6.7,fx,fy)).cut(cone(4.7,4,2.4,.35,fx,fy))
        # Keep the complete Ruthex sleeve; relieve only the independently
        # fitted carrier and real screw heads above that sleeve.
        foot=foot.cut(pose(carrier_clearance))
        for px,py in P['stock_posts']:
            foot=foot.cut(pose(cyl(5.6,M+3.1,2.5,px,py)))
        tab=api['saddle_tab'](x,y); worldtab=pose(tab)
        topy=P.get('saddle_gusset_tab_y',y-4.8); front=face; back=face+2.4
        fw=fy+3.2; bw=fy+5.6
        tf=(topy*math.cos(A)+front*math.sin(A),topy*math.sin(A)-front*math.cos(A)+ORIGIN_Z)
        tb=(topy*math.cos(A)+back*math.sin(A),topy*math.sin(A)-back*math.cos(A)+ORIGIN_Z)
        gusset=cq.Workplane('YZ').polyline([(fw,foot_top),(bw,foot_top),tb,tf]).close().extrude(7.4).translate((x-3.7,0,0))
        gusset=gusset.cut(pose(body)).cut(pose(rear))
        saddle=foot.union(gusset).union(worldtab)
        saddle=saddle.cut(pose(carrier_clearance))
        for px,py in P['stock_posts']:
            saddle=saddle.cut(pose(cyl(5.6,M+3.1,2.5,px,py)))
        saddle=saddle.cut(pose(cyl(3.4,face-.1,2.7,x,y))).cut(pose(cone(3.4,5.8,face+1.2,1.21,x,y)))
        saddles['saddle-'+label]=saddle.clean()
        portcy=fy+.5
        base=base.cut(box(11.2,14.2,55,fx,portcy,2.4))
        base=base.cut(cyl(3.4,-.1,2.6,fx,fy)).cut(cone(6.2,3.4,-.01,1.41,fx,fy))
        if P['saddle_port_caps'].get('count',2)==0:continue
        port=box(11.2,14.2,4,fx,portcy,bearing-2.2)
        ledge=box(13.4,16.4,2.0,fx,portcy,bearing-2).cut(port).cut(body_clear)
        base=base.union(ledge.intersect(base_exterior))
        base=base.cut(box(13.4,16.4,25,fx,portcy,bearing))
        skin=base_exterior.intersect(box(13.1,16.1,15,fx,portcy,bearing))
        tongue=box(10.9,13.9,1.6,fx,portcy,bearing-1.6).cut(body_clear)
        cap=skin.union(tongue)
        slot=pose(box(30,110,4.4,x-sign*11.1,-32,face-.2))
        caps['saddle-port-cap-'+label]=cap.cut(slot).clean()

    # Independent flush underside hatch and pack tray in the rear reserve.
    compact=P.get('base_variant')=='usb-compact'
    bc=P.get('hatch_center_y',P['battery_center_y']); bx=P.get('hatch_center_x',cx)
    hw=P.get('hatch_width',46);hh=P.get('hatch_height',36);hr=P.get('hatch_radius',2)
    hatch_open=rounded(hw,hh,hr,-.1,2.5,x=bx,y=bc)
    hatch=rounded(hw-.5,hh-.5,hr-.25,0,2.4,x=bx,y=bc)
    for x,y in P['hatch_fasteners']:
        sign=1 if x>cx else -1; wing=cx+(x-cx)*.85
        hatch=hatch.union(box(15,9,2.4,wing,y,0)).union(cyl(8,0,2.4,x,y))
        hatch_open=hatch_open.union(box(15.5,9.5,2.5,wing,y,-.1)).union(cyl(8.5,-.1,2.5,x,y))
        boss=cyl(8.2,2.4,9.6,x,y)
        reach=P['base_width']/2-abs(x-cx)+.5
        web=box(reach,9.2,9.6,x+sign*reach/2,y,2.4).intersect(base_exterior)
        base=base.union(boss).union(web)
        base=base.cut(cyl(4,2.4,6.7,x,y)).cut(cone(4.7,4,2.4,.35,x,y))
        hatch=hatch.cut(cyl(3.4,-.1,2.6,x,y)).cut(cone(6.2,3.4,-.01,1.41,x,y))
    base=base.cut(hatch_open).clean(); hatch=hatch.clean()
    tray=rounded(43.2,33.2,1.2,2.4,.8,x=bx,y=bc)
    rim=rounded(43.2,33.2,1.2,3.2,3.2,x=bx,y=bc).cut(rounded(40.8,30.8,.3,3.1,3.5,x=bx,y=bc))
    tray=tray.union(rim).cut(box(5,3.2,4,bx,bc+16.4,3.2).edges('|Y').fillet(.65))
    for x in (bx-21,bx+21):
        for y in (bc-8,bc+8):tray=tray.cut(box(3,3,2,x,y,3.6))
    nominalpack=box(40,30,6,bx,bc,3.9)
    world={'base':base,'cable-hatch' if compact else 'battery-hatch':hatch,**saddles,**caps}
    if not compact:world['battery-tray']=tray.clean()
    assembly={**{n:pose(q) for n,q in local.items()},**world}
    printed={**{n:bed(q) for n,q in local.items()},**{n:bed(q,floor_z=0 if n=='base' else None) for n,q in world.items()}}
    printed['rear-cover']=bed(rear.rotate((0,0,0),(1,0,0),180))
    for n,q in local.items():
        if n.startswith('blank-cap'):printed[n]=bed(q.rotate((0,0,0),(1,0,0),90))
    printed.update({
        'coupon-glass-seat':bed(body.intersect(box(100,80,2.0,cx,0,0))),
        'coupon-carrier-posts':bed(carrier.intersect(box(11,55,4,-27.1,0,M-.1))),
        'coupon-carrier-nut':bed(carrier.intersect(box(12,13,4.2,P['carrier_ears'][0][0],P['carrier_ears'][0][1],M-.1))),
        'coupon-mount-seat':bed(body.intersect(box(13,15,22,*P['lower_mount_bosses'][1],0))),
        'coupon-saddle-head':bed(api['saddle_tab'](*P['lower_mount_bosses'][1])),
        'coupon-insert':bed(cyl(10,0,9).cut(cyl(4,2.3,6.8)).cut(cone(4,4.7,8.65,.36))),
        'coupon-accessory-socket':bed(body.intersect(box(12,10,9,ac['centers_x'][1],28,13.5))),
    })
    if not compact:printed['battery-dummy']=bed(nominalpack)
    from accessories import accessory_parts,fit_coupon_parts,validate_accessories
    accessory_data={**accessory_parts(),**fit_coupon_parts()}
    for n,r in accessory_data.items():printed[n]=r['print_shape']
    m25=[cyl(5,M+3.2,2.12,x,y) for x,y in P['stock_posts']]
    def compound(items):return cq.Workplane(obj=cq.Compound.makeCompound([q.val() for q in items]))
    g.update(local_parts=local,world_parts={**world,'usb-clamp':pose(clamp)},assembly_parts=assembly,printable_parts=printed,
             envelopes_local={'carrier-nuts':compound(carrier_nuts),'carrier-screws':compound(carrier_bolts),'carrier-washers':compound(carrier_washers),'m25-heads':compound(m25)},
             envelopes_world={**({} if compact else {'battery-reserve':extent_shape(P['battery_reserve']),'future-pack-placeholder':nominalpack}),'female-usb-reserve':pose(housing_clear),'female-usb-nominal-housing':pose(housing)},
             accessory_parts=accessory_data,accessory_validation=validate_accessories(),
             screw_axes={'module_M2.5':P['stock_posts'],'carrier_M3':P['carrier_ears'],'cover_M3':P['upper_cover_bosses'],'saddle_to_body_M3':P['lower_mount_bosses'],'base_to_saddle_M3_world':P['saddle_base_fasteners'],'hatch_M3_world':P['hatch_fasteners']},
             contact_exceptions=['carrier on four factory posts','carrier ear tops contact bodybridge underside','saddle tabs contact body boss faces','saddle feet on basefloor','port caps on fixed baseledges','tray on independent hatch'])
    g['metadata'].update(status=P['status'],carrier_hardware=P['carrier_hardware'],saddle_port_caps=P['saddle_port_caps'],rear_cover_service=P['rear_cover_service'],saddle_head_fit=P['saddle_head_fit'],print_gate=P['print_gate'])
    poses={}
    for n,q in {**local,**world}.items():
        rotated=q.rotate((0,0,0),(1,0,0),90) if n.startswith('blank-cap') else q.rotate((0,0,0),(1,0,0),180) if n=='rear-cover' else q
        bb=rotated.val().BoundingBox()
        poses[n]={'source_frame':'local' if n in local else 'world','rotation_xyz_degrees':[90,0,0] if n.startswith('blank-cap') else [180,0,0] if n=='rear-cover' else [0,0,0], 'print_translation_mm':[-bb.xmin,-bb.ymin,0 if n=='base' else -bb.zmin]}
    g['print_pose_metadata']=poses
    return g
