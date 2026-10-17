/* Synthetic dedicated-BLE transport tests; builds unchanged as C99 and C++17. */
#include "flipper_ble_protocol.h"
#include "status_view.h"
#include <assert.h>
#include <stdio.h>

static void make_frame(uint32_t sequence, uint8_t frame[GD_DETAIL_BYTES]) {
    GdStatus status;
    memset(&status, 0, sizeof(status));
    status.sequence = sequence;
    for(unsigned i = 0; i < GD_STATUS_PAGES; ++i) {
        status.pages[i].flags = GD_STATUS_VALID;
        status.pages[i].age_seconds = 3;
        status.pages[i].ttl_seconds = 15;
        strcpy(status.pages[i].title, "Status");
        strcpy(status.pages[i].rows[0], "Synthetic sample");
    }
    assert(gd_detail_encode(&status, "test-server", 1, true, frame, GD_DETAIL_BYTES) == GD_DETAIL_BYTES);
}

static void reset_receiver(GdBleAssembler* receiver) {
    memset(receiver, 0x9a, sizeof(*receiver));
    gd_ble_reset(receiver);
    assert(!receiver->active && !receiver->used && !receiver->sequence);
    for(size_t i = 0; i < GD_DETAIL_BYTES; ++i) assert(receiver->frame[i] == 0);
}

static void begin_partial(GdBleAssembler* receiver, const uint8_t* frame) {
    uint8_t packet[GD_BLE_CHUNK_MAX];
    size_t size = gd_ble_chunk_encode(frame, 7, 0, 100, packet, sizeof(packet));
    assert(size == GD_BLE_CHUNK_HEADER + 100);
    assert(gd_ble_chunk_receive(receiver, packet, size) == GdBleChunkPartial);
    assert(receiver->active && receiver->sequence == 7 && receiver->used == 100);
}

static void encoding(void) {
    assert(GD_BLE_CHUNK_HEADER == 8 && GD_BLE_CHUNK_PAYLOAD == 224 && GD_BLE_CHUNK_MAX == 232);
    uint8_t frame[GD_DETAIL_BYTES];
    for(size_t i = 0; i < sizeof(frame); ++i) frame[i] = (uint8_t)(i + 1);
    uint8_t packet[GD_BLE_CHUNK_MAX + 2];
    memset(packet, 0xa5, sizeof(packet));
    assert(gd_ble_chunk_encode(frame, 0x78563412, 2, 3, packet + 1,
                               sizeof(packet) - 2) == 11);
    const uint8_t expected[] = {'G', 'D', 0x12, 0x34, 0x56, 0x78, 2, 0, 3, 4, 5};
    assert(!memcmp(packet + 1, expected, sizeof(expected)));
    assert(packet[0] == 0xa5 && packet[12] == 0xa5 && packet[sizeof(packet) - 1] == 0xa5);
    memset(packet, 0xa5, sizeof(packet));
    assert(!gd_ble_chunk_encode(frame, 1, 0, 3, packet + 1, 10));
    assert(!gd_ble_chunk_encode(frame, 1, 0, 0, packet + 1, sizeof(packet) - 2));
    assert(!gd_ble_chunk_encode(frame, 1, 0, GD_BLE_CHUNK_PAYLOAD + 1, packet + 1, sizeof(packet) - 2));
    assert(!gd_ble_chunk_encode(frame, 1, GD_DETAIL_BYTES, 1, packet + 1, sizeof(packet) - 2));
    assert(!gd_ble_chunk_encode(frame, 1, GD_DETAIL_BYTES - 1, 2, packet + 1, sizeof(packet) - 2));
    assert(!gd_ble_chunk_encode(frame, 1, UINT16_MAX, 1, packet + 1, sizeof(packet) - 2));
    assert(!gd_ble_chunk_encode(NULL, 1, 0, 3, packet + 1, sizeof(packet) - 2));
    assert(!gd_ble_chunk_encode(frame, 1, 0, 3, NULL, sizeof(packet) - 2));
    for(size_t i = 0; i < sizeof(packet); ++i) assert(packet[i] == 0xa5);
}

