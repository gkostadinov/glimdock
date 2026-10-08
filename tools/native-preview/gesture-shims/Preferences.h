#pragma once
#include "Arduino.h"
#include <map>
class Preferences {
 inline static std::map<std::string,bool> booleans;
 inline static std::map<std::string,uint8_t> integers;
 std::string ns;
 public:
 bool begin(const char *name,bool=false){ns=name;return true;}
 void end(){}
 uint8_t getUChar(const char*k,uint8_t fallback=0){auto i=integers.find(ns+k);return i==integers.end()?fallback:i->second;}
 bool getBool(const char*k,bool fallback=false){auto i=booleans.find(ns+k);return i==booleans.end()?fallback:i->second;}
 size_t putUChar(const char*k,uint8_t v){integers[ns+k]=v;return 1;}
 size_t putBool(const char*k,bool v){booleans[ns+k]=v;return 1;}
};
