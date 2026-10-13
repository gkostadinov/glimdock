#pragma once
#include <cmath>
#include <stdint.h>

// Voltage-only indication for a single 3.7 V LiPo, not a fuel gauge.
// Charging, load, temperature and cell age change terminal voltage.
enum class BatteryLevel:uint8_t { Unknown, VeryLow, Low, Medium, High };
struct BoardBatteryStatus {
  float volts=NAN;
  BatteryLevel level=BatteryLevel::Unknown;
  bool available()const{return std::isfinite(volts)&&level!=BatteryLevel::Unknown;}
};
inline const char* batteryLevelName(BatteryLevel level){
  switch(level){case BatteryLevel::VeryLow:return "Very low";case BatteryLevel::Low:return "Low";case BatteryLevel::Medium:return "Medium";case BatteryLevel::High:return "High";default:return "Unavailable";}
}
class BatteryMonitor {
  BoardBatteryStatus status;
  static constexpr float threshold(unsigned index){return index==0?3.45f:index==1?3.65f:3.95f;}
public:
  BoardBatteryStatus update(float measured){
    // An invalid or missing sample must not look like a full/empty battery.
    if(!std::isfinite(measured)||measured<2.5f||measured>4.45f){status={};return status;}
    if(!status.available()){
      status.volts=measured;unsigned band=0;while(band<3&&measured>=threshold(band))++band;
      status.level=BatteryLevel(band+1);
    }else{
      status.volts+=0.2f*(measured-status.volts);
      unsigned band=unsigned(status.level)-1;
      // Hysteresis suppresses changes near band edges after ADC smoothing.
      while(band<3&&status.volts>=threshold(band)+0.05f)++band;
      while(band>0&&status.volts<threshold(band-1)-0.05f)--band;
      status.level=BatteryLevel(band+1);
    }
    return status;
  }
};

// One notice per sustained low-voltage episode. Sensor loss and falling from
// Low to Very low never reset the latch; only sustained voltage recovery does.
class LowBatteryNoticePolicy {
  bool notified=false,lowPending=false,recoveryPending=false;
  uint32_t lowSince=0,recoverySince=0;
public:
  static constexpr uint32_t LOW_CONFIRM_MS=30000,RECOVERY_CONFIRM_MS=60000;
  bool update(const BoardBatteryStatus&battery,uint32_t now){
    if(!battery.available()){lowPending=recoveryPending=false;return false;}
    bool low=battery.level==BatteryLevel::Low||battery.level==BatteryLevel::VeryLow;
    if(!notified){
      if(!low)lowPending=false;
      else if(!lowPending){lowPending=true;lowSince=now;}
      else if(uint32_t(now-lowSince)>=LOW_CONFIRM_MS){notified=true;lowPending=false;return true;}
    }else{
      if(battery.volts<3.80f)recoveryPending=false;
      else if(!recoveryPending){recoveryPending=true;recoverySince=now;}
      else if(uint32_t(now-recoverySince)>=RECOVERY_CONFIRM_MS){notified=false;lowPending=recoveryPending=false;}
    }
    return false;
  }
};