static void full_frames(void) {
    uint8_t frame[GD_DETAIL_BYTES];
    uint8_t packet[GD_BLE_CHUNK_MAX];
    make_frame(GD_CACHE_DETAIL_SEQUENCE_MAX, frame);
    /* At negotiated MTU 247, the 516-byte envelope needs three bounded writes. */
    GdBleAssembler three_writes;
    reset_receiver(&three_writes);
    size_t first = gd_ble_chunk_encode(frame, GD_CACHE_DETAIL_SEQUENCE_MAX, 0, 224, packet, sizeof(packet));
    assert(first == 232 && first <= 247 - 3);
    assert(gd_ble_chunk_receive(&three_writes, packet, first) == GdBleChunkPartial);
    size_t second = gd_ble_chunk_encode(frame, GD_CACHE_DETAIL_SEQUENCE_MAX, 224, 224, packet, sizeof(packet));
    assert(second == 232 && second <= 247 - 3);
    assert(gd_ble_chunk_receive(&three_writes, packet, second) == GdBleChunkPartial);
    size_t third = gd_ble_chunk_encode(frame, GD_CACHE_DETAIL_SEQUENCE_MAX, 448, 68, packet, sizeof(packet));
    assert(third == 76 && third <= 247 - 3);
    assert(gd_ble_chunk_receive(&three_writes, packet, third) == GdBleChunkComplete);
    assert(!memcmp(three_writes.frame, frame, sizeof(frame)));
    /* Includes ATT MTU 23's 12-byte payload as well as maximum-size writes. */
    for(size_t payload = 1; payload <= GD_BLE_CHUNK_PAYLOAD; ++payload) {
        GdBleAssembler receiver;
        reset_receiver(&receiver);
        for(size_t offset = 0; offset < GD_DETAIL_BYTES;) {
            size_t count = GD_DETAIL_BYTES - offset;
            if(count > payload) count = payload;
            size_t size = gd_ble_chunk_encode(frame, GD_CACHE_DETAIL_SEQUENCE_MAX, (uint16_t)offset,
                                              count, packet, sizeof(packet));
            assert(size == GD_BLE_CHUNK_HEADER + count);
            GdBleChunkResult result = gd_ble_chunk_receive(&receiver, packet, size);
            offset += count;
            assert(result == (offset == GD_DETAIL_BYTES ? GdBleChunkComplete : GdBleChunkPartial));
            assert(receiver.used == offset);
            assert(receiver.active == (offset != GD_DETAIL_BYTES));
        }
        assert(receiver.sequence == GD_CACHE_DETAIL_SEQUENCE_MAX);
        assert(!memcmp(receiver.frame, frame, sizeof(frame)));
        GdDetail decoded;
        assert(gd_detail_decode(receiver.frame, sizeof(receiver.frame), &decoded));
        assert(decoded.status.sequence == receiver.sequence);
        /* A later frame can start over, including wrapped zero sequence. */
        size_t size = gd_ble_chunk_encode(frame, 0, 0, 1, packet, sizeof(packet));
        assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkPartial);
        assert(receiver.active && receiver.used == 1 && receiver.sequence == 0);
    }
}

