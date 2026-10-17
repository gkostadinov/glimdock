#pragma once
#include <stdint.h>
#include <stddef.h>
#include "flipper_cache_protocol.h"

namespace flipper {
// Portable bounded handoff from BLE callbacks to the display thread. Caller
// serializes access; generation tokens belong to individual subscriptions.
class ControlQueue {
    int8_t steps_[8] = {};
    uint8_t head_ = 0, count_ = 0;
    uint32_t generation_ = 0, lastSequence_ = 0;
    bool active_ = false, seen_ = false;
public:
    void clear() {
        ++generation_;
        head_ = count_ = 0;
        active_ = seen_ = false;
        lastSequence_ = 0;
    }
    uint32_t begin() { clear(); active_ = true; return generation_; }
    uint32_t generation() const { return generation_; }
    bool push(uint32_t generation, uint32_t sequence, int8_t step) {
        if(!active_ || generation != generation_ || !sequence || (step != 1 && step != -1)) return false;
        const uint32_t delta = sequence - lastSequence_;
        if(seen_ && (!delta || delta >= 0x80000000u)) return false;
        seen_ = true; lastSequence_ = sequence;
        // Once observed, a dropped full-queue event cannot replay later.
        if(count_ == sizeof(steps_)) return false;
        steps_[(head_ + count_) % sizeof(steps_)] = step;
        ++count_;
        return true;
    }
    bool take(int8_t& step) {
        if(!active_ || !count_) return false;
        step = steps_[head_];
        head_ = (head_ + 1) % sizeof(steps_);
        --count_;
        return true;
    }
};

// Absolute selections coalesce: only the most recent requested node needs
// work on the display/network thread. A subscription owns one generation.
class NodeSelectionMailbox {
    char id_[GD_NODE_ID_BYTES] = {};
    uint32_t generation_ = 0, lastSequence_ = 0;
    bool active_ = false, seen_ = false, pending_ = false;
public:
    void clear() {
        ++generation_; active_ = seen_ = pending_ = false;
        lastSequence_ = 0; memset(id_, 0, sizeof(id_));
    }
    uint32_t begin() { clear(); active_ = true; return generation_; }
    uint32_t generation() const { return generation_; }
    bool push(uint32_t generation, uint32_t sequence, const char* id) {
        if(!active_ || generation != generation_ || !sequence) return false;
        const uint32_t delta = sequence - lastSequence_;
        if(seen_ && (!delta || delta >= 0x80000000u)) return false;
        char checked[GD_NODE_ID_BYTES];
        if(!gd_cache_copy_id(checked, id, false)) return false;
        memcpy(id_, checked, sizeof(id_));
        seen_ = pending_ = true; lastSequence_ = sequence;
        return true;
    }
    bool take(char* id, size_t capacity, uint32_t& sequence) {
        if(!active_ || !pending_ || !id || capacity < sizeof(id_)) return false;
        memcpy(id, id_, sizeof(id_)); sequence = lastSequence_; pending_ = false;
        return true;
    }
};
} // namespace flipper
