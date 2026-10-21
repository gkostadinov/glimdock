#include "device_power.h"
#include <cassert>
#include <cstdio>

static void exercise(uint32_t base){
  DevicePowerButton key;
  const auto at=[&](bool down,uint32_t elapsed){return key.update(down,base+elapsed);};
  // Holding BAT_PWR to boot must never immediately shut down the device.
  assert(!at(true,0));assert(!at(true,40));assert(!at(true,8000));
  assert(!key.released());assert(!at(false,8001));assert(!key.released());
  assert(!at(false,8041));assert(key.released());
  // A brief contact or bounce is not a two-second hold.
  for(uint32_t t=8100;t<8500;t+=20)assert(!at((t/20)%2,t));
  assert(!at(false,8500));assert(!at(false,8540));assert(key.released());
  assert(!at(true,9000));assert(!at(true,9040));
  assert(!at(true,11039));assert(at(true,11040));
  for(uint32_t t=11041;t<12000;++t)assert(!at(true,t));
  // Waiting for a debounced release prevents immediate EXT0 wake loops.
  assert(!at(false,12000));assert(!key.released());
  assert(!at(false,12039));assert(!key.released());
  assert(!at(false,12040));assert(key.released());
  assert(!at(true,13000));assert(!at(true,13040));assert(at(true,15040));
}
int main(){
  exercise(0);exercise(UINT32_MAX-10000);
  puts("Power button: startup hold, bounce, short press, one-shot long hold, release and timer rollover passed.");
}
