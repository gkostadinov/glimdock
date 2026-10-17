#pragma once
/* Glimdock all-node RAM cache protocol v2, C99/C++17. */
#include "flipper_protocol.h"

#define GD_CATALOG_MAX_NODES 8u
#define GD_CACHE_MAX_NODES GD_CATALOG_MAX_NODES
#define GD_NODE_ID_BYTES 64u
#define GD_DETAIL_BYTES 516u
#define GD_DETAIL_COMPLETE 1u
#define GD_CACHE_DETAIL_BYTES GD_DETAIL_BYTES
#define GD_CACHE_MAX_PACKET GD_DETAIL_BYTES
#define GD_CATALOG_BEGIN 1u
#define GD_CATALOG_ENTRY 2u
#define GD_CATALOG_COMMIT 3u
#define GD_CATALOG_HEADER_BYTES 80u
#define GD_CATALOG_ENTRY_BYTES 189u
#define GD_CACHE_CONTROL_BYTES 70u
#define GD_CACHE_DETAIL_SEQUENCE_MAX 0x7fffffffu
#define GD_CACHE_CATALOG_SEQUENCE_FLAG 0x80000000u

static inline bool gd_detail_sequence_valid(uint32_t sequence) {
    return sequence && !(sequence & GD_CACHE_CATALOG_SEQUENCE_FLAG);
}
static inline bool gd_catalog_sequence_valid(uint32_t sequence) {
    return (sequence & GD_CACHE_CATALOG_SEQUENCE_FLAG) != 0;
}

typedef struct { char id[GD_NODE_ID_BYTES]; GdStatusPage system; } GdCatalogNode;
typedef struct {
    uint32_t generation;
    uint8_t count;
    char selected_id[GD_NODE_ID_BYTES];
    GdCatalogNode nodes[GD_CATALOG_MAX_NODES];
} GdNodeCatalog;
typedef struct {
    GdStatus status;
    char node_id[GD_NODE_ID_BYTES];
    uint32_t request_sequence;
    bool complete;
} GdDetail;
typedef struct {
    uint8_t type, index, count;
    uint32_t transport_sequence, generation;
    char node_id[GD_NODE_ID_BYTES];
    GdStatusPage system;
} GdCatalogPacket;

