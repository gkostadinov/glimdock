#pragma once
#include <stdint.h>
#include "config.h"

// Geometry is shared by the physical display, UI and native rasterizer.
// Rotation numbers describe panel coordinates; the USB-bottom orientation must
// still be confirmed on the assembled board before selecting 0 or 2 for a case.
static_assert(HOMELAB_ROTATION >= 0 && HOMELAB_ROTATION <= 3,
              "HOMELAB_ROTATION must be 0, 1, 2 or 3");
constexpr int HOMELAB_PANEL_WIDTH=240;
constexpr int HOMELAB_PANEL_HEIGHT=320;
constexpr bool HOMELAB_IS_PORTRAIT=(HOMELAB_ROTATION % 2)==0;
constexpr int HOMELAB_VIEWPORT_WIDTH=HOMELAB_IS_PORTRAIT?HOMELAB_PANEL_WIDTH:HOMELAB_PANEL_HEIGHT;
constexpr int HOMELAB_VIEWPORT_HEIGHT=HOMELAB_IS_PORTRAIT?HOMELAB_PANEL_HEIGHT:HOMELAB_PANEL_WIDTH;
static_assert(HOMELAB_VIEWPORT_WIDTH*HOMELAB_VIEWPORT_HEIGHT==76800,
              "The V1 panel has 240 by 320 pixels");

struct BoardTouchPoint {int16_t x,y;};
// CST328 reports remain in the panel's original 240x320 coordinate space.
// Apply rotation only after the original report parser accepts the contact.
constexpr BoardTouchPoint boardRotateTouch(uint16_t x,uint16_t y,uint8_t rotation){
  if(x>=HOMELAB_PANEL_WIDTH||y>=HOMELAB_PANEL_HEIGHT)return {-1,-1};
  switch(rotation){
    case 0:return {int16_t(x),int16_t(y)};
    case 1:return {int16_t(y),int16_t(HOMELAB_PANEL_WIDTH-1-x)};
    case 2:return {int16_t(HOMELAB_PANEL_WIDTH-1-x),int16_t(HOMELAB_PANEL_HEIGHT-1-y)};
    case 3:return {int16_t(HOMELAB_PANEL_HEIGHT-1-y),int16_t(x)};
    default:return {-1,-1};
  }
}
constexpr uint8_t boardMadctlForRotation(uint8_t rotation){
  // ST7789 MADCTL: MX=0x40, MY=0x80, MV=0x20. Retain the V1 RGB order.
  return rotation==0?0x00:rotation==1?0x60:rotation==2?0xc0:0xa0;
}

void boardHoldPower();
void boardInit();
void boardBacklight(uint8_t percent);
void boardLvglInit();
void noteTouch(bool pressed);

extern bool consumeWakeTouch;
