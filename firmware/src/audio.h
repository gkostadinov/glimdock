#pragma once
#include <stdint.h>
constexpr uint8_t DEFAULT_SOUND_VOLUME=30;
constexpr uint8_t clampSoundVolume(unsigned value){return value>100?100:value;}
// Keep the established quiet chime at the default; 0 is silent.
constexpr int soundAmplitude(unsigned volume){return 30*clampSoundVolume(volume);}
enum class AudioCue:uint8_t { Connected, Alert, PrintComplete, Test };
#ifdef HOMELAB_NATIVE_PREVIEW
inline bool nativeSoundEnabled=true;
inline unsigned nativeAudioRequests=0;
inline uint8_t nativeSoundVolume=DEFAULT_SOUND_VOLUME;
inline bool boardAudioInit(bool enabled){nativeSoundEnabled=enabled;return true;}
inline bool boardAudioReady(){return true;}
inline void boardAudioEnable(bool enabled){nativeSoundEnabled=enabled;}
inline void boardAudioVolume(unsigned value){nativeSoundVolume=clampSoundVolume(value);}
inline uint8_t boardAudioVolume(){return nativeSoundVolume;}
inline bool boardAudioPlay(AudioCue cue){if(!nativeSoundVolume||(!nativeSoundEnabled&&cue!=AudioCue::Test))return false;++nativeAudioRequests;return true;}
#else
bool boardAudioInit(bool enabled);
bool boardAudioReady();
void boardAudioEnable(bool enabled);
void boardAudioVolume(unsigned value);
uint8_t boardAudioVolume();
bool boardAudioPlay(AudioCue cue);
#endif
