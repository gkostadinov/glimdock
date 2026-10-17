#pragma once
#include <stddef.h>
#include <stdint.h>
#include "flipper_cache_protocol.h"

namespace flipper {
struct State {
    bool enabled = false;
    bool connected = false; // Authenticated link to the open native Glimdock app.
    char message[96] = {};
    char peer[40] = {};
    uint32_t passkey = 0;
    uint32_t revision = 0;
    bool pairing = false; // User PIN entry or numeric comparison is pending.
    bool passkeyEntry = false; // Stock Flipper displays a PIN for entry on Glimdock.
    uint32_t ackedFrames = 0; // Successful status exchanges in the current BLE session.
    uint32_t lastAckMs = 0; // Local millis() at last status acknowledgment; zero until first.
    uint32_t linkGeneration = 0; // Changes on disconnect/reset/new control subscription.
    char phase[20] = {}; // Local USB diagnostics; contains no pairing data.
    int32_t securityStartRc = -1, securityEventRc = -1;
    uint8_t securityFlags = 0, securityKeyBytes = 0; // encrypted/authenticated/bonded bits.
};
#ifdef HOMELAB_NATIVE_PREVIEW
inline State& previewState() { static State value; return value; }
inline void begin() {}
inline void setEnabled(bool) {}
inline void pair() {}
inline void forget() {}
inline bool readState(State& out) { out = previewState(); return true; }
inline bool confirmPairing(bool) { return false; }
inline bool submitPasskey(uint32_t) { return false; }
inline void publish(const uint8_t*, size_t, bool = false) {}
inline void publishCatalog(const GdNodeCatalog&) {}
inline bool takeNodeStep(int8_t&) { return false; }
inline bool takeNodeSelection(char*, size_t, uint32_t&) { return false; }
#else
void begin();
void setEnabled(bool enabled);
// Explicitly discover a Flipper for 20 seconds, or reconnect the remembered peer.
// New pairing requires submitPasskey() for stock firmware, or explicit
// confirmPairing(true) when a numeric-comparison peer displays the same code.
void pair();
void forget();
bool readState(State& out);
bool confirmPairing(bool accept);
bool submitPasskey(uint32_t passkey);
// Latest status replaces queued status. Urgent navigation bypasses normal
// pacing after the current in-flight frame; never blocks on BLE/UI/network I/O.
void publish(const uint8_t* wire, size_t len, bool urgent = false);
// Refreshes coalesce; a full catalog transaction always uses one snapshot.
void publishCatalog(const GdNodeCatalog& catalog);
// Consume one authenticated node button event on the UI thread (+1/-1).
bool takeNodeStep(int8_t& step);
bool takeNodeSelection(char* id, size_t capacity, uint32_t& requestSequence);
#endif
} // namespace flipper
