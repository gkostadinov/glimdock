// c++ -std=c++17 -Wall -Wextra -Werror -Itools/flipper-usb-test-stubs firmware/src/flipper_usb.cpp tools/flipper-test-usb.cpp -o /tmp/flipper-test-usb && /tmp/flipper-test-usb
#include <Arduino.h>
#include "../firmware/src/flipper_ble.h"
#include "../firmware/src/flipper_usb.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

namespace flipper {
State testState;
unsigned pairs = 0, toggles = 0, submissions = 0, cancellations = 0;
uint32_t lastPin = 0;
void setEnabled(bool enabled) { testState.enabled = enabled; ++toggles; }
void pair() { ++pairs; }
bool submitPasskey(uint32_t pin) { lastPin = pin; ++submissions; return testState.passkeyEntry; }
bool confirmPairing(bool accept) { assert(!accept); ++cancellations; return testState.pairing; }
bool readState(State& out) { out = testState; return true; }
}
static std::string command(const char* text, size_t len = 0) {
    char input[256];
    if(!len) len = strlen(text);
    assert(len <= sizeof(input)); memcpy(input, text, len);
    Stream serial;
    assert(flipper::handleUsbLine(input, len, serial));
    for(size_t i = 0; i < len; ++i) assert(input[i] == 0);
    assert(serial.writes == 1 && serial.output.back() == '\n');
    return serial.output;
}
int main() {
    using namespace flipper;
    char privateInput[] = "GLIMDOCK_PRIVATE_READER secret-content";
    Stream untouched;
    assert(!handleUsbLine(privateInput, strlen(privateInput), untouched));
    assert(strcmp(privateInput, "GLIMDOCK_PRIVATE_READER secret-content") == 0 && untouched.output.empty());
    testState.passkey = 987654; testState.pairing = testState.passkeyEntry = true;
    testState.ackedFrames = 17; testState.lastAckMs = 1234000;
    strcpy(testState.phase, "securing"); testState.securityStartRc = 2;
    testState.securityEventRc = -1; testState.securityFlags = 7; testState.securityKeyBytes = 16;
    strcpy(testState.peer, "Name\"\\\n"); strcpy(testState.message, "Ready");
    auto status = command("GLIMDOCK_FLIPPER_STATUS");
    assert(status.find("987654") == std::string::npos);
    assert(status.find("\"passkey\":") == std::string::npos);
    assert(status.find("\"passkey_entry\":true") != std::string::npos);
    assert(status.find("Name\\\"\\\\?") != std::string::npos);
    assert(status.find("\"free_heap\":12345") != std::string::npos);
    assert(status.find("\"acked_frames\":17") != std::string::npos);
    assert(status.find("\"last_ack_ms\":1234000") != std::string::npos);
    assert(status.find("\"phase\":\"securing\"") != std::string::npos);
    assert(status.find("\"security_start_rc\":2") != std::string::npos);
    assert(status.find("\"security_event_rc\":-1") != std::string::npos);
    assert(status.find("\"security_flags\":7") != std::string::npos);
    assert(status.find("\"security_key_bytes\":16") != std::string::npos);
    assert(command("GLIMDOCK_FLIPPER_ENABLE 1").find("\"ok\":true") != std::string::npos && testState.enabled);
    command("GLIMDOCK_FLIPPER_ENABLE 0"); assert(!testState.enabled && toggles == 2);
    command("GLIMDOCK_FLIPPER_PAIR"); assert(pairs == 1);
    auto pin = command("GLIMDOCK_FLIPPER_PIN 012345\r");
    assert(lastPin == 12345 && submissions == 1 && pin.find("012345") == std::string::npos);
    assert(pin.find("\"ok\":true") != std::string::npos);
    command("GLIMDOCK_FLIPPER_PIN 000000"); assert(lastPin == 0 && submissions == 2);
    testState.passkeyEntry = false;
    assert(command("GLIMDOCK_FLIPPER_PIN 123456").find("\"ok\":false") != std::string::npos);
    command("GLIMDOCK_FLIPPER_CANCEL"); assert(cancellations == 1);
    const char* invalid[] = {"GLIMDOCK_FLIPPER_PIN 12345", "GLIMDOCK_FLIPPER_PIN 1234567",
        "GLIMDOCK_FLIPPER_PIN 1a3456", "GLIMDOCK_FLIPPER_ENABLE true", "GLIMDOCK_FLIPPER_FORGET",
        "GLIMDOCK_FLIPPER_PAIR suffix", "GLIMDOCK_FLIPPER_"};
    for(const auto* text : invalid) assert(command(text).find("\"ok\":false") != std::string::npos);
    assert(submissions == 3 && pairs == 1 && toggles == 2);
    const char embedded[] = "GLIMDOCK_FLIPPER_PIN 123456\0ignored";
    assert(command(embedded, sizeof(embedded) - 1).find("\"ok\":false") != std::string::npos);
    std::string oversized = "GLIMDOCK_FLIPPER_PIN " + std::string(200, '9');
    assert(command(oversized.c_str()).find("\"ok\":false") != std::string::npos);
    puts("Flipper USB status, strict commands, secret-free replies and input wiping checks passed.");
}
