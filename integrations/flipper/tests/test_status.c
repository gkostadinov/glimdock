#include <assert.h>
#include <stdio.h>
#include "status_view.h"
#include "glimdock_hci.h"
#include "node_controls.h"

static void packet(uint8_t* wire, uint32_t sequence) {
    GdStatus status = {.sequence = sequence};
    const char* titles[] = {"System", "Alerts", "Agents", "Printer"};
    for(size_t i = 0; i < GD_STATUS_PAGES; ++i) {
        status.pages[i].flags = GD_STATUS_VALID;
        status.pages[i].age_seconds = 3;
        status.pages[i].ttl_seconds = 15;
        strcpy(status.pages[i].title, titles[i]);
        strcpy(status.pages[i].rows[0], "CPU 12% RAM 42%");
    }
    assert(gd_status_encode(&status, wire, GD_STATUS_BYTES));
}

static void node_packet(uint8_t* wire, uint32_t sequence, const char* name) {
    packet(wire, sequence);
    GdStatus status;
    assert(gd_status_decode(wire, GD_STATUS_BYTES, &status));
    memset(status.pages[0].rows[0], 0, GD_STATUS_TEXT);
    strcpy(status.pages[0].rows[0], name);
    assert(gd_status_encode(&status, wire, GD_STATUS_BYTES));
}

