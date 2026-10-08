"""Internal USB privacy walls carried by the removable rear cover.

No material reaches the outside skin. The open front of the channel clears
the front-shell floor; its side walls and back shade screen the surrounding
electronics without narrowing the existing plug passage.
"""


def create_baffle(P, box):
    cfg=P['usb_baffle']
    y=.06
    rear_outer=P['shell_depth']+P['joint_gap']+P['rear_thickness']
    width=cfg['x_outer']-cfg['x_inner']
    center=(cfg['x_outer']+cfg['x_inner'])/2
    half=cfg['passage_height']/2
    wall=cfg['wall']
    result=None
    for sign in (-1,1):
        part=box(width,wall,rear_outer-cfg['front_depth'],center,
                 y+sign*(half+wall/2),cfg['front_depth'])
        result=part if result is None else result.union(part)
    back=box(width,cfg['passage_height']+2*wall,rear_outer-cfg['back_depth'],
             center,y,cfg['back_depth'])
    return result.union(back)
