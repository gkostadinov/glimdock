"""Render the published Portable r45 meshes; no geometry is reshaped."""
import argparse
import json
import math
import shutil
import sys
from pathlib import Path
import bpy
import bmesh
from mathutils import Matrix, Vector, Euler

parser=argparse.ArgumentParser()
parser.add_argument('--source',type=Path,required=True)
parser.add_argument('--out',type=Path,required=True)
parser.add_argument('--screen',type=Path,help='Path to the authored synthetic dashboard PNG; defaults to out/synthetic-screen.png.')
parser.add_argument('--screen-portrait',type=Path,help='Portrait synthetic artwork; defaults to out/synthetic-screen-portrait.png.')
parser.add_argument('--shots',default='hero,side,usb-detail,cradle-detail,exploded,portrait')
parser.add_argument('--draft',action='store_true')
args=parser.parse_args(sys.argv[sys.argv.index('--')+1:])
root=args.source.resolve(); out=args.out.resolve(); out.mkdir(parents=True,exist_ok=True)
for given,name in [(args.screen,'synthetic-screen.png'),(args.screen_portrait,'synthetic-screen-portrait.png')]:
    if given and given.resolve()!=(out/name).resolve():shutil.copyfile(given.resolve(),out/name)
manifest=json.loads((root/'viewer/scene.json').read_text())
bpy.ops.object.select_all(action='SELECT'); bpy.ops.object.delete(use_global=False)

def lin(v): return v/12.92 if v<=.04045 else ((v+.055)/1.055)**2.4
def material(name,color,rough=.43,metal=0):
    if isinstance(color,str):color=tuple(int(color[i:i+2],16)/255 for i in (1,3,5))
    m=bpy.data.materials.new(name);m.use_nodes=True
    p=m.node_tree.nodes.get('Principled BSDF')
    p.inputs['Base Color'].default_value=(*[lin(c) for c in color],1)
    p.inputs['Roughness'].default_value=rough;p.inputs['Metallic'].default_value=metal
    return m

mats={}
def getmat(p):
    role=p.get('role','body');color=p.get('color','#8251aa')
    if role=='accessory-cap':color='#8251aa'
    key=role+color
    if key not in mats:mats[key]=material(key,color,.2 if role=='glass' else .43,.22 if role=='battery' else 0)
    return mats[key]

screen=material('Synthetic dashboard','#ffffff',.4)
nodes=screen.node_tree.nodes;links=screen.node_tree.links
tex=nodes.new('ShaderNodeTexImage');tex.image=bpy.data.images.load(str(args.screen.resolve() if args.screen else out/'synthetic-screen.png'))
bsdf=nodes.get('Principled BSDF');links.new(tex.outputs['Color'],bsdf.inputs['Base Color'])
links.new(tex.outputs['Color'],bsdf.inputs['Emission Color']);bsdf.inputs['Emission Strength'].default_value=.55
bsdf.inputs['Specular IOR Level'].default_value=.15
portrait_screen=screen.copy();portrait_screen.name='Synthetic portrait dashboard'
portrait_image=bpy.data.images.load(str(args.screen_portrait.resolve() if args.screen_portrait else out/'synthetic-screen-portrait.png'))
portrait_screen.node_tree.nodes.get(tex.name).image=portrait_image

objects=[];uv_original={}
for p in manifest['parts']:
    bpy.ops.wm.stl_import(filepath=str((root/'viewer'/p['file']).resolve()))
    obj=bpy.context.object;obj.name=p['name']
    if p.get('coordinate_frame')!='stand-world-Z-up':
        for v in obj.data.vertices:v.co=(v.co.x,v.co.z,v.co.y)
        for f in obj.data.polygons:f.flip()
    bm=bmesh.new();bm.from_mesh(obj.data);bmesh.ops.remove_doubles(bm,verts=bm.verts,dist=.00002)
    bm.to_mesh(obj.data);bm.free();obj.data.update()
    for f in obj.data.polygons:f.use_smooth=True
    split=obj.modifiers.new('Angle-limited shading','EDGE_SPLIT');split.split_angle=math.radians(42)
    obj.data.materials.append(screen if p.get('role')=='display' else getmat(p))
    if p.get('role')=='display':
        vv=[v.co for v in obj.data.vertices];xmin=min(v.x for v in vv);xmax=max(v.x for v in vv);zmin=min(v.z for v in vv);zmax=max(v.z for v in vv)
        uv=obj.data.uv_layers.new(name='Dashboard')
        for f in obj.data.polygons:
            for li in f.loop_indices:
                v=obj.data.vertices[obj.data.loops[li].vertex_index].co
                uv.data[li].uv=((v.x-xmin)/(xmax-xmin),(v.z-zmin)/(zmax-zmin))
        uv_original[p['name']]=[tuple(v.uv) for v in uv.data]
    objects.append((p,obj))