static void malformed_and_retry(void) {
    uint8_t frame[GD_DETAIL_BYTES], packet[GD_BLE_CHUNK_MAX + 1];
    make_frame(7, frame);
    GdBleAssembler receiver;
    reset_receiver(&receiver);
    for(size_t length = 0; length <= GD_BLE_CHUNK_HEADER; ++length) {
        begin_partial(&receiver, frame);
        assert(gd_ble_chunk_receive(&receiver, packet, length) == GdBleChunkRejected);
        assert(!receiver.active && !receiver.used);
    }
    begin_partial(&receiver, frame);
    assert(gd_ble_chunk_receive(&receiver, NULL, 20) == GdBleChunkRejected);
    assert(!receiver.active && !receiver.used);
    assert(gd_ble_chunk_receive(NULL, packet, 20) == GdBleChunkRejected);
    begin_partial(&receiver, frame);
    assert(gd_ble_chunk_receive(&receiver, packet, sizeof(packet)) == GdBleChunkRejected);
    assert(!receiver.active && !receiver.used);
    for(unsigned header = 0; header < 2; ++header) {
        begin_partial(&receiver, frame);
        size_t size = gd_ble_chunk_encode(frame, 7, 100, 20, packet, sizeof(packet));
        packet[header] ^= 0x80;
        assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkRejected);
        assert(!receiver.active && !receiver.used);
    }
    /* Missing, duplicate, out-of-order, interleaved and out-of-bounds fragments. */
    const uint16_t offsets[] = {99, 101, 180, GD_DETAIL_BYTES, UINT16_MAX};
    for(size_t i = 0; i < sizeof(offsets) / sizeof(offsets[0]); ++i) {
        begin_partial(&receiver, frame);
        size_t size = gd_ble_chunk_encode(frame, 7, 100, 20, packet, sizeof(packet));
        gd_write_u16(packet + 6, offsets[i]);
        assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkRejected);
        assert(!receiver.active && !receiver.used);
    }
    begin_partial(&receiver, frame);
    size_t size = gd_ble_chunk_encode(frame, 8, 100, 20, packet, sizeof(packet));
    assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkRejected);
    assert(!receiver.active && !receiver.used);
    size = gd_ble_chunk_encode(frame, 7, 100, 20, packet, sizeof(packet));
    assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkRejected);
    /* Restart offset zero always replaces partial state, for same or new sequence. */
    begin_partial(&receiver, frame);
    size = gd_ble_chunk_encode(frame, 7, 0, 20, packet, sizeof(packet));
    assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkPartial);
    assert(receiver.used == 20 && receiver.sequence == 7);
    size = gd_ble_chunk_encode(frame, 8, 0, 20, packet, sizeof(packet));
    assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkPartial);
    assert(receiver.used == 20 && receiver.sequence == 8);
    /* A disconnect must discard partial state before a nonzero continuation. */
    gd_ble_reset(&receiver);
    size = gd_ble_chunk_encode(frame, 8, 20, 20, packet, sizeof(packet));
    assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkRejected);
    /* A final fragment cannot overflow even when its offset is otherwise correct. */
    for(size_t offset = 0; offset < GD_DETAIL_BYTES - 1;) {
        size_t count = GD_DETAIL_BYTES - 1 - offset;
        if(count > GD_BLE_CHUNK_PAYLOAD) count = GD_BLE_CHUNK_PAYLOAD;
        size = gd_ble_chunk_encode(frame, 7, (uint16_t)offset, count, packet, sizeof(packet));
        assert(gd_ble_chunk_receive(&receiver, packet, size) == GdBleChunkPartial);
        offset += count;
    }
    assert(receiver.used == GD_DETAIL_BYTES - 1);
    packet[0] = 'G'; packet[1] = 'D';
    gd_ble_write_u32(packet + 2, 7); gd_write_u16(packet + 6, GD_DETAIL_BYTES - 1);
    packet[8] = 0; packet[9] = 0;
    assert(gd_ble_chunk_receive(&receiver, packet, GD_BLE_CHUNK_HEADER + 2) == GdBleChunkRejected);
    assert(!receiver.active && !receiver.used);
}

