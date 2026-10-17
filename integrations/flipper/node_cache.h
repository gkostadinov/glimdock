#pragma once
#include "flipper_cache_protocol.h"

typedef enum { GdNodeCacheRejected, GdNodeCacheAccepted, GdNodeCacheCommitted } GdNodeCacheResult;
#define GD_NODE_CACHE_REJECTED GdNodeCacheRejected
#define GD_NODE_CACHE_ACCEPTED GdNodeCacheAccepted
#define GD_NODE_CACHE_COMMITTED GdNodeCacheCommitted

typedef struct {
    char id[GD_NODE_ID_BYTES];
    GdStatusPage system;
    GdStatus detail;
    uint32_t summary_received_ms, detail_received_ms, detail_request_sequence, last_detail_sequence;
    bool present, has_detail, detail_complete, detail_sequence_seen;
} GdCachedNode;
typedef struct {
    GdCachedNode nodes[GD_CACHE_MAX_NODES];
    uint8_t order[GD_CACHE_MAX_NODES], count, selected;
    uint32_t request_sequence;
    bool waiting_for_detail;
    GdNodeCatalog staging;
    uint32_t staging_received_ms[GD_CACHE_MAX_NODES];
    uint32_t staging_sequences[GD_CACHE_MAX_NODES];
    uint32_t begin_sequence, commit_sequence, catalog_generation;
    uint32_t active_begin_sequence, active_entry_sequences[GD_CACHE_MAX_NODES];
    char active_selected_id[GD_NODE_ID_BYTES];
    uint8_t staging_seen;
    bool staging_active, catalog_sequence_seen;
} GdNodeCache;

static inline void gd_node_cache_init(GdNodeCache* cache);
static inline void gd_node_cache_reset_transport(GdNodeCache* cache);
static inline GdNodeCacheResult gd_node_cache_accept_catalog(GdNodeCache* cache, const uint8_t* packet, size_t size, uint32_t now);
static inline bool gd_node_cache_accept_detail(GdNodeCache* cache, const uint8_t* packet, size_t size, uint32_t now);
static inline bool gd_node_cache_select_step(GdNodeCache* cache, int direction, uint32_t now, uint32_t* request_sequence, char id[GD_NODE_ID_BYTES]);
static inline bool gd_node_cache_project(const GdNodeCache* cache, GdStatus* status, uint32_t now);
static inline const char* gd_node_cache_current_id(const GdNodeCache* cache);
static inline bool gd_node_cache_selection(const GdNodeCache* cache, uint32_t* sequence, char id[GD_NODE_ID_BYTES]);
static inline bool gd_node_cache_selected_loading(const GdNodeCache* cache);

