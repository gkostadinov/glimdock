// ST7789 register sequence and CST328 protocol adapted from the user's BabyStory
// Waveshare V1 example. Sleep/audio code is deliberately independent of this app.
#include "board.h"
#include "config.h"
#include "touch_state.h"
#include <SPI.h>
#include <Wire.h>
#include <esp_heap_caps.h>
extern "C" void *homelab_lvgl_pool(size_t bytes){static void *pool=nullptr;if(!pool)pool=heap_caps_malloc(bytes,MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);if(!pool){Serial.println("512 KiB PSRAM pool unavailable; check board memory configuration");abort();}return pool;}
namespace {
constexpr int CS=42,DC=41,RST=39,BL=5,INT_PIN=4,TOUCH_RST=2;
SPIClass lcd(FSPI);
volatile bool touchIRQ=false;
TouchState touchState;
alignas(4) uint8_t drawBuffer[320*24*2];
void command(uint8_t c){lcd.beginTransaction(SPISettings(80000000,MSBFIRST,SPI_MODE0));digitalWrite(CS,LOW);digitalWrite(DC,LOW);lcd.transfer(c);digitalWrite(CS,HIGH);lcd.endTransaction();}
void bytes(const uint8_t *d,size_t n){lcd.beginTransaction(SPISettings(80000000,MSBFIRST,SPI_MODE0));digitalWrite(CS,LOW);digitalWrite(DC,HIGH);lcd.writeBytes(d,n);digitalWrite(CS,HIGH);lcd.endTransaction();}
void reg(uint8_t c,std::initializer_list<uint8_t> d){command(c);bytes(d.begin(),d.size());}
void IRAM_ATTR irq(){touchIRQ=true;}
bool touchRead(uint16_t r,uint8_t *d,size_t n){Wire1.beginTransmission(0x1a);Wire1.write(uint8_t(r>>8));Wire1.write(uint8_t(r));if(Wire1.endTransmission(true)!=0)return false;size_t got=Wire1.requestFrom(uint8_t(0x1a),uint8_t(n));if(got!=n){while(Wire1.available())Wire1.read();return false;}for(size_t i=0;i<n;i++)d[i]=Wire1.read();return true;}
bool touchWrite(uint16_t r,const uint8_t *d=nullptr,size_t n=0){Wire1.beginTransmission(0x1a);Wire1.write(uint8_t(r>>8));Wire1.write(uint8_t(r));if(n)Wire1.write(d,n);return Wire1.endTransmission(true)==0;}
void flush(lv_display_t *display,const lv_area_t *a,uint8_t *pixels){
  reg(0x2a,{uint8_t(a->x1>>8),uint8_t(a->x1),uint8_t(a->x2>>8),uint8_t(a->x2)});
  reg(0x2b,{uint8_t(a->y1>>8),uint8_t(a->y1),uint8_t(a->y2>>8),uint8_t(a->y2)});
  command(0x2c);bytes(pixels,(a->x2-a->x1+1)*(a->y2-a->y1+1)*2);
  lv_display_flush_ready(display);
}
void touch(lv_indev_t *input,lv_indev_data_t *data){
  static uint32_t lastProbe=0;uint32_t now=millis();
  // Consume before I2C so an interrupt arriving during the read is retained.
  noInterrupts();bool ready=touchIRQ;touchIRQ=false;interrupts();
  bool newReport=ready||digitalRead(INT_PIN)==LOW;
  if(newReport||uint32_t(now-lastProbe)>=40){
    lastProbe=now;uint8_t buf[27]{},count=0,clear=0;
    // The Waveshare V1 driver reads count first and acknowledges D005 only.
    // Do not impose another controller revision's status/sync-byte values.
    if(touchRead(0xd005,&count,1)&&touchRead(0xd000,buf,sizeof(buf))){
      bool wasDown=touchState.down;
      touchState.accept(buf,sizeof(buf),count,newReport,now);
      static unsigned contactLogs=0;
      if(wasDown!=touchState.down&&contactLogs<20){
        ++contactLogs;Serial.printf("Touch input: %s\n",touchState.down?"down":"released");
      }
      touchWrite(0xd005,&clear,1);
    }
  }
  touchState.expire(now);
  if(touchState.takeCancel())lv_indev_reset(input,nullptr);
  bool pressed=touchState.down;uint16_t x=touchState.x,y=touchState.y;
  noteTouch(pressed);
  if(consumeWakeTouch){data->state=LV_INDEV_STATE_RELEASED;return;}
  if(pressed){
#if HOMELAB_ROTATION == 1
    data->point.x=y;data->point.y=239-x;
#elif HOMELAB_ROTATION == 3
    data->point.x=319-y;data->point.y=x;
#else
#error "Use landscape HOMELAB_ROTATION 1 or 3"
#endif
    data->state=LV_INDEV_STATE_PRESSED;
  }else data->state=LV_INDEV_STATE_RELEASED;
}
}
void boardBacklight(uint8_t percent){percent=min(uint8_t(100),percent);ledcWrite(5,(uint32_t(percent)*1023)/100);}
void boardHoldPower(){
  // Assert the V1 latch before USB, display or radio startup. Never pulse it low.
  digitalWrite(7,HIGH);pinMode(7,OUTPUT);digitalWrite(7,HIGH);
  pinMode(6,INPUT_PULLUP);
}
void boardInit(){
  boardHoldPower();
  pinMode(CS,OUTPUT);pinMode(DC,OUTPUT);pinMode(RST,OUTPUT);digitalWrite(CS,HIGH);
  ledcSetup(5,20000,10);ledcAttachPin(BL,5);boardBacklight(0);
  lcd.begin(40,-1,45);digitalWrite(RST,LOW);delay(50);digitalWrite(RST,HIGH);delay(170);
  command(0x29);delay(120);command(0x11);delay(120);
#if HOMELAB_ROTATION == 1
  reg(0x36,{0x60});
#else
  reg(0x36,{0xa0});
#endif
  reg(0x3a,{0x05});reg(0xb0,{0x00,0xe8}); // RAMCTRL ENDIAN: little-endian RGB565.
  reg(0xb2,{0x0c,0x0c,0x00,0x33,0x33});reg(0xb7,{0x75});reg(0xbb,{0x1a});
  reg(0xc0,{0x2c});reg(0xc2,{0x01,0xff});reg(0xc3,{0x13});reg(0xc4,{0x20});reg(0xc6,{0x0f});
  reg(0xd0,{0xa4,0xa1});reg(0xd6,{0xa1});
  reg(0xe0,{0xd0,0x0d,0x14,0x0d,0x0d,0x09,0x38,0x44,0x4e,0x3a,0x17,0x18,0x2f,0x30});
  reg(0xe1,{0xd0,0x09,0x0f,0x08,0x07,0x14,0x37,0x44,0x4d,0x38,0x15,0x16,0x2c,0x2e});
  command(0x21);command(0x29);
  Wire1.begin(1,3,400000);Wire1.setTimeOut(12);pinMode(INT_PIN,INPUT);pinMode(TOUCH_RST,OUTPUT);
  digitalWrite(TOUCH_RST,HIGH);delay(50);digitalWrite(TOUCH_RST,LOW);delay(5);digitalWrite(TOUCH_RST,HIGH);delay(50);
  touchWrite(0xd101);uint8_t verification[24]{};bool good=touchRead(0xd1f4,verification,24)&&verification[10]==0xca&&verification[11]==0xca;
  touchWrite(0xd109);attachInterrupt(INT_PIN,irq,FALLING);
  Serial.printf("Waveshare V1: touch %s; PSRAM %u bytes\n",good?"ready":"unavailable",ESP.getPsramSize());
#if HOMELAB_COLOR_TEST
  static uint16_t line[320];const uint16_t colors[]={0xf800,0x07e0,0x001f,0xffff};
  for(int y=0;y<240;y++){for(int x=0;x<320;x++)line[x]=colors[x/80];reg(0x2a,{0,0,1,63});reg(0x2b,{0,uint8_t(y),0,uint8_t(y)});command(0x2c);bytes((uint8_t*)line,sizeof(line));}
  boardBacklight(80);delay(3000);
#endif
}
void boardLvglInit(){lv_init();lv_tick_set_cb([]()->uint32_t{return millis();});lv_display_t *display=lv_display_create(320,240);lv_display_set_color_format(display,LV_COLOR_FORMAT_RGB565);lv_display_set_flush_cb(display,flush);lv_display_set_buffers(display,drawBuffer,nullptr,sizeof(drawBuffer),LV_DISPLAY_RENDER_MODE_PARTIAL);lv_indev_t *input=lv_indev_create();lv_indev_set_type(input,LV_INDEV_TYPE_POINTER);lv_indev_set_scroll_limit(input,8);lv_indev_set_read_cb(input,touch);}
