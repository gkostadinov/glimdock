#pragma once
#include "Arduino.h"
/* Native rendering never loads or writes credentials or device preferences. */
class Preferences {
 public:
  bool begin(const char *, bool = false) { return true; }
  void end() {}
  uint8_t getUChar(const char *, uint8_t fallback = 0) { return fallback; }
  bool getBool(const char *, bool fallback = false) { return fallback; }
  size_t putUChar(const char *, uint8_t) { return 1; }
  size_t putBool(const char *, bool) { return 1; }
};