static inline uint32_t gd_cache_read_u32(const uint8_t* p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static inline void gd_cache_write_u32(uint8_t* p, uint32_t n) {
    for(size_t i = 0; i < 4; ++i) p[i] = (uint8_t)(n >> (8 * i));
}
static inline bool gd_cache_id_valid(const char* id, bool empty) {
    if(!id) return false;
    bool ended = false;
    for(size_t i = 0; i < GD_NODE_ID_BYTES; ++i) {
        const uint8_t c = (uint8_t)id[i];
        if(!c) { if(!i && !empty) return false; ended = true; }
        else if(ended || !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                (c >= '0' && c <= '9') || c == ':' || c == '.' || c == '_' || c == '-')) return false;
    }
    return ended;
}
static inline bool gd_cache_copy_id(char out[GD_NODE_ID_BYTES], const char* id, bool empty) {
    if(!out || !id) return false;
    char checked[GD_NODE_ID_BYTES] = {0};
    size_t n = 0;
    while(n < GD_NODE_ID_BYTES && id[n]) { checked[n] = id[n]; ++n; }
    if(n == GD_NODE_ID_BYTES || !gd_cache_id_valid(checked, empty)) return false;
    memcpy(out, checked, GD_NODE_ID_BYTES);
    return true;
}
static inline bool gd_cache_page_decode(const uint8_t* p, GdStatusPage* out) {
    if(!p || !out || (p[0] & ~(GD_STATUS_VALID | GD_STATUS_STALE | GD_STATUS_DEMO)) ||
       !gd_read_u16(p + 3) || !gd_status_text_valid(p + 5, GD_STATUS_TITLE)) return false;
    for(size_t r = 0; r < GD_STATUS_ROWS; ++r)
        if(!gd_status_text_valid(p + 5 + GD_STATUS_TITLE + r * GD_STATUS_TEXT, GD_STATUS_TEXT)) return false;
    memset(out, 0, sizeof(*out));
    out->flags = p[0]; out->age_seconds = gd_read_u16(p + 1); out->ttl_seconds = gd_read_u16(p + 3);
    memcpy(out->title, p + 5, GD_STATUS_TITLE);
    memcpy(out->rows, p + 5 + GD_STATUS_TITLE, GD_STATUS_ROWS * GD_STATUS_TEXT);
    return true;
}
static inline bool gd_cache_page_encode(const GdStatusPage* page, uint8_t* out) {
    if(!page || !out) return false;
    out[0] = page->flags; gd_write_u16(out + 1, page->age_seconds); gd_write_u16(out + 3, page->ttl_seconds);
    memcpy(out + 5, page->title, GD_STATUS_TITLE);
    memcpy(out + 5 + GD_STATUS_TITLE, page->rows, GD_STATUS_ROWS * GD_STATUS_TEXT);
    GdStatusPage checked;
    return gd_cache_page_decode(out, &checked);
}
static inline size_t gd_detail_encode(const GdStatus* status, const char* id, uint32_t request_sequence,
                                     bool complete, uint8_t* out, size_t capacity) {
    char checked[GD_NODE_ID_BYTES];
    if(!status || !out || !gd_detail_sequence_valid(status->sequence) || capacity < GD_DETAIL_BYTES || !gd_cache_copy_id(checked, id, false)) return 0;
    if(!gd_status_encode(status, out + 72, capacity - 72)) return 0;
    out[0] = 'G'; out[1] = 'V'; out[2] = 2; out[3] = complete ? GD_DETAIL_COMPLETE : 0;
    gd_cache_write_u32(out + 4, request_sequence); memcpy(out + 8, checked, GD_NODE_ID_BYTES);
    return GD_DETAIL_BYTES;
}
static inline bool gd_detail_decode(const uint8_t* wire, size_t size, GdDetail* out) {
    if(!wire || !out || size != GD_DETAIL_BYTES || wire[0] != 'G' || wire[1] != 'V' || wire[2] != 2 ||
       (wire[3] & ~GD_DETAIL_COMPLETE) || !gd_cache_id_valid((const char*)wire + 8, false)) return false;
    GdDetail result;
    if(!gd_status_decode(wire + 72, GD_STATUS_BYTES, &result.status) || !gd_detail_sequence_valid(result.status.sequence)) return false;
    memcpy(result.node_id, wire + 8, GD_NODE_ID_BYTES);
    result.request_sequence = gd_cache_read_u32(wire + 4); result.complete = (wire[3] & GD_DETAIL_COMPLETE) != 0;
    *out = result; return true;
}
static inline size_t gd_catalog_encode(const GdNodeCatalog* catalog, uint8_t type, uint8_t index,
                                      uint32_t sequence, uint8_t* out, size_t capacity) {
    if(!catalog || !out || !gd_catalog_sequence_valid(sequence) || catalog->count > GD_CATALOG_MAX_NODES ||
       type < GD_CATALOG_BEGIN || type > GD_CATALOG_COMMIT ||
       (type == GD_CATALOG_ENTRY && index >= catalog->count) ||
       (type != GD_CATALOG_ENTRY && index)) return 0;
    const size_t size = type == GD_CATALOG_ENTRY ? GD_CATALOG_ENTRY_BYTES : GD_CATALOG_HEADER_BYTES;
    const char* id = type == GD_CATALOG_ENTRY ? catalog->nodes[index].id : catalog->selected_id;
    char checked[GD_NODE_ID_BYTES];
    if(capacity < size || !gd_cache_copy_id(checked, id, catalog->count == 0)) return 0;
    memset(out, 0, size); out[0] = 'G'; out[1] = 'C'; out[2] = 2; out[3] = type;
    gd_cache_write_u32(out + 4, sequence); gd_cache_write_u32(out + 8, catalog->generation);
    out[12] = catalog->count; out[13] = index; memcpy(out + 16, checked, GD_NODE_ID_BYTES);
    if(type == GD_CATALOG_ENTRY && !gd_cache_page_encode(&catalog->nodes[index].system, out + 80)) return 0;
    return size;
}
static inline bool gd_catalog_decode(const uint8_t* wire, size_t size, GdCatalogPacket* out) {
    if(!wire || !out || size < GD_CATALOG_HEADER_BYTES || wire[0] != 'G' || wire[1] != 'C' || wire[2] != 2 ||
       wire[3] < GD_CATALOG_BEGIN || wire[3] > GD_CATALOG_COMMIT || wire[12] > GD_CATALOG_MAX_NODES ||
       wire[14] || wire[15] || !gd_catalog_sequence_valid(gd_cache_read_u32(wire + 4)) ||
       !gd_cache_id_valid((const char*)wire + 16, wire[12] == 0)) return false;
    const bool entry = wire[3] == GD_CATALOG_ENTRY;
    if(size != (entry ? GD_CATALOG_ENTRY_BYTES : GD_CATALOG_HEADER_BYTES) ||
       (entry ? wire[13] >= wire[12] : wire[13] != 0)) return false;
    GdCatalogPacket result; memset(&result, 0, sizeof(result));
    if(entry && !gd_cache_page_decode(wire + 80, &result.system)) return false;
    result.type = wire[3]; result.count = wire[12]; result.index = wire[13];
    result.transport_sequence = gd_cache_read_u32(wire + 4); result.generation = gd_cache_read_u32(wire + 8);
    memcpy(result.node_id, wire + 16, GD_NODE_ID_BYTES); *out = result; return true;
}
static inline bool gd_cache_packet_sequence(const uint8_t* wire, size_t size, uint32_t* sequence) {
    if(!wire || !sequence || size < 2) return false;
    if(wire[0] == 'G' && wire[1] == 'V') {
        GdDetail detail; if(!gd_detail_decode(wire, size, &detail)) return false;
        *sequence = detail.status.sequence; return true;
    }
    GdCatalogPacket packet; if(!gd_catalog_decode(wire, size, &packet)) return false;
    *sequence = packet.transport_sequence; return true;
}
static inline size_t gd_cache_control_encode(uint32_t sequence, const char* id, uint8_t* out, size_t capacity) {
    char checked[GD_NODE_ID_BYTES];
    if(!out || !sequence || capacity < GD_CACHE_CONTROL_BYTES || !gd_cache_copy_id(checked, id, false)) return 0;
    out[0] = 'G'; out[1] = 'S'; gd_cache_write_u32(out + 2, sequence);
    memcpy(out + 6, checked, GD_NODE_ID_BYTES); return GD_CACHE_CONTROL_BYTES;
}
static inline bool gd_cache_control_decode(const uint8_t* wire, size_t size, uint32_t* sequence,
                                          char* id, size_t capacity) {
    if(!wire || !sequence || !id || capacity < GD_NODE_ID_BYTES || size != GD_CACHE_CONTROL_BYTES ||
       wire[0] != 'G' || wire[1] != 'S' || !gd_cache_read_u32(wire + 2) ||
       !gd_cache_id_valid((const char*)wire + 6, false)) return false;
    *sequence = gd_cache_read_u32(wire + 2); memcpy(id, wire + 6, GD_NODE_ID_BYTES); return true;
}
