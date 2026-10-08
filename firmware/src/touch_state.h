#pragma once
#include <stdint.h>
#include <stddef.h>

// V1 CST328 report lifetime. D005 is read separately and then cleared by the host.
// A cleared count between reports must not split a swipe into independent taps.
class TouchState {
 public:
  bool down=false;
  uint16_t x=0,y=0;
  bool cancel=false;
  uint32_t lastReport=0;
  uint32_t lastRead=0;
  static constexpr uint32_t releaseGraceMs=120;
  static constexpr uint32_t lossTimeoutMs=750;
  bool accept(const uint8_t *frame,size_t size,uint8_t reportCount,bool newReport,uint32_t now){
    if(size!=27)return false;
    const uint8_t count=reportCount&15,event=frame[0]&15;
    if(count>5)return false;
    lastRead=now;
    if(event==5||(count==0&&(newReport||(down&&uint32_t(now-lastReport)>=releaseGraceMs)))){down=false;return true;}
    if(count==0)return true;
    const uint16_t nextX=(uint16_t(frame[1])<<4)|(frame[3]>>4);
    const uint16_t nextY=(uint16_t(frame[2])<<4)|(frame[3]&15);
    lastReport=now;
    if(count!=1||nextX>=240||nextY>=320){if(down)cancel=true;down=false;return true;}
    down=true;x=nextX;y=nextY;return true;
  }
  void expire(uint32_t now){if(down&&uint32_t(now-lastRead)>=lossTimeoutMs){down=false;cancel=true;}}
  bool takeCancel(){bool result=cancel;cancel=false;return result;}
};
