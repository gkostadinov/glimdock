#pragma once
#include <stdint.h>

// Active-low Power button, sampled without blocking the display task. Ignore the
// power-on press until the button has first been released and debounced.
class DevicePowerButton {
public:
    static constexpr uint32_t DebounceMs = 40, HoldMs = 2000;
    bool update(bool pressed, uint32_t now) {
        if(!initialized_) {
            initialized_ = true; raw_ = pressed; changed_ = now;
            return false;
        }
        if(pressed != raw_) { raw_ = pressed; changed_ = now; }
        if(uint32_t(now - changed_) < DebounceMs) return false;
        if(!raw_) {
            armed_ = true; stable_ = false; reported_ = false;
            return false;
        }
        if(!stable_) { stable_ = true; pressedAt_ = now; }
        if(armed_ && !reported_ && uint32_t(now - pressedAt_) >= HoldMs) {
            reported_ = true;
            return true;
        }
        return false;
    }
    bool released() const { return armed_ && !raw_ && !stable_; }
private:
    bool initialized_ = false, raw_ = false, stable_ = false;
    bool armed_ = false, reported_ = false;
    uint32_t changed_ = 0, pressedAt_ = 0;
};
