#pragma once
#include <bt/bt_service/bt.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct GlimdockBle GlimdockBle;
typedef bool (*GlimdockFrameCallback)(const uint8_t* frame, size_t size, void* context);

GlimdockBle* glimdock_ble_start(Bt* bt, GlimdockFrameCallback callback, void* context);
void glimdock_ble_reset(GlimdockBle* profile);
void glimdock_ble_set_connected(GlimdockBle* profile, bool connected);
/* Retain only the newest absolute target, including while disconnected. */
bool glimdock_ble_request_node(GlimdockBle* profile, uint32_t sequence, const char* node_id);
void glimdock_ble_clear_selection(GlimdockBle* profile);
void glimdock_ble_prepare_stop(GlimdockBle* profile);
void glimdock_ble_poll(GlimdockBle* profile);
