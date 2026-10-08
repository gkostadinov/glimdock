#include "../firmware/src/touch_state.h"
#include <cassert>
#include <cstdio>
int main(){
  TouchState s;uint8_t f[27]{};f[0]=6;f[1]=6;f[2]=8;f[3]=0x45;f[5]=1;f[6]=0xab;
  assert(s.accept(f,27,1,true,10)&&s.down&&s.x==100&&s.y==133);
  // Ack-cleared registers between movement reports cannot end the drag.
  uint8_t ack[27]{};ack[0]=0xab;ack[6]=0xab;
  assert(s.accept(ack,27,0,false,30)&&s.down);s.expire(50);assert(s.down&&!s.cancel);
  f[2]=12;assert(s.accept(f,27,1,true,60)&&s.down&&s.y==197);
  assert(!s.accept(f,12,1,true,70)&&s.down);s.expire(100);assert(s.down);
  // A genuine up packet ends a clean tap without cancellation.
  f[0]=5;f[5]=0;assert(s.accept(f,27,0,true,110)&&!s.down&&!s.takeCancel());
  // This board does not require the generic driver's marker/status bytes.
  f[0]=0;f[6]=0;f[5]=0;
  assert(s.accept(f,27,1,true,120)&&s.down);
  assert(s.accept(ack,27,0,false,160)&&s.down);
  assert(s.accept(ack,27,0,false,200)&&s.down);
  // Verified quiet count reads eventually release on revisions without an up IRQ.
  assert(s.accept(ack,27,0,false,240)&&!s.down&&!s.takeCancel());
  assert(s.accept(f,27,1,true,250)&&s.down);
  // Repeated down reports support a stationary hold longer than the loss timeout.
  for(uint32_t now=290;now<=1490;now+=40){
    assert(s.accept(ack,27,0,false,now)&&s.down);
    assert(s.accept(f,27,1,true,now+20)&&s.down);
    s.expire(now+30);assert(s.down&&!s.takeCancel());
  }
  // A fresh zero-count IRQ is an explicit lift, even with no status marker.
  assert(s.accept(ack,27,0,true,1550)&&!s.down&&!s.takeCancel());
  assert(s.accept(f,27,1,true,1560)&&s.down);
  // Persistent lost input cancels rather than synthesizing a row click.
  s.expire(2310);assert(!s.down&&s.takeCancel());assert(!s.takeCancel());
  assert(!s.accept(f,27,6,true,2320)&&!s.down);
  assert(s.accept(f,27,1,true,2330)&&s.down);
  assert(s.accept(f,27,2,true,2340)&&!s.down&&s.takeCancel());
  // Timeout arithmetic must survive the millis() rollover.
  f[5]=1;assert(s.accept(f,27,1,true,0xffffff00)&&s.down);s.expire(0x20);assert(s.down);
  s.expire(0x200);assert(!s.down&&s.takeCancel());
  puts("Touch lifetime regressions passed");
}
