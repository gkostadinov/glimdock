// Native checks for the bounded, connection-scoped BLE node-control queue.
#include "flipper_control_queue.h"
#include <cassert>
#include <cstdint>
#include <cstring>
#include <iostream>

static void checkSelectionMailbox() {
    flipper::NodeSelectionMailbox mailbox;
    char id[GD_NODE_ID_BYTES]; std::memset(id, 'x', sizeof(id));
    uint32_t sequence = 123;
    const auto unchanged = [&] {
        assert(sequence == 123);
        for(char c : id) assert(c == 'x');
    };
    assert(!mailbox.take(id, sizeof(id), sequence)); unchanged();
    assert(!mailbox.push(0, 1, "nas"));
    const uint32_t first = mailbox.begin();
    assert(!mailbox.push(first, 0, "nas"));
    assert(!mailbox.push(first, 1, nullptr));
    assert(!mailbox.push(first, 1, ""));
    assert(!mailbox.push(first, 1, "bad node"));
    char tooLong[GD_NODE_ID_BYTES + 1];
    std::memset(tooLong, 'a', sizeof(tooLong)); tooLong[GD_NODE_ID_BYTES] = 0;
    assert(!mailbox.push(first, 1, tooLong));
    assert(mailbox.push(first, 1, "macbook")); // Invalid requests did not consume sequence 1.
    assert(!mailbox.push(first, 1, "nas"));
    assert(!mailbox.push(first, UINT32_MAX, "nas"));
    assert(!mailbox.push(first, 0x80000001u, "nas"));

    // A burst replaces pending work with the final absolute destination.
    for(uint32_t i = 2; i < 1000; ++i) assert(mailbox.push(first, i, "nas"));
    assert(mailbox.push(first, 1000, "vm:ubuntu-01"));
    assert(!mailbox.take(nullptr, sizeof(id), sequence));
    assert(!mailbox.take(id, sizeof(id) - 1, sequence)); unchanged();
    assert(mailbox.take(id, sizeof(id), sequence));
    assert(sequence == 1000 && !std::strcmp(id, "vm:ubuntu-01"));
    for(size_t i = std::strlen(id) + 1; i < sizeof(id); ++i) assert(!id[i]);
    assert(!mailbox.take(id, sizeof(id), sequence));

    // Cancellation discards pending selections and invalidates late callbacks.
    assert(mailbox.push(first, 1001, "nas"));
    mailbox.clear();
    assert(!mailbox.take(id, sizeof(id), sequence));
    assert(!mailbox.push(first, 1002, "nas"));
    const uint32_t second = mailbox.begin();
    assert(second != first);
    assert(!mailbox.push(first, 2000, "old-link"));
    assert(mailbox.push(second, 1, "new-link"));
    assert(mailbox.take(id, sizeof(id), sequence));
    assert(sequence == 1 && !std::strcmp(id, "new-link"));

    const uint32_t third = mailbox.begin();
    assert(mailbox.push(third, UINT32_MAX - 1, "nas"));
    assert(mailbox.push(third, UINT32_MAX, "nas"));
    assert(mailbox.push(third, 1, "after-wrap"));
    assert(!mailbox.push(third, UINT32_MAX, "stale"));
    assert(!mailbox.push(third, 0x80000001u, "ambiguous"));
    assert(mailbox.take(id, sizeof(id), sequence));
    assert(sequence == 1 && !std::strcmp(id, "after-wrap"));
}

int main() {
    checkSelectionMailbox();
    flipper::ControlQueue queue;
    int8_t step = 42;
    assert(!queue.take(step) && step == 42);
    assert(!queue.push(0, 1, 1));
    const uint32_t first = queue.begin();
    assert(first);
    assert(!queue.push(first, 0, 1)); // Zero denotes an empty characteristic value.
    assert(!queue.push(first, 1, 0));
    assert(!queue.push(first, 1, 2));
    assert(!queue.push(first, 1, -2));
    assert(queue.push(first, 1, -1)); // Invalid directions cannot consume the sequence.
    assert(!queue.push(first, 1, 1)); // Duplicate notification.
    assert(!queue.push(first, UINT32_MAX, 1)); // Older serial number.
    assert(!queue.push(first, 0x80000001u, 1)); // Ambiguous half-range.
    assert(queue.push(first, 2, 1));
    assert(queue.take(step) && step == -1);
    assert(queue.take(step) && step == 1);
    step = 42; assert(!queue.take(step) && step == 42);

    // Exactly eight steps fit; a dropped overflow event cannot be replayed later.
    const uint32_t second = queue.begin();
    assert(second && second != first);
    for(uint32_t i = 1; i <= 8; ++i) assert(queue.push(second, i, i % 2 ? -1 : 1));
    assert(!queue.push(second, 9, 1));
    assert(queue.take(step) && step == -1);
    assert(!queue.push(second, 9, 1));
    assert(queue.push(second, 10, -1));
    for(uint32_t i = 2; i <= 8; ++i) assert(queue.take(step) && step == (i % 2 ? -1 : 1));
    assert(queue.take(step) && step == -1);
    assert(!queue.take(step));

    // A subscription's captured generation never authorizes a later connection.
    assert(queue.push(second, 11, 1));
    queue.clear();
    assert(!queue.take(step));
    assert(!queue.push(second, 12, 1));
    const uint32_t third = queue.begin();
    assert(third != first && third != second);
    assert(!queue.push(first, 200, 1));
    assert(!queue.push(second, 200, 1));
    assert(queue.push(third, 1, -1));
    assert(queue.take(step) && step == -1);

    // Peripheral sequence wrap skips the reserved zero value and still advances.
    const uint32_t fourth = queue.begin();
    assert(queue.push(fourth, UINT32_MAX - 1, 1));
    assert(queue.push(fourth, UINT32_MAX, -1));
    assert(queue.push(fourth, 1, 1));
    assert(!queue.push(fourth, UINT32_MAX, -1));
    assert(!queue.push(fourth, 0x80000001u, -1));
    assert(queue.take(step) && step == 1);
    assert(queue.take(step) && step == -1);
    assert(queue.take(step) && step == 1);
    assert(!queue.take(step));
    std::cout << "Flipper control FIFO and latest absolute selection: bounds, replay, generations and wrap checks passed.\n";
}
