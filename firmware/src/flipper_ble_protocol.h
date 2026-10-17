#pragma once
/* Glimdock dedicated BLE transport. C99/C++17; GdStatus itself remains v1. */
#include "flipper_protocol.h"
#include "flipper_cache_protocol.h"

#define GD_BLE_SERVICE_UUID "781e3b16-8186-4b22-9a7b-0ab583e57a10"
#define GD_BLE_WRITE_UUID "781e3b16-8186-4b22-9a7b-0ab583e57a11"
#define GD_BLE_ACK_UUID "781e3b16-8186-4b22-9a7b-0ab583e57a12"
#define GD_BLE_CONTROL_UUID "781e3b16-8186-4b22-9a7b-0ab583e57a13"
#define GD_BLE_SERVICE_UUID_BYTES {0x10,0x7a,0xe5,0x83,0xb5,0x0a,0x7b,0x9a,0x22,0x4b,0x86,0x81,0x16,0x3b,0x1e,0x78}
#define GD_BLE_WRITE_UUID_BYTES {0x11,0x7a,0xe5,0x83,0xb5,0x0a,0x7b,0x9a,0x22,0x4b,0x86,0x81,0x16,0x3b,0x1e,0x78}
#define GD_BLE_ACK_UUID_BYTES {0x12,0x7a,0xe5,0x83,0xb5,0x0a,0x7b,0x9a,0x22,0x4b,0x86,0x81,0x16,0x3b,0x1e,0x78}
#define GD_BLE_CONTROL_UUID_BYTES {0x13,0x7a,0xe5,0x83,0xb5,0x0a,0x7b,0x9a,0x22,0x4b,0x86,0x81,0x16,0x3b,0x1e,0x78}
#define GD_BLE_ADVERTISED_SERVICE 0xFFF0u
#define GD_BLE_NAME "Glimdock"
#define GD_BLE_CHUNK_HEADER 8u
/* 232-byte value: 242 event-parameter bytes (vendor2 + attributes8), below
 * the pinned STM32WB 255-byte HCI limit. MTU247 carries a 516-byte detail in 3 writes;
 * senders still cap each write to the actual negotiated MTU minus 3 and 8. */
#define GD_BLE_CHUNK_PAYLOAD 224u
#define GD_BLE_CHUNK_MAX (GD_BLE_CHUNK_HEADER + GD_BLE_CHUNK_PAYLOAD)
#define GD_BLE_ACK_BYTES 8u
#define GD_BLE_ACK_OK 0u
#define GD_BLE_CONTROL_BYTES 8u
#define GD_BLE_NODE_NEXT 1u
#define GD_BLE_NODE_PREVIOUS 2u

typedef struct {
    uint8_t frame[GD_DETAIL_BYTES];
    uint32_t sequence;
    uint16_t used;
    bool active;
} GdBleAssembler;

typedef enum {
    GdBleChunkRejected,
    GdBleChunkPartial,
    GdBleChunkComplete,
} GdBleChunkResult;

static inline uint32_t gd_ble_read_u32(const uint8_t* p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static inline void gd_ble_write_u32(uint8_t* p, uint32_t value) {
    for(size_t i = 0; i < 4; ++i) p[i] = (uint8_t)(value >> (i * 8));
}
static inline void gd_ble_reset(GdBleAssembler* receiver) {
    memset(receiver, 0, sizeof(*receiver));
}
static inline size_t gd_ble_chunk_encode(
    const uint8_t* frame, uint32_t sequence, uint16_t offset, size_t payload_size,
    uint8_t* out, size_t capacity) {
    if(!frame || !out || !payload_size || payload_size > GD_BLE_CHUNK_PAYLOAD ||
       offset >= GD_DETAIL_BYTES || payload_size > GD_DETAIL_BYTES - offset ||
       capacity < GD_BLE_CHUNK_HEADER + payload_size) return 0;
    out[0] = 'G'; out[1] = 'D';
    gd_ble_write_u32(out + 2, sequence);
    gd_write_u16(out + 6, offset);
    memcpy(out + GD_BLE_CHUNK_HEADER, frame + offset, payload_size);
    return GD_BLE_CHUNK_HEADER + payload_size;
}
/* Offset zero deliberately restarts an incomplete/retried generation. */
static inline GdBleChunkResult gd_ble_chunk_receive(
    GdBleAssembler* receiver, const uint8_t* packet, size_t size) {
    if(!receiver) return GdBleChunkRejected;
    if(!packet || size <= GD_BLE_CHUNK_HEADER || size > GD_BLE_CHUNK_MAX ||
       packet[0] != 'G' || packet[1] != 'D') {
        gd_ble_reset(receiver);
        return GdBleChunkRejected;
    }
    uint32_t sequence = gd_ble_read_u32(packet + 2);
    uint16_t offset = gd_read_u16(packet + 6);
    size_t count = size - GD_BLE_CHUNK_HEADER;
    if(!offset) {
        gd_ble_reset(receiver);
        receiver->active = true;
        receiver->sequence = sequence;
    }
    if(!receiver->active || sequence != receiver->sequence || offset != receiver->used ||
       offset >= GD_DETAIL_BYTES || count > GD_DETAIL_BYTES - offset) {
        gd_ble_reset(receiver);
        return GdBleChunkRejected;
    }
    memcpy(receiver->frame + offset, packet + GD_BLE_CHUNK_HEADER, count);
    receiver->used += (uint16_t)count;
    if(receiver->used == GD_DETAIL_BYTES) {
        receiver->active = false;
        return GdBleChunkComplete;
    }
    return GdBleChunkPartial;
}
static inline void gd_ble_ack_encode(uint32_t sequence, uint8_t out[GD_BLE_ACK_BYTES]) {
    out[0] = 'G'; out[1] = 'A';
    gd_ble_write_u32(out + 2, sequence);
    out[6] = GD_BLE_ACK_OK; out[7] = 0;
}
static inline bool gd_ble_ack_decode(const uint8_t* packet, size_t size, uint32_t* sequence) {
    if(!packet || !sequence || size != GD_BLE_ACK_BYTES || packet[0] != 'G' ||
       packet[1] != 'A' || packet[6] != GD_BLE_ACK_OK || packet[7]) return false;
    *sequence = gd_ble_read_u32(packet + 2);
    return true;
}
/* User node-selection events; sequence zero is reserved for an empty value.
 * Clients deduplicate advancing sequences within each connection and discard
 * controls queued by an old connection. The peripheral does likewise. */
static inline bool gd_ble_control_encode(
    uint32_t sequence, uint8_t direction, uint8_t out[GD_BLE_CONTROL_BYTES]) {
    if(!out || !sequence || (direction != GD_BLE_NODE_NEXT && direction != GD_BLE_NODE_PREVIOUS))
        return false;
    out[0] = 'G'; out[1] = 'N';
    gd_ble_write_u32(out + 2, sequence);
    out[6] = direction; out[7] = 0;
    return true;
}
static inline bool gd_ble_control_decode(
    const uint8_t* packet, size_t size, uint32_t* sequence, uint8_t* direction) {
    if(!packet || !sequence || !direction || size != GD_BLE_CONTROL_BYTES ||
       packet[0] != 'G' || packet[1] != 'N' || packet[7] ||
       (packet[6] != GD_BLE_NODE_NEXT && packet[6] != GD_BLE_NODE_PREVIOUS) ||
       !gd_ble_read_u32(packet + 2)) return false;
    *sequence = gd_ble_read_u32(packet + 2);
    *direction = packet[6];
    return true;
}