int main(void) {
    GdNodeControls controls = {0};
    GdNodeControlEvent control;
    assert(!gd_node_controls_peek(&controls, 0, &control));
    for(size_t i = 0; i < GD_NODE_CONTROL_QUEUE; ++i)
        assert(gd_node_controls_push(&controls, GD_BLE_NODE_NEXT, 100 + i));
    assert(!gd_node_controls_push(&controls, GD_BLE_NODE_PREVIOUS, 110));
    assert(gd_node_controls_peek(&controls, 200, &control) && control.sequence == 1);
    /* A BUSY update does not pop; a mismatched completion cannot pop another event. */
    assert(gd_node_controls_peek(&controls, 201, &control) && control.sequence == 1);
    gd_node_controls_sent(&controls, 99);
    assert(controls.count == GD_NODE_CONTROL_QUEUE);
    gd_node_controls_sent(&controls, control.sequence);
    assert(gd_node_controls_push(&controls, GD_BLE_NODE_PREVIOUS, 202));
    assert(gd_node_controls_peek(&controls, 203, &control) && control.sequence == 2);
    uint32_t sequence_before_reset = controls.sequence;
    gd_node_controls_clear(&controls);
    assert(!gd_node_controls_peek(&controls, 204, &control));
    assert(gd_node_controls_push(&controls, GD_BLE_NODE_PREVIOUS, 205));
    assert(gd_node_controls_peek(&controls, 206, &control) && control.sequence == sequence_before_reset + 1);
    assert(!gd_node_controls_peek(&controls, 2206, &control));
    gd_node_controls_clear(&controls);
    controls.sequence = UINT32_MAX;
    assert(gd_node_controls_push(&controls, GD_BLE_NODE_NEXT, UINT32_MAX - 500));
    assert(gd_node_controls_peek(&controls, 500, &control) && control.sequence == 1);
    assert(!gd_node_controls_peek(&controls, 1600, &control));
    uint8_t control_packet[GD_BLE_CONTROL_BYTES];
    uint32_t event_sequence = 0;
    uint8_t direction = 0;
    assert(gd_ble_control_encode(0x12345678, GD_BLE_NODE_PREVIOUS, control_packet));
    assert(gd_ble_control_decode(control_packet, sizeof(control_packet), &event_sequence, &direction));
    assert(event_sequence == 0x12345678 && direction == GD_BLE_NODE_PREVIOUS);
    assert(!gd_ble_control_encode(0, GD_BLE_NODE_NEXT, control_packet));
    assert(!gd_ble_control_encode(1, 0, control_packet));
    control_packet[7] = 1;
    assert(!gd_ble_control_decode(control_packet, sizeof(control_packet), &event_sequence, &direction));
    control_packet[7] = 0;
    assert(!gd_ble_control_decode(control_packet, sizeof(control_packet) - 1, &event_sequence, &direction));
    /* Real ST HCI UART vendor-event envelope, with three attribute bytes. */
    uint8_t event[] = {0x04, 0xFF, 13, 0x01, 0x0C, 1, 0, 0x34, 0x12, 0, 0, 3, 0, 'A', 'B', 'C'};
    GdHciModified modified;
    assert(gd_hci_attribute_modified(event, sizeof(event), &modified));
    assert(modified.handle == 0x1234 && modified.offset == 0 && modified.size == 3);
    assert(!memcmp(modified.data, "ABC", 3));
    for(size_t truncated = 0; truncated < sizeof(event); ++truncated)
        assert(!gd_hci_attribute_modified(event, truncated, &modified));
    event[11] = 4;
    assert(!gd_hci_attribute_modified(event, sizeof(event), &modified));
    event[11] = 3;
    event[3] = 2;
    assert(!gd_hci_attribute_modified(event, sizeof(event), &modified));
    event[3] = 1;
    event[0] = 2;
    assert(!gd_hci_attribute_modified(event, sizeof(event), &modified));
    /* The maximum status write still fits the pinned 255-byte HCI event.
     * Every shorter physical buffer must reject its advertised full length. */
    uint8_t large_event[13 + GD_BLE_CHUNK_MAX] = {0x04, 0xFF, 10 + GD_BLE_CHUNK_MAX, 0x01, 0x0C};
    gd_write_u16(large_event + 7, 0x1234);
    gd_write_u16(large_event + 11, GD_BLE_CHUNK_MAX);
    assert(GD_BLE_CHUNK_MAX == 232 && large_event[2] == 242);
    assert(gd_hci_attribute_modified(large_event, sizeof(large_event), &modified));
    assert(modified.handle == 0x1234 && modified.size == GD_BLE_CHUNK_MAX);
    for(size_t truncated = 0; truncated < sizeof(large_event); ++truncated)
        assert(!gd_hci_attribute_modified(large_event, truncated, &modified));
    gd_write_u16(large_event + 11, GD_BLE_CHUNK_MAX + 1);
    assert(!gd_hci_attribute_modified(large_event, sizeof(large_event), &modified));
    assert(GD_STATUS_BYTES == 444);
    uint8_t wire[GD_STATUS_BYTES];
    packet(wire, 42);
    GdView view = {0};
    assert(gd_view_receive(&view, wire, sizeof(wire), 1000));
    assert(view.available && view.status.sequence == 42);
    assert(!strcmp(view.status.pages[3].title, "Printer"));
    assert(!gd_view_page_stale(&view, 1000));

    /* All four directions wrap and preserve the selected page on new data. */
    gd_view_move(&view, -1);
    assert(view.selected == 3);
    gd_view_move(&view, 1);
    assert(view.selected == 0);
    for(size_t i = 0; i < 4; ++i) gd_view_move(&view, 1);
    assert(view.selected == 0);
    gd_view_move(&view, 1);

    /* Invalid/oversized/truncated frames cannot replace a good display. */
    GdView before = view;
    assert(!gd_view_receive(&view, wire, sizeof(wire) - 1, 2000));
    assert(!memcmp(&view, &before, sizeof(view)));
    assert(!gd_view_receive(&view, NULL, sizeof(wire), 2000));
    wire[3] = 9;
    assert(!gd_view_receive(&view, wire, sizeof(wire), 2000));
    assert(!memcmp(&view, &before, sizeof(view)));
    packet(wire, 42);
    wire[8] = 0x80;
    assert(!gd_view_receive(&view, wire, sizeof(wire), 2000));
    packet(wire, 42);
    memset(wire + 8 + 5, 'x', GD_STATUS_TITLE);
    assert(!gd_view_receive(&view, wire, sizeof(wire), 2000));

    /* A duplicate received much later proves transport, never source freshness. */
    packet(wire, 42);
    assert(gd_view_receive(&view, wire, sizeof(wire), 14000));
    assert(view.received_ms == 1000 && view.packet_ms == 14000);
    assert(gd_view_page_stale(&view, 14000));
    assert(!gd_view_stream_stale(&view, 14000));
    assert(gd_view_stream_stale(&view, 24001));
    packet(wire, 43);
    assert(gd_view_receive(&view, wire, sizeof(wire), 25000));
    assert(view.selected == 1 && !gd_view_page_stale(&view, 25000));

    /* Explicit stale/unknown/invalid source states override a fresh BLE receipt. */
    view.status.pages[1].flags |= GD_STATUS_STALE;
    assert(gd_view_page_stale(&view, 25000));
    view.status.pages[1].flags = GD_STATUS_VALID | GD_STATUS_DEMO;
    view.status.pages[1].age_seconds = GD_STATUS_UNKNOWN_AGE;
    assert(gd_view_page_stale(&view, 25000));
    view.status.pages[1].age_seconds = 0;
    view.status.pages[1].flags = 0;
    assert(gd_view_page_stale(&view, 25000));

    /* Unsigned millisecond deltas keep aging across the hardware tick wrap. */
    packet(wire, 44);
    assert(gd_view_receive(&view, wire, sizeof(wire), UINT32_MAX - 499));
    assert(gd_view_elapsed(&view, 500) == 1);
    assert(!gd_view_page_stale(&view, 500));

    /* Accepted node controls show old readings until the actual node changes. */
    GdView switching = {0};
    gd_view_node_switch_start(&switching, 0);
    assert(!gd_view_node_switch_active(&switching, 0));
    node_packet(wire, 1, "Node A");
    assert(gd_view_receive(&switching, wire, sizeof(wire), 1000));
    switching.selected = 2;
    gd_view_node_switch_start(&switching, 1100);
    assert(gd_view_node_switch_active(&switching, 1100));
    node_packet(wire, 2, "Node A");
    assert(gd_view_receive(&switching, wire, sizeof(wire), 1200));
    assert(gd_view_node_switch_active(&switching, 1200));
    assert(switching.selected == 2 && switching.received_ms == 1200);
    gd_view_node_switch_start(&switching, 1300); /* Rapid press renews local feedback only. */
    assert(switching.received_ms == 1200);
    assert(gd_view_node_switch_active(&switching, 3299));
    assert(!gd_view_node_switch_active(&switching, 3300));
    gd_view_node_switch_start(&switching, 3400);
    node_packet(wire, 3, "Node B");
    assert(gd_view_receive(&switching, wire, sizeof(wire), 3500));
    assert(!switching.node_switch_pending && switching.selected == 2);
    gd_view_node_switch_start(&switching, UINT32_MAX - 499);
    assert(gd_view_node_switch_active(&switching, 500));
    assert(!gd_view_node_switch_active(&switching, 1500));
    puts("Flipper decoder, navigation and freshness checks passed.");
    return 0;
}
