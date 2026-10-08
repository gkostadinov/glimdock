#pragma once
// Optional private build defaults. Saved setup settings take precedence.
#if __has_include("local_credentials.h")
#include "local_credentials.h"
#endif
#ifndef HOMELAB_DEFAULT_SSID
#define HOMELAB_DEFAULT_SSID ""
#endif
#ifndef HOMELAB_DEFAULT_PASSWORD
#define HOMELAB_DEFAULT_PASSWORD ""
#endif
#ifndef HOMELAB_DEFAULT_TOKEN
#define HOMELAB_DEFAULT_TOKEN ""
#endif
#ifndef HOMELAB_DEFAULT_ENDPOINT
#define HOMELAB_DEFAULT_ENDPOINT "http://192.0.2.10:8765/api/v1/snapshot"
#endif
#define HOMELAB_POLL_MS 3000
#define HOMELAB_STALE_MS 15000
#define HOMELAB_IDLE_DIM_MS 120000
#define HOMELAB_AUTO_PAGE_MS 15000
// ST7789 rotation 3, landscape 320x240 for the USB-right enclosure.
// Select 1 for the opposite landscape direction; touch rotates to match.
#define HOMELAB_ROTATION 3
// Physical RGB bars test at boot; no networking is required. Default is off.
#define HOMELAB_COLOR_TEST 0
