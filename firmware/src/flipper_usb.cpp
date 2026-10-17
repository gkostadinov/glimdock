#include "flipper_usb.h"
#ifndef HOMELAB_NATIVE_PREVIEW
#include "flipper_ble.h"
#include <Arduino.h>
#include <stdio.h>
#include <string.h>

namespace flipper {
namespace {
constexpr char Prefix[] = "GLIMDOCK_FLIPPER_";
void wipe(char* line, size_t len) {
    volatile char* p = line;
    while(len--) *p++ = 0;
}
void jsonText(char* out, size_t cap, const char* text, size_t sourceCap) {
    size_t used = 0;
    for(size_t i = 0; i < sourceCap && text[i] && used + 2 < cap; ++i) {
        const unsigned char c = (unsigned char)text[i];
        if(c == '"' || c == '\\') out[used++] = '\\';
        out[used++] = c >= 32 && c <= 126 ? (char)c : '?';
    }
    out[used] = 0;
}
void result(Stream& serial, const char* action, bool ok) {
    char output[128];
    const int n = snprintf(output, sizeof(output),
        "GLIMDOCK_FLIPPER_RESULT {\"action\":\"%s\",\"ok\":%s}\n", action, ok ? "true" : "false");
    if(n > 0 && (size_t)n < sizeof(output)) serial.write((const uint8_t*)output, (size_t)n);
}
void status(Stream& serial) {
    State state;
    bool ready = readState(state);
    char name[sizeof(state.peer) * 2 + 1], message[sizeof(state.message) * 2 + 1];
    char phase[sizeof(state.phase) * 2 + 1], output[896];
    jsonText(name, sizeof(name), state.peer, sizeof(state.peer));
    jsonText(message, sizeof(message), state.message, sizeof(state.message));
    jsonText(phase, sizeof(phase), state.phase, sizeof(state.phase));
    const int n = snprintf(output, sizeof(output),
        "GLIMDOCK_FLIPPER_STATUS {\"schema_version\":1,\"ready\":%s,\"enabled\":%s,"
        "\"connected\":%s,\"pairing\":%s,\"passkey_entry\":%s,\"peer\":\"%s\","
        "\"message\":\"%s\",\"revision\":%lu,\"acked_frames\":%lu,\"last_ack_ms\":%lu,"
        "\"uptime_ms\":%lu,\"free_heap\":%lu,\"min_free_heap\":%lu,"
        "\"phase\":\"%s\",\"security_start_rc\":%ld,\"security_event_rc\":%ld,"
        "\"security_flags\":%u,\"security_key_bytes\":%u}\n",
        ready ? "true" : "false", state.enabled ? "true" : "false", state.connected ? "true" : "false",
        state.pairing ? "true" : "false", state.passkeyEntry ? "true" : "false", name, message,
        (unsigned long)state.revision, (unsigned long)state.ackedFrames, (unsigned long)state.lastAckMs,
        (unsigned long)millis(), (unsigned long)ESP.getFreeHeap(),
        (unsigned long)ESP.getMinFreeHeap(), phase, (long)state.securityStartRc,
        (long)state.securityEventRc, (unsigned)state.securityFlags, (unsigned)state.securityKeyBytes);
    // Pairing codes and all Wi-Fi/private reader data are deliberately absent.
    if(n > 0 && (size_t)n < sizeof(output)) serial.write((const uint8_t*)output, (size_t)n);
}
} // namespace

bool handleUsbLine(char* line, size_t len, Stream& serial) {
    if(!line || len < sizeof(Prefix) - 1 || memcmp(line, Prefix, sizeof(Prefix) - 1)) return false;
    const size_t inputLen = len;
    if(len && line[len - 1] == '\r') --len;
    // Bound all command parsing and ensure embedded NULs cannot hide suffixes.
    if(len > 64 || memchr(line, 0, len)) {
        wipe(line, inputLen); result(serial, "invalid", false); return true;
    }
    const char* command = line + sizeof(Prefix) - 1;
    const size_t commandLen = len - (sizeof(Prefix) - 1);
    enum class Action { Invalid, Status, Enable, Disable, Pair, Pin, Cancel } action = Action::Invalid;
    volatile uint32_t pin = 0;
    if(commandLen == 6 && !memcmp(command, "STATUS", 6)) action = Action::Status;
    else if(commandLen == 8 && !memcmp(command, "ENABLE 1", 8)) action = Action::Enable;
    else if(commandLen == 8 && !memcmp(command, "ENABLE 0", 8)) action = Action::Disable;
    else if(commandLen == 4 && !memcmp(command, "PAIR", 4)) action = Action::Pair;
    else if(commandLen == 6 && !memcmp(command, "CANCEL", 6)) action = Action::Cancel;
    else if(commandLen == 10 && !memcmp(command, "PIN ", 4)) {
        bool valid = true;
        for(size_t i = 4; i < 10; ++i) {
            if(command[i] < '0' || command[i] > '9') { valid = false; break; }
            pin = pin * 10 + (uint32_t)(command[i] - '0');
        }
        if(valid) action = Action::Pin;
    }
    wipe(line, inputLen);
    switch(action) {
    case Action::Status: status(serial); break;
    case Action::Enable: setEnabled(true); result(serial, "enable", true); break;
    case Action::Disable: setEnabled(false); result(serial, "disable", true); break;
    case Action::Pair: pair(); result(serial, "pair", true); break;
    case Action::Pin: result(serial, "pin", submitPasskey(pin)); break;
    case Action::Cancel: result(serial, "cancel", confirmPairing(false)); break;
    default: result(serial, "invalid", false); break;
    }
    pin = 0;
    return true;
}
} // namespace flipper
#endif
