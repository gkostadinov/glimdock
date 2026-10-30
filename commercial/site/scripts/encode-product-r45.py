"""Encode actual r45 CAD renders for the website, with verifiable provenance."""
import argparse
import hashlib
import io
import json
from pathlib import Path
from PIL import Image

p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--assets',type=Path,required=True)
a=p.parse_args();source=a.source.resolve();assets=a.assets.resolve()
sha=lambda path:hashlib.sha256(path.read_bytes()).hexdigest()
scene=source/'viewer/scene.json';manifest=json.loads(scene.read_text())
images=[]
for name in ['hero','side','usb-detail','cradle-detail','exploded','portrait']:
    original=assets/(name+'.png');im=Image.open(original).convert('RGB');target=36000
    candidates=[]
    for width in [1200,1100,1000]:
        size=(width,round(im.height*width/im.width));resized=im.resize(size,Image.Resampling.LANCZOS)
        for quality in [78,74,70,66,62]:
            b=io.BytesIO();resized.save(b,'WEBP',quality=quality,method=6)
            data=b.getvalue();candidates.append((data,size,quality))
            if len(data)<=target:break
        if len(data)<=target:break
    data,size,quality=candidates[-1];dest=assets/(name+'.webp');dest.write_bytes(data)
    images.append({'file':dest.name,'width':size[0],'height':size[1],'bytes':len(data),'quality':quality,'sha256':sha(dest),'source_sha256':sha(original),'description':'Actual Portable r45 CAD render; synthetic dashboard and illustrative hardware envelopes.'})
sources=[]
for part in manifest['parts']:
    path=(source/'viewer'/part['file']).resolve();rel=path.relative_to(source).as_posix()
    if any(v['path']==rel for v in sources):continue
    sources.append({'path':rel,'sha256':sha(path),'reference':part.get('reference',False)})
metadata=json.loads((assets/'render-shots.json').read_text())
metadata.update({'geometry_modified':False,'geometry_license':'CERN-OHL-P-2.0','presentation_artwork_license':'MIT','case_revision':'Portable r45','source_package':'enclosure/public/portable-r45','manifest_sha256':sha(scene),'source_files':[{'path':'viewer/scene.json','sha256':sha(scene)},*sources],'artwork':[{'file':name,'sha256':sha(assets/name),'description':'Authored synthetic dashboard; not an actual firmware screenshot.'} for name in ['synthetic-screen.png','synthetic-screen-portrait.png']],'renders':{i['file'].removesuffix('.webp'):i for i in images},'total_webp_bytes':sum(i['bytes'] for i in images),'limits':['CAD illustrations are not photographs or evidence of physical finish, fit or strength.','Generic authored battery/speaker/glass/display envelopes are illustrative; manufacturer PCB CAD is not included.','The portrait screen is synthetic layout artwork and does not establish portrait firmware support.']})
(assets/'r45-render-provenance.json').write_text(json.dumps(metadata,indent=2)+'\n')
print(json.dumps({'images':images,'total_webp_bytes':metadata['total_webp_bytes']},indent=2))
