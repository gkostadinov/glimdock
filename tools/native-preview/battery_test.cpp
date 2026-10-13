#include "battery_status.h"
#include <cassert>
#include <cstdio>

int main(){
  BatteryMonitor monitor;
  assert(!monitor.update(NAN).available());
  assert(!monitor.update(0).available());
  assert(!monitor.update(5).available());
  assert(monitor.update(3.30f).level==BatteryLevel::VeryLow);
  monitor.update(NAN);
  assert(monitor.update(3.55f).level==BatteryLevel::Low);
  monitor.update(NAN);
  assert(monitor.update(3.80f).level==BatteryLevel::Medium);
  monitor.update(NAN);
  assert(monitor.update(4.20f).level==BatteryLevel::High);
  monitor.update(NAN);
  auto previous=monitor.update(3.94f);
  for(unsigned i=0;i<100;i++){
    auto reading=monitor.update(i%2?3.96f:3.94f);
    assert(reading.level==BatteryLevel::Medium);
    assert(std::fabs(reading.volts-previous.volts)<0.01f);
    previous=reading;
  }
  for(unsigned i=0;i<50;i++)monitor.update(4.10f);
  assert(monitor.update(4.10f).level==BatteryLevel::High);
  for(unsigned i=0;i<50;i++)monitor.update(3.80f);
  assert(monitor.update(3.80f).level==BatteryLevel::Medium);
  for(unsigned i=0;i<50;i++)monitor.update(3.30f);
  assert(monitor.update(3.30f).level==BatteryLevel::VeryLow);
  // Invalid input immediately masks a previous charge indication.
  assert(!monitor.update(INFINITY).available());
  assert(!monitor.update(4.46f).available());
  assert(!monitor.update(2.49f).available());
  assert(monitor.update(3.7f).level==BatteryLevel::Medium);
  BoardBatteryStatus low{3.55f,BatteryLevel::Low},veryLow{3.30f,BatteryLevel::VeryLow},recovered{3.85f,BatteryLevel::Medium},nearRecovery{3.75f,BatteryLevel::Medium};
  LowBatteryNoticePolicy notice;
  assert(!notice.update(low,0));
  assert(!notice.update(low,29999));
  assert(notice.update(low,30000));
  for(uint32_t at=30500;at<300000;at+=500)assert(!notice.update(at%1000?low:veryLow,at));
  assert(!notice.update({},300000));assert(!notice.update(low,400000));
  assert(!notice.update(recovered,410000));assert(!notice.update(nearRecovery,469999));
  assert(!notice.update(low,500000)); // Brief recovery must not re-arm.
  assert(!notice.update(recovered,510000));assert(!notice.update(recovered,569999));
  assert(!notice.update(recovered,570000));
  assert(!notice.update(low,571000));assert(notice.update(low,601000));
  assert(!notice.update(veryLow,602000));
  LowBatteryNoticePolicy transient;
  assert(!transient.update(low,0));assert(!transient.update(recovered,29999));
  assert(!transient.update(low,30000));assert(!transient.update(low,59999));
  assert(!transient.update({},60000));assert(!transient.update(low,90000));
  assert(transient.update(low,120000)); // Unknown samples break confirmation.
  LowBatteryNoticePolicy wrap;
  assert(!wrap.update(low,UINT32_MAX-9999));assert(!wrap.update(low,19999));
  assert(wrap.update(low,20000)); // Millis rollover preserves elapsed time.
  puts("Battery bands, smoothing, hysteresis, invalid readings, one-shot notices, sustained recovery and timer rollover passed.");
}
