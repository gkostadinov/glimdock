#include "audio.h"
#include <cassert>
int main(){
 static_assert(soundAmplitude(DEFAULT_SOUND_VOLUME)==900,"default preserves installed chime level");
 static_assert(soundAmplitude(0)==0,"zero volume is silence");
 static_assert(soundAmplitude(100)==3000,"maximum stays far below PCM clipping");
 static_assert(soundAmplitude(255)==3000,"corrupt saved volume is clamped");
 for(unsigned v=0;v<100;v++)assert(soundAmplitude(v+1)>soundAmplitude(v));
 boardAudioVolume(0);assert(!boardAudioPlay(AudioCue::Connected)&&!boardAudioPlay(AudioCue::Test));
 boardAudioVolume(100);boardAudioEnable(false);assert(!boardAudioPlay(AudioCue::Alert)&&boardAudioPlay(AudioCue::Test));
}
