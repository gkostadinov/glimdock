#pragma once
#include "Arduino.h"
#include <map>
/* The emulator's display preferences last for its browser session. */
class Preferences {
  inline static std::map<std::string, uint8_t> values_;
 public:
  bool begin(const char *, bool = false) { return true; }
  void end() {}
  uint8_t getUChar(const char *name, uint8_t fallback = 0) { auto i=values_.find(name); return i==values_.end()?fallback:i->second; }
  bool getBool(const char *name, bool fallback = false) { return getUChar(name, fallback)!=0; }
  size_t putUChar(const char *name, uint8_t value) { values_[name]=value; return 1; }
  size_t putBool(const char *name, bool value) { return putUChar(name,value); }
};
