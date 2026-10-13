// Short generated PCM chimes; no SD card, decoder, timer callback or UI blocking.
#include "audio.h"
#ifndef HOMELAB_NATIVE_PREVIEW
#include <Arduino.h>
#include <driver/i2s.h>
#include <atomic>
#include <algorithm>
#include <cmath>
namespace {
constexpr i2s_port_t PORT=I2S_NUM_0;
constexpr int RATE=16000;
QueueHandle_t queue=nullptr;
std::atomic<bool> enabled{true};
std::atomic<uint8_t> volume{DEFAULT_SOUND_VOLUME};
bool ready=false;
bool tone(float hz,unsigned durationMs){
  int16_t pcm[128];const unsigned frames=RATE*durationMs/1000,fade=RATE*8/1000;
  for(unsigned offset=0;offset<frames;offset+=64){
    unsigned count=std::min(64u,frames-offset);
    const int amplitude=soundAmplitude(volume.load());
    for(unsigned i=0;i<count;i++){
      unsigned at=offset+i;float envelope=std::min(1.0f,float(std::min(at,frames-1-at))/fade);
      int16_t sample=int16_t(amplitude*envelope*std::sin(6.28318530718f*hz*at/RATE));
      pcm[2*i]=pcm[2*i+1]=sample;
    }
    size_t bytes=0;size_t wanted=count*2*sizeof(int16_t);
    if(i2s_write(PORT,pcm,wanted,&bytes,pdMS_TO_TICKS(100))!=ESP_OK||bytes!=wanted)return false;
  }
  return true;
}
void worker(void*){
  AudioCue cue;
  for(;;){
    if(xQueueReceive(queue,&cue,portMAX_DELAY)!=pdTRUE)continue;
    if(!volume.load()||(!enabled.load()&&cue!=AudioCue::Test))continue;
    float first=cue==AudioCue::Alert?660:880,second=cue==AudioCue::Alert?440:1175;
    bool ok=tone(first,90)&&tone(0,45)&&tone(second,cue==AudioCue::PrintComplete?150:110);
    // Drain the bounded DMA tail, then leave silence rather than replaying a buffer.
    vTaskDelay(pdMS_TO_TICKS(80));i2s_zero_dma_buffer(PORT);
    if(!ok)Serial.println("Speaker: write failed; chime stopped");
  }
}
}
bool boardAudioInit(bool soundEnabled){
  enabled.store(soundEnabled);if(ready)return true;
  i2s_config_t config{};config.mode=i2s_mode_t(I2S_MODE_MASTER|I2S_MODE_TX);
  config.sample_rate=RATE;config.bits_per_sample=I2S_BITS_PER_SAMPLE_16BIT;
  config.channel_format=I2S_CHANNEL_FMT_RIGHT_LEFT;config.communication_format=I2S_COMM_FORMAT_STAND_I2S;
  config.intr_alloc_flags=ESP_INTR_FLAG_LEVEL1;config.dma_buf_count=4;config.dma_buf_len=128;config.tx_desc_auto_clear=true;
  if(i2s_driver_install(PORT,&config,0,nullptr)!=ESP_OK){Serial.println("Speaker: I2S unavailable");return false;}
  i2s_pin_config_t pins{};pins.mck_io_num=I2S_PIN_NO_CHANGE;pins.bck_io_num=48;pins.ws_io_num=38;
  pins.data_out_num=47;pins.data_in_num=I2S_PIN_NO_CHANGE;
  if(i2s_set_pin(PORT,&pins)!=ESP_OK){i2s_driver_uninstall(PORT);Serial.println("Speaker: pin setup failed");return false;}
  i2s_zero_dma_buffer(PORT);queue=xQueueCreate(4,sizeof(AudioCue));
  if(!queue||xTaskCreatePinnedToCore(worker,"glim-audio",3072,nullptr,1,nullptr,0)!=pdPASS){
    if(queue)vQueueDelete(queue);queue=nullptr;i2s_driver_uninstall(PORT);Serial.println("Speaker: worker unavailable");return false;
  }
  ready=true;Serial.println("Speaker: PCM5101 ready; quiet notification chimes");return true;
}
bool boardAudioReady(){return ready;}
void boardAudioEnable(bool value){enabled.store(value);}
void boardAudioVolume(unsigned value){volume.store(clampSoundVolume(value));}
uint8_t boardAudioVolume(){return volume.load();}
bool boardAudioPlay(AudioCue cue){
  bool queued=ready&&volume.load()&&(enabled.load()||cue==AudioCue::Test)&&xQueueSend(queue,&cue,0)==pdTRUE;
  if(queued)Serial.printf("Speaker: %s chime queued\n",cue==AudioCue::Connected?"connected":cue==AudioCue::Alert?"alert":cue==AudioCue::PrintComplete?"print complete":"test");
  return queued;
}
#endif
