#pragma once
#include <stdint.h>

// Product branding is separate from storage keys and the collector protocol.
// Existing displays retain the homelab NVS namespace across firmware updates.
namespace glimdock {
inline constexpr char NAME[] = "Glimdock";
inline constexpr char WORDMARK[] = "glimdock";
inline constexpr char SETUP_SSID[] = "Glimdock-Setup";
inline constexpr uint32_t PURPLE = 0x6644dd;
inline constexpr uint32_t WHITE = 0xffffff;
inline constexpr uint32_t INK_LIGHT = 0x171323;
inline constexpr uint32_t INK_DARK = 0xeaf2fb;
}
