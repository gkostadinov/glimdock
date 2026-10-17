#pragma once
#include <stddef.h>
class Stream;

namespace flipper {
// Called only by the existing local USB line dispatcher. Never attach this to
// a network endpoint. Returns true for recognized Flipper command prefixes;
// consumed input is wiped, including submitted PIN digits.
#ifdef HOMELAB_NATIVE_PREVIEW
inline bool handleUsbLine(char*, size_t, Stream&) { return false; }
#else
bool handleUsbLine(char* line, size_t len, Stream& serial);
#endif
} // namespace flipper
