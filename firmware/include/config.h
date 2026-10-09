#pragma once
// Optional private build defaults. Saved setup settings take precedence.
#if !defined(GLIMDOCK_PUBLIC_FIRMWARE) && __has_include("local_credentials.h")
#include "local_credentials.h"
#endif
#ifdef GLIMDOCK_PUBLIC_FIRMWARE
// Redistributable builds always begin unpaired; NVS settings survive updates.
#undef HOMELAB_DEFAULT_SSID
#undef HOMELAB_DEFAULT_PASSWORD
#undef HOMELAB_DEFAULT_TOKEN
#undef HOMELAB_DEFAULT_SETUP_TOKEN
#undef HOMELAB_DEFAULT_ENDPOINT
#define HOMELAB_DEFAULT_SSID ""
#define HOMELAB_DEFAULT_PASSWORD ""
#define HOMELAB_DEFAULT_TOKEN ""
#define HOMELAB_DEFAULT_SETUP_TOKEN ""
#define HOMELAB_DEFAULT_ENDPOINT ""
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
#ifndef HOMELAB_DEFAULT_SETUP_TOKEN
#define HOMELAB_DEFAULT_SETUP_TOKEN ""
#endif
#ifndef HOMELAB_DEFAULT_ENDPOINT
#define HOMELAB_DEFAULT_ENDPOINT ""
#endif
#define HOMELAB_POLL_MS 3000
#define HOMELAB_STALE_MS 15000
#define HOMELAB_IDLE_DIM_MS 120000
#define HOMELAB_AUTO_PAGE_MS 15000
// 0/2: portrait 240x320; 1/3: landscape 320x240. Display and touch rotate together.
// Pebble Landscape uses USB-right rotation 3. Explicit build environments
// preserve portrait and alternate landscape orientations.
#ifndef HOMELAB_ROTATION
#define HOMELAB_ROTATION 3
#endif
// Physical RGB bars test at boot; no networking is required. Default is off.
#ifndef HOMELAB_COLOR_TEST
#define HOMELAB_COLOR_TEST 0
#endif
