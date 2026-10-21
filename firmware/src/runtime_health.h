#pragma once
#ifndef HOMELAB_NATIVE_PREVIEW
#include <Arduino.h>
void startRuntimeHealth();
void tickRuntimeHealth();
bool handleRuntimeHealthLine(char* line, size_t length, Stream& serial);
#endif
