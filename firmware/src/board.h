#pragma once
#include <Arduino.h>
#include <lvgl.h>
void boardHoldPower();
void boardInit();
void boardBacklight(uint8_t percent);
void boardLvglInit();
void noteTouch(bool pressed);

extern bool consumeWakeTouch;