def pose(mode,access='ears',explode=0):
    v=manifest['views'][mode]
    for p,obj in objects:
        visible=True
        if p.get('view') and p['view']!=mode:visible=False
        if p.get('accessory_config') not in (None,'cap') and (p.get('accessory_config')!=access or p.get('mount_orientation')!=('portrait' if mode=='portrait' else 'landscape')):visible=False
        if p.get('accessory_config')=='cap' and access!='capped' and p.get('mount_orientation')==('portrait' if mode=='portrait' else 'landscape'):visible=False
        if p.get('inspection_only') and not explode:visible=False
        obj.hide_render=not visible;obj.hide_viewport=not visible
        po=p.get('poses',{}).get(mode,{} if p.get('fixed') else v.get('pose',{}))
        rr=po.get('rotation',[0,0,0]);tt=po.get('translation',[0,0,0])
        # Browser uses Y-up with depth inverted. Convert its proper rotation
        # into Blender's Z-up frame; the native vertices are already swapped.
        B=Matrix.Rotation(math.pi/2,4,'X')
        R=B @ Euler(tuple(math.radians(x) for x in (-rr[0],-rr[1],rr[2])),'XYZ').to_matrix().to_4x4() @ B.inverted()
        t=p.get('translation',[0,0,0]);e=p.get('explode',[0,0,0])
        native=Vector((t[0]+e[0]*explode,t[2]+e[2]*explode,t[1]+e[1]*explode))
        obj.matrix_world=Matrix.Translation(Vector((tt[0],tt[2],tt[1]))) @ R @ Matrix.Translation(native)
        if p.get('role')=='display':
            obj.data.materials[0]=portrait_screen if mode=='portrait' else screen
            for datum,(uv_u,uv_v) in zip(obj.data.uv_layers.active.data,uv_original[p['name']]):datum.uv=(uv_v,1-uv_u) if mode=='portrait' else (uv_u,uv_v)
    bpy.context.view_layer.update()

def bounds():
    vv=[obj.matrix_world@v.co for p,obj in objects if not obj.hide_render for v in obj.data.vertices]
    return Vector(tuple(min(v[i] for v in vv) for i in range(3))),Vector(tuple(max(v[i] for v in vv) for i in range(3)))

desk=material('Warm studio','#eeeae4',.8)
bpy.ops.mesh.primitive_plane_add(size=3000);floor=bpy.context.object;floor.data.materials.append(desk)
def light(name,loc,energy,size,color):
    bpy.ops.object.light_add(type='AREA',location=loc);o=bpy.context.object;o.name=name;o.data.energy=energy;o.data.shape='DISK';o.data.size=size;o.data.color=color
    o.rotation_euler=(Vector((0,0,35))-o.location).to_track_quat('-Z','Y').to_euler()
light('Large softbox',(-140,-180,240),1200000,220,(1,.97,.94))
light('Right fill',(180,-20,160),750000,180,(.92,.95,1))
light('Rear rim',(-90,160,170),900000,160,(1,1,1))
bpy.ops.object.camera_add();cam=bpy.context.object;cam.data.type='ORTHO'
scene=bpy.context.scene;scene.camera=cam;scene.render.engine='CYCLES';scene.cycles.samples=12 if args.draft else 64;scene.cycles.use_denoising=True
scene.world.color=(.45,.45,.45);scene.render.image_settings.file_format='PNG';scene.view_settings.view_transform='AgX'
scene.render.resolution_percentage=50 if args.draft else 100
scene.render.resolution_x=1800;scene.render.resolution_y=1584
configs={
 'hero':('landscape','ears',(145,-245,140),163,None,0),
 'side':('landscape','capped',(-150,230,125),136,None,0),
 'usb-detail':('landscape','capped',(200,-110,105),57,(38,8,42),0),
 'cradle-detail':('landscape','ears',(135,-235,93),85,(12,0,17),0),
 'exploded':('landscape','capped',(175,-265,170),184,None,1),
 'portrait':('portrait','eyes',(140,-250,130),154,None,0),
}
shots=[]
for name in args.shots.split(','):
    mode,access,pos,scale,target,ex=configs[name];pose(mode,access,ex);lo,hi=bounds();floor.location.z=lo.z-.03
    center=(lo+hi)*.5
    if target is not None:center=Vector(target)
    cam.location=pos;cam.rotation_euler=(center-cam.location).to_track_quat('-Z','Y').to_euler();cam.data.ortho_scale=scale
    scene.render.filepath=str(out/(name+('-draft' if args.draft else '')+'.png'))
    bpy.ops.render.render(write_still=True)
    shots.append({'name':name,'mode':mode,'accessories':access,'camera':pos,'target':list(center),'orthographic_scale':scale,'exploded':bool(ex)})
old=json.loads((out/'render-shots.json').read_text()).get('shots',[]) if (out/'render-shots.json').exists() else []
combined={s['name']:s for s in old};combined.update({s['name']:s for s in shots})
(out/'render-shots.json').write_text(json.dumps({'revision':'portable-r45-public','source_commit':'115aa457c2c804d5e48c1b866748d57f49429dc8','renderer':'Blender '+bpy.app.version_string,'method':'Original public mesh geometry with angle-limited shading; no geometry reshaping. Simple authored hardware envelopes and synthetic dashboard.','shots':list(combined.values())},indent=2)+'\n')
print('R45_SITE_RENDERS_COMPLETE')