static void acknowledgments(void) {
    uint8_t ack[GD_BLE_ACK_BYTES + 2];
    memset(ack, 0xa5, sizeof(ack));
    gd_ble_ack_encode(0x78563412, ack + 1);
    const uint8_t expected[] = {'G', 'A', 0x12, 0x34, 0x56, 0x78, 0, 0};
    assert(!memcmp(ack + 1, expected, sizeof(expected)));
    assert(ack[0] == 0xa5 && ack[sizeof(ack) - 1] == 0xa5);
    uint32_t sequence = 0;
    assert(gd_ble_ack_decode(ack + 1, GD_BLE_ACK_BYTES, &sequence));
    assert(sequence == 0x78563412);
    for(size_t size = 0; size <= GD_BLE_ACK_BYTES + 1; ++size) {
        if(size == GD_BLE_ACK_BYTES) continue;
        sequence = 0xaabbccdd;
        assert(!gd_ble_ack_decode(ack + 1, size, &sequence));
        assert(sequence == 0xaabbccdd);
    }
    const unsigned fields[] = {0, 1, 6, 7};
    for(size_t i = 0; i < sizeof(fields) / sizeof(fields[0]); ++i) {
        uint8_t bad[GD_BLE_ACK_BYTES];
        memcpy(bad, expected, sizeof(bad)); bad[fields[i]] ^= 1;
        sequence = 0xaabbccdd;
        assert(!gd_ble_ack_decode(bad, sizeof(bad), &sequence));
        assert(sequence == 0xaabbccdd);
    }
    assert(!gd_ble_ack_decode(NULL, GD_BLE_ACK_BYTES, &sequence));
    assert(!gd_ble_ack_decode(ack + 1, GD_BLE_ACK_BYTES, NULL));
}

static void displayed_status(void) {
    uint8_t frame[GD_DETAIL_BYTES], packet[GD_BLE_CHUNK_MAX];
    GdBleAssembler receiver;
    GdView view;
    memset(&view, 0, sizeof(view)); reset_receiver(&receiver);
    make_frame(7, frame);
    assert(gd_view_receive(&view, frame + 72, GD_STATUS_BYTES, 1000));
    GdView before = view;
    /* Chunk assembly itself never replaces a good display with an incomplete frame. */
    for(size_t offset = 0; offset < GD_DETAIL_BYTES;) {
        size_t count = GD_DETAIL_BYTES - offset;
        if(count > GD_BLE_CHUNK_PAYLOAD) count = GD_BLE_CHUNK_PAYLOAD;
        size_t size = gd_ble_chunk_encode(frame, 7, (uint16_t)offset, count, packet, sizeof(packet));
        GdBleChunkResult result = gd_ble_chunk_receive(&receiver, packet, size);
        assert(!memcmp(&view, &before, sizeof(view)));
        offset += count;
        if(result == GdBleChunkComplete) {
            assert(gd_view_receive(&view, receiver.frame + 72, GD_STATUS_BYTES, 15000));
            assert(view.received_ms == 1000 && gd_view_page_stale(&view, 15000));
        }
    }
    before = view;
    /* Complete transport framing does not make malformed inner status data acceptable. */
    frame[72 + 8] = 0xff;
    for(size_t offset = 0; offset < GD_DETAIL_BYTES;) {
        size_t count = GD_DETAIL_BYTES - offset;
        if(count > GD_BLE_CHUNK_PAYLOAD) count = GD_BLE_CHUNK_PAYLOAD;
        size_t size = gd_ble_chunk_encode(frame, 8, (uint16_t)offset, count, packet, sizeof(packet));
        GdBleChunkResult result = gd_ble_chunk_receive(&receiver, packet, size);
        offset += count;
        if(result == GdBleChunkComplete)
            assert(!gd_view_receive(&view, receiver.frame + 72, GD_STATUS_BYTES, 16000));
    }
    assert(!memcmp(&view, &before, sizeof(view)));
}

