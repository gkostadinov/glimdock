#pragma once
#include "flipper_ble_protocol.h"

#define GD_NODE_CONTROL_QUEUE 8u
#define GD_NODE_CONTROL_TTL_MS 2000u

typedef struct {
    uint32_t sequence;
    uint32_t created_ms;
    uint8_t direction;
} GdNodeControlEvent;

typedef struct {
    GdNodeControlEvent events[GD_NODE_CONTROL_QUEUE];
    uint32_t sequence;
    uint8_t head;
    uint8_t count;
} GdNodeControls;

/* Link changes discard pending actions without restarting the event counter. */
static inline void gd_node_controls_clear(GdNodeControls* queue) {
    queue->head = queue->count = 0;
}
static inline bool gd_node_controls_push(GdNodeControls* queue, uint8_t direction, uint32_t now) {
    if(queue->count == GD_NODE_CONTROL_QUEUE ||
       (direction != GD_BLE_NODE_NEXT && direction != GD_BLE_NODE_PREVIOUS)) return false;
    if(!++queue->sequence) ++queue->sequence;
    uint8_t slot = (queue->head + queue->count) % GD_NODE_CONTROL_QUEUE;
    GdNodeControlEvent event = {queue->sequence, now, direction};
    queue->events[slot] = event;
    queue->count++;
    return true;
}
static inline bool gd_node_controls_peek(GdNodeControls* queue, uint32_t now, GdNodeControlEvent* event) {
    while(queue->count && (uint32_t)(now - queue->events[queue->head].created_ms) > GD_NODE_CONTROL_TTL_MS) {
        queue->head = (queue->head + 1) % GD_NODE_CONTROL_QUEUE;
        queue->count--;
    }
    if(!queue->count) return false;
    *event = queue->events[queue->head];
    return true;
}
static inline void gd_node_controls_sent(GdNodeControls* queue, uint32_t sequence) {
    if(queue->count && queue->events[queue->head].sequence == sequence) {
        queue->head = (queue->head + 1) % GD_NODE_CONTROL_QUEUE;
        queue->count--;
    }
}
