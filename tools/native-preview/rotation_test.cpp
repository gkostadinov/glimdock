#include "board.h"
#include <array>
#include <cassert>
#include <cstdio>
#include <vector>

int main(){
  const BoardTouchPoint rawCorners[]={{0,0},{239,0},{0,319},{239,319}};
  const BoardTouchPoint expected[4][4]={
    {{0,0},{239,0},{0,319},{239,319}},
    {{0,239},{0,0},{319,239},{319,0}},
    {{239,319},{0,319},{239,0},{0,0}},
    {{319,0},{319,239},{0,0},{0,239}}
  };
  const uint8_t madctl[]={0x00,0x60,0xc0,0xa0};
  for(uint8_t rotation=0;rotation<4;rotation++){
    const int width=(rotation&1)?320:240,height=(rotation&1)?240:320;
    assert(boardMadctlForRotation(rotation)==madctl[rotation]);
    for(size_t i=0;i<4;i++){
      const auto corner=boardRotateTouch(rawCorners[i].x,rawCorners[i].y,rotation);
      assert(corner.x==expected[rotation][i].x&&corner.y==expected[rotation][i].y);
    }
    std::vector<bool> visited(width*height,false);
    for(int y=0;y<320;y++)for(int x=0;x<240;x++){
      const auto point=boardRotateTouch(x,y,rotation);
      assert(point.x>=0&&point.x<width&&point.y>=0&&point.y<height);
      const size_t index=point.y*width+point.x;
      assert(!visited[index]);visited[index]=true;
    }
    for(bool seen:visited)assert(seen);
    assert(boardRotateTouch(240,0,rotation).x==-1);
    assert(boardRotateTouch(0,320,rotation).y==-1);
  }
  assert(boardRotateTouch(0,0,4).x==-1);
  assert(HOMELAB_VIEWPORT_WIDTH==((HOMELAB_ROTATION&1)?320:240));
  assert(HOMELAB_VIEWPORT_HEIGHT==((HOMELAB_ROTATION&1)?240:320));
  assert(HOMELAB_IS_PORTRAIT==((HOMELAB_ROTATION&1)==0));
  printf("Rotation %d: %dx%d; all four rotations cover each panel pixel once, corners and bounds pass\n",
         HOMELAB_ROTATION,HOMELAB_VIEWPORT_WIDTH,HOMELAB_VIEWPORT_HEIGHT);
}