static void node_controls(void) {
    uint8_t guarded[GD_BLE_CONTROL_BYTES + 2];
    memset(guarded, 0xa5, sizeof(guarded));
    assert(GD_BLE_CONTROL_BYTES == 8);
    assert(gd_ble_control_encode(0x78563412, GD_BLE_NODE_NEXT, guarded + 1));
    const uint8_t expected[] = {'G', 'N', 0x12, 0x34, 0x56, 0x78, 1, 0};
    assert(!memcmp(guarded + 1, expected, sizeof(expected)));
    assert(guarded[0] == 0xa5 && guarded[sizeof(guarded) - 1] == 0xa5);
    uint32_t sequence = 0;
    uint8_t direction = 0;
    assert(gd_ble_control_decode(guarded + 1, GD_BLE_CONTROL_BYTES, &sequence, &direction));
    assert(sequence == 0x78563412 && direction == GD_BLE_NODE_NEXT);
    assert(gd_ble_control_encode(UINT32_MAX, GD_BLE_NODE_PREVIOUS, guarded + 1));
    assert(gd_ble_control_decode(guarded + 1, GD_BLE_CONTROL_BYTES, &sequence, &direction));
    assert(sequence == UINT32_MAX && direction == GD_BLE_NODE_PREVIOUS);
    for(unsigned bad = 0; bad < 256; ++bad) {
        if(bad == GD_BLE_NODE_NEXT || bad == GD_BLE_NODE_PREVIOUS) continue;
        memset(guarded, 0xa5, sizeof(guarded));
        assert(!gd_ble_control_encode(1, (uint8_t)bad, guarded + 1));
        for(size_t i = 0; i < sizeof(guarded); ++i) assert(guarded[i] == 0xa5);
        memcpy(guarded + 1, expected, sizeof(expected)); guarded[7] = (uint8_t)bad;
        sequence = 123; direction = 99;
        assert(!gd_ble_control_decode(guarded + 1, GD_BLE_CONTROL_BYTES, &sequence, &direction));
        assert(sequence == 123 && direction == 99);
    }
    memcpy(guarded + 1, expected, sizeof(expected));
    for(size_t size = 0; size <= GD_BLE_CONTROL_BYTES + 1; ++size) {
        if(size == GD_BLE_CONTROL_BYTES) continue;
        sequence = 123; direction = 99;
        assert(!gd_ble_control_decode(guarded + 1, size, &sequence, &direction));
        assert(sequence == 123 && direction == 99);
    }
    const unsigned corrupt[] = {0, 1, 7};
    for(size_t i = 0; i < sizeof(corrupt) / sizeof(corrupt[0]); ++i) {
        memcpy(guarded + 1, expected, sizeof(expected)); guarded[1 + corrupt[i]] ^= 0x80;
        sequence = 123; direction = 99;
        assert(!gd_ble_control_decode(guarded + 1, GD_BLE_CONTROL_BYTES, &sequence, &direction));
        assert(sequence == 123 && direction == 99);
    }
    memcpy(guarded + 1, expected, sizeof(expected));
    memset(guarded + 3, 0, 4);  /* Empty characteristic value has no usable sequence. */
    sequence = 123; direction = 99;
    assert(!gd_ble_control_decode(guarded + 1, GD_BLE_CONTROL_BYTES, &sequence, &direction));
    assert(sequence == 123 && direction == 99);
    memset(guarded, 0xa5, sizeof(guarded));
    assert(!gd_ble_control_encode(0, GD_BLE_NODE_NEXT, guarded + 1));
    for(size_t i = 0; i < sizeof(guarded); ++i) assert(guarded[i] == 0xa5);
    assert(!gd_ble_control_encode(1, GD_BLE_NODE_NEXT, NULL));
    assert(!gd_ble_control_decode(NULL, GD_BLE_CONTROL_BYTES, &sequence, &direction));
    assert(!gd_ble_control_decode(expected, GD_BLE_CONTROL_BYTES, NULL, &direction));
    assert(!gd_ble_control_decode(expected, GD_BLE_CONTROL_BYTES, &sequence, NULL));
}

int main(void) {
    encoding();
    full_frames();
    malformed_and_retry();
    acknowledgments();
    displayed_status();
    node_controls();
    puts("Flipper dedicated BLE chunk, reassembly, receipt, node-control and display-preservation checks passed.");
    return 0;
}
