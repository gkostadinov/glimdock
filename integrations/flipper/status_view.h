#pragma once
#include "flipper_protocol.h"

#define GD_STREAM_TIMEOUT_MS 10000u
#define GD_NODE_SWITCH_TIMEOUT_MS 2000u

typedef struct {
    GdStatus status;
    uint32_t received_ms;
    uint32_t packet_ms;
    uint32_t node_switch_started_ms;
    char node_switch_from[GD_STATUS_TEXT];
    uint8_t selected;
    bool available;
    bool node_switch_pending;
} GdView;

/* Local feedback describes the readings already on screen, never new data. */
static inline void gd_view_node_switch_start(GdView* view, uint32_t now) {
    if(!view->available) return;
    memcpy(view->node_switch_from, view->status.pages[0].rows[0], GD_STATUS_TEXT);
    view->node_switch_started_ms = now;
    view->node_switch_pending = true;
}

static inline bool gd_view_node_switch_active(const GdView* view, uint32_t now) {
    return view->node_switch_pending &&
           (uint32_t)(now - view->node_switch_started_ms) < GD_NODE_SWITCH_TIMEOUT_MS;
}

/* Repeated transport receipts cannot renew the source's original age. */
static inline bool gd_view_receive(GdView* view, const uint8_t* wire, size_t size, uint32_t now) {
    GdStatus next;
    if(!gd_status_decode(wire, size, &next)) return false;
    view->packet_ms = now;
    if(!view->available || next.sequence != view->status.sequence) {
        view->status = next;
        view->received_ms = now;
        view->available = true;
        /* A status already in flight for the old node is not a switch receipt. */
        if(view->node_switch_pending &&
           strcmp(view->status.pages[0].rows[0], view->node_switch_from))
            view->node_switch_pending = false;
    }
    return true;
}

static inline void gd_view_move(GdView* view, int direction) {
    view->selected = (uint8_t)((view->selected + GD_STATUS_PAGES + direction) % GD_STATUS_PAGES);
}

static inline uint32_t gd_view_elapsed(const GdView* view, uint32_t now) {
    return (uint32_t)(now - view->received_ms) / 1000u;
}

static inline bool gd_view_stream_stale(const GdView* view, uint32_t now) {
    return !view->available || (uint32_t)(now - view->packet_ms) > GD_STREAM_TIMEOUT_MS;
}

static inline bool gd_view_page_stale(const GdView* view, uint32_t now) {
    return gd_view_stream_stale(view, now) ||
           gd_status_page_stale(&view->status.pages[view->selected], gd_view_elapsed(view, now));
}