static inline void gd_node_cache_init(GdNodeCache* cache) { if(cache) memset(cache, 0, sizeof(*cache)); }
static inline int gd_node_cache_slot(const GdNodeCache* cache, const char* id) {
    for(unsigned i = 0; i < GD_CACHE_MAX_NODES; ++i)
        if(cache->nodes[i].present && !strcmp(cache->nodes[i].id, id)) return (int)i;
    return -1;
}
static inline const char* gd_node_cache_current_id(const GdNodeCache* cache) {
    return cache && cache->count && cache->selected < cache->count ? cache->nodes[cache->order[cache->selected]].id : "";
}
static inline bool gd_node_cache_selected_loading(const GdNodeCache* cache) { return cache && cache->waiting_for_detail; }
static inline void gd_node_cache_next_request(GdNodeCache* cache) {
    if(!++cache->request_sequence) ++cache->request_sequence;
    cache->waiting_for_detail = cache->count != 0;
}
static inline bool gd_node_cache_selection(const GdNodeCache* cache, uint32_t* sequence, char id[GD_NODE_ID_BYTES]) {
    if(!cache || !sequence || !id || !cache->count || !cache->request_sequence) return false;
    *sequence = cache->request_sequence; memcpy(id, gd_node_cache_current_id(cache), GD_NODE_ID_BYTES); return true;
}
static inline void gd_node_cache_reset_transport(GdNodeCache* cache) {
    if(!cache) return;
    cache->staging_active = false; cache->catalog_sequence_seen = false;
    for(unsigned i = 0; i < GD_CACHE_MAX_NODES; ++i) cache->nodes[i].detail_sequence_seen = false;
    cache->waiting_for_detail = cache->count != 0;
}
static inline bool gd_node_cache_page_equal(const GdStatusPage* a, const GdStatusPage* b) {
    return a->flags == b->flags && a->age_seconds == b->age_seconds && a->ttl_seconds == b->ttl_seconds &&
           !memcmp(a->title, b->title, GD_STATUS_TITLE) && !memcmp(a->rows, b->rows, GD_STATUS_ROWS * GD_STATUS_TEXT);
}
static inline bool gd_node_cache_newer(uint32_t next, uint32_t previous) {
    const uint32_t delta = next - previous; return delta && delta < 0x80000000u;
}
static inline GdNodeCacheResult gd_node_cache_accept_catalog(GdNodeCache* cache, const uint8_t* wire, size_t size, uint32_t now) {
    GdCatalogPacket packet;
    if(!cache || !gd_catalog_decode(wire, size, &packet)) return GdNodeCacheRejected;
    // Exact retransmissions are acknowledged without renewing sample receipt times.
    if(cache->catalog_sequence_seen && packet.generation == cache->catalog_generation) {
        if(packet.count != cache->count) return GdNodeCacheRejected;
        if(packet.type == GD_CATALOG_ENTRY) {
            const GdCachedNode* node = &cache->nodes[cache->order[packet.index]];
            return packet.transport_sequence == cache->active_entry_sequences[packet.index] &&
                   !strcmp(packet.node_id, node->id) && gd_node_cache_page_equal(&packet.system, &node->system) ?
                   GdNodeCacheAccepted : GdNodeCacheRejected;
        }
        return !strcmp(packet.node_id, cache->active_selected_id) && packet.transport_sequence ==
               (packet.type == GD_CATALOG_BEGIN ? cache->active_begin_sequence : cache->commit_sequence) ?
               GdNodeCacheAccepted : GdNodeCacheRejected;
    }
    if(cache->catalog_sequence_seen && !gd_node_cache_newer(packet.generation, cache->catalog_generation)) return GdNodeCacheRejected;
    if(packet.type == GD_CATALOG_BEGIN) {
        if(cache->staging_active && packet.generation == cache->staging.generation) {
            return packet.transport_sequence == cache->begin_sequence && packet.count == cache->staging.count &&
                   !strcmp(packet.node_id, cache->staging.selected_id) ? GdNodeCacheAccepted : GdNodeCacheRejected;
        }
        if(cache->staging_active && !gd_node_cache_newer(packet.generation, cache->staging.generation)) return GdNodeCacheRejected;
        memset(&cache->staging, 0, sizeof(cache->staging));
        cache->staging.generation = packet.generation; cache->staging.count = packet.count;
        memcpy(cache->staging.selected_id, packet.node_id, GD_NODE_ID_BYTES);
        cache->begin_sequence = packet.transport_sequence; cache->staging_seen = 0; cache->staging_active = true;
        return GdNodeCacheAccepted;
    }
    if(!cache->staging_active || packet.generation != cache->staging.generation || packet.count != cache->staging.count)
        return GdNodeCacheRejected;
    if(packet.type == GD_CATALOG_ENTRY) {
        const uint8_t bit = (uint8_t)(1u << packet.index);
        GdCatalogNode* entry = &cache->staging.nodes[packet.index];
        if(cache->staging_seen & bit) {
            return cache->staging_sequences[packet.index] == packet.transport_sequence && !strcmp(entry->id, packet.node_id) &&
                   gd_node_cache_page_equal(&entry->system, &packet.system) ? GdNodeCacheAccepted : GdNodeCacheRejected;
        }
        for(unsigned i = 0; i < packet.count; ++i)
            if((cache->staging_seen & (1u << i)) && !strcmp(cache->staging.nodes[i].id, packet.node_id)) return GdNodeCacheRejected;
        memcpy(entry->id, packet.node_id, GD_NODE_ID_BYTES); entry->system = packet.system;
        cache->staging_received_ms[packet.index] = now; cache->staging_sequences[packet.index] = packet.transport_sequence;
        cache->staging_seen |= bit; return GdNodeCacheAccepted;
    }
    if(strcmp(packet.node_id, cache->staging.selected_id) || cache->staging_seen != (uint8_t)((1u << packet.count) - 1u))
        return GdNodeCacheRejected;
    char previous[GD_NODE_ID_BYTES] = {0};
    if(cache->count) memcpy(previous, gd_node_cache_current_id(cache), GD_NODE_ID_BYTES);
    // Stable slots preserve full details through catalog reorder; no stack array of eight statuses.
    for(unsigned slot = 0; slot < GD_CACHE_MAX_NODES; ++slot) {
        bool retained = false;
        for(unsigned i = 0; i < packet.count; ++i)
            if(!strcmp(cache->nodes[slot].id, cache->staging.nodes[i].id)) retained = true;
        if(!retained) cache->nodes[slot].present = false;
    }
    for(unsigned i = 0; i < packet.count; ++i) {
        int slot = gd_node_cache_slot(cache, cache->staging.nodes[i].id);
        if(slot < 0) {
            for(unsigned j = 0; j < GD_CACHE_MAX_NODES; ++j) if(!cache->nodes[j].present) { slot = (int)j; break; }
            GdCachedNode* node = &cache->nodes[slot]; memset(node, 0, sizeof(*node));
            memcpy(node->id, cache->staging.nodes[i].id, GD_NODE_ID_BYTES); node->present = true;
        }
        cache->order[i] = (uint8_t)slot;
        // A new generation carries an authoritative projected source age;
        // same-generation retries return above without renewing this baseline.
        cache->nodes[slot].system = cache->staging.nodes[i].system;
        cache->nodes[slot].summary_received_ms = cache->staging_received_ms[i];
        cache->active_entry_sequences[i] = cache->staging_sequences[i];
    }
    cache->count = packet.count; cache->selected = 0;
    bool retained_selection = false;
    for(unsigned i = 0; i < cache->count; ++i) if(!strcmp(cache->nodes[cache->order[i]].id, previous)) {
        cache->selected = (uint8_t)i; retained_selection = true; break;
    }
    if(!retained_selection) {
        for(unsigned i = 0; i < cache->count; ++i) if(!strcmp(cache->nodes[cache->order[i]].id, packet.node_id)) cache->selected = (uint8_t)i;
        if(cache->count) gd_node_cache_next_request(cache); else cache->waiting_for_detail = false;
    }
    cache->catalog_generation = packet.generation; cache->catalog_sequence_seen = true; cache->staging_active = false;
    cache->active_begin_sequence = cache->begin_sequence; cache->commit_sequence = packet.transport_sequence;
    memcpy(cache->active_selected_id, packet.node_id, GD_NODE_ID_BYTES);
    return GdNodeCacheCommitted;
}
static inline bool gd_node_cache_accept_detail(GdNodeCache* cache, const uint8_t* wire, size_t size, uint32_t now) {
    GdDetail detail;
    if(!cache || !gd_detail_decode(wire, size, &detail)) return false;
    int slot = gd_node_cache_slot(cache, detail.node_id);
    // Valid traffic for an introducing/removed node must not stall catalog delivery.
    if(slot < 0 || !detail.complete) return true;
    GdCachedNode* node = &cache->nodes[slot];
    if(node->has_detail && detail.status.sequence == node->detail.sequence &&
       detail.request_sequence == node->detail_request_sequence && detail.complete == node->detail_complete) {
        bool identical = true;
        for(unsigned i = 0; i < GD_STATUS_PAGES; ++i)
            if(!gd_node_cache_page_equal(&detail.status.pages[i], &node->detail.pages[i])) identical = false;
        if(identical) {
            if(detail.request_sequence == cache->request_sequence &&
               !strcmp(detail.node_id, gd_node_cache_current_id(cache))) cache->waiting_for_detail = false;
            return true;
        }
        if(node->detail_sequence_seen) return false;
    }
    if(node->detail_sequence_seen) {
        const uint32_t delta = (detail.status.sequence - node->last_detail_sequence) & GD_CACHE_DETAIL_SEQUENCE_MAX;
        if(!delta || delta >= 0x40000000u) return true;
    }
    node->last_detail_sequence = detail.status.sequence; node->detail_sequence_seen = true;
    // A late response to an earlier visit cannot replace this visit's completed data.
    if(node->has_detail && node->detail_complete && node->detail_request_sequence == cache->request_sequence &&
       detail.request_sequence != cache->request_sequence && !strcmp(detail.node_id, gd_node_cache_current_id(cache))) return true;
    // An obsolete request may refresh that node's cache, never its selected ID or current waiting state.
    memcpy(&node->detail, &detail.status, sizeof(GdStatus));
    node->detail_received_ms = now; node->detail_request_sequence = detail.request_sequence;
    node->has_detail = true; node->detail_complete = detail.complete; node->detail_sequence_seen = true;
    if(detail.complete && detail.request_sequence == cache->request_sequence &&
       !strcmp(detail.node_id, gd_node_cache_current_id(cache))) cache->waiting_for_detail = false;
    return true;
}
static inline bool gd_node_cache_select_step(GdNodeCache* cache, int direction, uint32_t now, uint32_t* request_sequence, char id[GD_NODE_ID_BYTES]) {
    (void)now;
    if(!cache || !request_sequence || !id || cache->count < 2 || (direction != -1 && direction != 1)) return false;
    cache->selected = (uint8_t)((cache->selected + cache->count + direction) % cache->count);
    gd_node_cache_next_request(cache); return gd_node_cache_selection(cache, request_sequence, id);
}
static inline void gd_node_cache_age_page(GdStatusPage* page, uint32_t elapsed) {
    const bool stale = gd_status_page_stale(page, elapsed);
    if(page->age_seconds != GD_STATUS_UNKNOWN_AGE) {
        const uint32_t age = (uint32_t)page->age_seconds + elapsed;
        page->age_seconds = (uint16_t)(age > 65534 ? 65534 : age);
    }
    if(stale) page->flags |= GD_STATUS_STALE;
}
static inline bool gd_node_cache_project(const GdNodeCache* cache, GdStatus* out, uint32_t now) {
    if(!cache || !out || !cache->count || cache->selected >= cache->count) return false;
    const GdCachedNode* node = &cache->nodes[cache->order[cache->selected]];
    if(node->has_detail) {
        memcpy(out, &node->detail, sizeof(*out));
        for(unsigned i = 0; i < GD_STATUS_PAGES; ++i)
            gd_node_cache_age_page(&out->pages[i], (uint32_t)(now - node->detail_received_ms) / 1000u);
    } else {
        memset(out, 0, sizeof(*out));
        const char* titles[] = {"System", "Alerts", "Agents", "Printer"};
        for(unsigned i = 0; i < GD_STATUS_PAGES; ++i) {
            GdStatusPage* page = &out->pages[i]; page->flags = GD_STATUS_STALE | (node->system.flags & GD_STATUS_DEMO);
            page->age_seconds = GD_STATUS_UNKNOWN_AGE; page->ttl_seconds = 15;
            memcpy(page->title, titles[i], strlen(titles[i]) + 1);
            memcpy(page->rows[0], node->system.rows[0], GD_STATUS_TEXT);
            memcpy(page->rows[1], "Details not loaded", 19);
        }
    }
    GdStatusPage summary = node->system;
    gd_node_cache_age_page(&summary, (uint32_t)(now - node->summary_received_ms) / 1000u);
    GdStatusPage* detailed = &out->pages[0];
    const bool summary_valid = (summary.flags & GD_STATUS_VALID) != 0;
    const bool detail_valid = (detailed->flags & GD_STATUS_VALID) != 0;
    const bool summary_fresh = !gd_status_page_stale(&summary, 0);
    const bool detail_fresh = !gd_status_page_stale(detailed, 0);
    if(!node->has_detail || (summary_valid && (!detail_valid ||
       (summary_fresh && !detail_fresh) ||
       (summary_fresh == detail_fresh && summary.age_seconds != GD_STATUS_UNKNOWN_AGE &&
        (detailed->age_seconds == GD_STATUS_UNKNOWN_AGE || summary.age_seconds <= detailed->age_seconds))))) *detailed = summary;
    return true;
}
