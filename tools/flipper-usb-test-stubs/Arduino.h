#pragma once
#include <stddef.h>
#include <stdint.h>
#include <string>
// Only for host-side USB command tests; never linked into device firmware.
class Stream {
public:
    std::string output;
    unsigned writes = 0;
    size_t write(const uint8_t* bytes, size_t len) {
        output.append((const char*)bytes, len); ++writes; return len;
    }
};
struct TestEsp {
    uint32_t getFreeHeap() const { return 12345; }
    uint32_t getMinFreeHeap() const { return 10000; }
};
inline TestEsp ESP;
inline unsigned long millis() { return 1234567; }
