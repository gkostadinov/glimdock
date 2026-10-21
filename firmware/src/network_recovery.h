#pragma once
#include <stdint.h>
#include <stddef.h>

namespace glimdock {
enum class BodyRead { Complete, Timeout, Incomplete, TooLarge };

// HTTPClient 2.x does not apply its timeout while waiting for body bytes.
// Read an HTTP/1.0 identity body with both an idle and an absolute deadline.
template<class Client, class Sink, class Clock, class Pause>
BodyRead readBoundedBody(Client& client, Sink& sink, int length, size_t limit,
                         uint32_t totalMs, uint32_t idleMs, Clock now, Pause pause) {
    if(length >= 0 && size_t(length) > limit) return BodyRead::TooLarge;
    const uint32_t began = now(); uint32_t lastData = began; size_t used = 0;
    uint8_t chunk[256];
    for(;;) {
        if(length >= 0 && used == size_t(length)) return BodyRead::Complete;
        const uint32_t tick = now();
        if(uint32_t(tick - began) >= totalMs || uint32_t(tick - lastData) >= idleMs)
            return BodyRead::Timeout;
        const int available = client.available();
        if(available <= 0) {
            if(!client.connected()) return length < 0 ? BodyRead::Complete : BodyRead::Incomplete;
            pause(); continue;
        }
        if(used == limit) return BodyRead::TooLarge;
        size_t count = size_t(available);
        if(count > sizeof(chunk)) count = sizeof(chunk);
        if(count > limit - used) count = limit - used;
        if(length >= 0 && count > size_t(length) - used) count = size_t(length) - used;
        const int got = client.read(chunk, count);
        if(got > 0) {
            if(size_t(got) > count || sink.write(chunk, size_t(got)) != size_t(got))
                return BodyRead::TooLarge;
            used += size_t(got); lastData = now();
        }
        pause();
    }
}

class NetworkRecovery {
    bool observed_ = false, connected_ = false, failed_ = false;
    uint32_t since_ = 0, delay_ = 30000, failedSince_ = 0, failures_ = 0;
public:
    void reset(uint32_t now, bool connected) {
        observed_ = true; connected_ = connected; since_ = now; delay_ = 30000;
        failed_ = false; failures_ = 0;
    }
    void healthy() { failed_ = false; failures_ = 0; }
    void failed(uint32_t now) {
        if(!failed_) { failed_ = true; failedSince_ = now; }
        if(failures_ != UINT32_MAX) ++failures_;
    }
    uint32_t failures() const { return failures_; }
    bool reconnect(uint32_t now, bool connected) {
        if(!observed_ || connected != connected_) { reset(now, connected); return false; }
        if(connected) {
            // A server error/auth rejection must never be counted as a lost radio.
            if(!failed_ || failures_ < 3 || uint32_t(now - failedSince_) < 90000 ||
               uint32_t(now - since_) < 120000) return false;
            since_ = failedSince_ = now; failures_ = 0; return true;
        }
        if(uint32_t(now - since_) < delay_) return false;
        since_ = now; delay_ = delay_ < 60000 ? delay_ * 2 : 120000;
        return true;
    }
};
} // namespace glimdock
