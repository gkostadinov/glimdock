#include "flipper_ble.h"
#ifndef HOMELAB_NATIVE_PREVIEW
#include "flipper_ble_protocol.h"
#include "flipper_control_queue.h"
#include <Arduino.h>
#include <NimBLEDevice.h>
#include <Preferences.h>
#include <WiFi.h>
#include <atomic>
#include <ctype.h>
#include <stdio.h>
#include <esp_wifi.h>
#include <freertos/FreeRTOS.h>
#include <freertos/queue.h>
#include <freertos/task.h>

namespace flipper {
namespace {
// Dedicated application GATT contract shared with the native Flipper app.
// Only that app advertises this service; the stock RPC service is never used.
constexpr uint32_t ScanMs = 20000, PairMs = 25000, CommandMs = 8000;
constexpr char TransportVersion[] = "native-v1";
portMUX_TYPE mutex = portMUX_INITIALIZER_UNLOCKED;
State state;
NodeSelectionMailbox nodeSelections;
bool begun = false, requestedEnabled = false, enableDirty = false;
bool pairRequested = false, forgetRequested = false;
int pairDecision = 0;
uint32_t enteredPasskey = 0;
uint8_t latest[GD_DETAIL_BYTES] = {};
uint32_t latestSerial = 0;
uint32_t latestUrgentSerial = 0;
GdNodeCatalog latestCatalog{}, activeCatalog{};
char committedNodeIds[GD_CATALOG_MAX_NODES][GD_NODE_ID_BYTES] = {};
uint32_t latestCatalogPublishedAt = 0;
bool haveCatalog = false;
struct Candidate { char address[18] = {}; char name[40] = {}; uint8_t type = 0; } candidate;
struct Ack { uint8_t bytes[GD_BLE_ACK_BYTES]; };
QueueHandle_t received = nullptr;
TaskHandle_t workerHandle = nullptr;
std::atomic<bool> scanning{false}, scanRememberedOnly{false}, candidateReady{false}, explicitPair{false};
std::atomic<bool> disconnected{true}, receiveFailed{false}, secure{false}, confirmed{false};
NimBLEClient* client = nullptr;
NimBLERemoteCharacteristic* rx = nullptr;
Preferences prefs;
char rememberedAddress[18] = {}, rememberedName[40] = {};
uint8_t rememberedType = 0;
bool gapListenerInstalled = false;

void safeText(char* out, size_t cap, const char* text) {
    size_t n = 0;
    if(text) while(n + 1 < cap && text[n]) {
        const unsigned char c = (unsigned char)text[n];
        out[n] = (c >= 32 && c <= 126) ? (char)c : '?'; ++n;
    }
    out[n] = 0;
}
void message(const char* text, bool connected = false) {
    portENTER_CRITICAL(&mutex);
    if(strcmp(state.message, text) || state.connected != connected) {
        safeText(state.message, sizeof(state.message), text);
        state.connected = connected; ++state.revision;
    }
    portEXIT_CRITICAL(&mutex);
}
void peer(const char* text) {
    portENTER_CRITICAL(&mutex);
    safeText(state.peer, sizeof(state.peer), text); ++state.revision;
    portEXIT_CRITICAL(&mutex);
}
bool enabled() {
    portENTER_CRITICAL(&mutex); bool value = requestedEnabled;
    portEXIT_CRITICAL(&mutex); return value;
}
bool cancelled() {
    portENTER_CRITICAL(&mutex);
    bool result = !requestedEnabled || forgetRequested || pairRequested;
    portEXIT_CRITICAL(&mutex); return result;
}
bool controlsPending() {
    portENTER_CRITICAL(&mutex);
    bool value = enableDirty || pairRequested || forgetRequested;
    portEXIT_CRITICAL(&mutex); return value;
}
void clearNodeControls() {
    portENTER_CRITICAL(&mutex);
    nodeSelections.clear(); state.linkGeneration = nodeSelections.generation(); ++state.revision;
    portEXIT_CRITICAL(&mutex);
}
void phase(const char* value) {
    portENTER_CRITICAL(&mutex);
    if(strcmp(state.phase, value)) {
        safeText(state.phase, sizeof(state.phase), value); ++state.revision;
    }
    portEXIT_CRITICAL(&mutex);
}
bool captureSecurity(const ble_gap_conn_desc& desc) {
    const uint8_t flags = (desc.sec_state.encrypted ? 1 : 0) |
                          (desc.sec_state.authenticated ? 2 : 0) |
                          (desc.sec_state.bonded ? 4 : 0);
    const uint8_t keyBytes = desc.sec_state.key_size;
    portENTER_CRITICAL(&mutex);
    if(state.securityFlags != flags || state.securityKeyBytes != keyBytes) {
        state.securityFlags = flags; state.securityKeyBytes = keyBytes; ++state.revision;
    }
    portEXIT_CRITICAL(&mutex);
    return flags == 7 && keyBytes == 16;
}
bool refreshSecurity() {
    ble_gap_conn_desc desc{};
    if(!client || !client->isConnected() || ble_gap_conn_find(client->getConnId(), &desc)) return false;
    const bool ok = captureSecurity(desc);
    if(ok) secure = true;
    return ok;
}
int gapEvents(ble_gap_event* event, void*) {
    if(event->type != BLE_GAP_EVENT_ENC_CHANGE || !client ||
       event->enc_change.conn_handle != client->getConnId()) return 0;
    portENTER_CRITICAL(&mutex);
    state.securityEventRc = event->enc_change.status; ++state.revision;
    portEXIT_CRITICAL(&mutex);
    ble_gap_conn_desc desc{};
    if(ble_gap_conn_find(event->enc_change.conn_handle, &desc) == 0) captureSecurity(desc);
    if(event->enc_change.status != 0) receiveFailed = true;
    return 0;
}

class ClientCallbacks final : public NimBLEClientCallbacks {
    void onDisconnect(NimBLEClient*) override {
        disconnected = true; secure = false;
        clearNodeControls();
    }
    uint32_t onPassKeyRequest() override {
        if(!explicitPair.load() || !enabled()) { receiveFailed = true; return 0; }
        portENTER_CRITICAL(&mutex);
        pairDecision = 0; state.pairing = true; state.passkeyEntry = true; state.passkey = 0;
        safeText(state.message, sizeof(state.message), "Enter the six-digit code displayed on Flipper.");
        ++state.revision;
        portEXIT_CRITICAL(&mutex);
        int decision = 0; uint32_t pin = 0;
        const uint32_t started = millis();
        while(millis() - started < PairMs) {
            portENTER_CRITICAL(&mutex);
            decision = pairDecision; pin = enteredPasskey;
            const bool abort = !requestedEnabled || forgetRequested || pairRequested;
            portEXIT_CRITICAL(&mutex);
            if(decision || abort) break;
            vTaskDelay(pdMS_TO_TICKS(20));
        }
        const bool accepted = decision == 2 && !cancelled();
        confirmed = accepted;
        if(!accepted) receiveFailed = true;
        portENTER_CRITICAL(&mutex);
        enteredPasskey = 0;
        state.pairing = false; state.passkeyEntry = false; state.passkey = 0; ++state.revision;
        portEXIT_CRITICAL(&mutex);
        message(accepted ? "Authenticating Flipper..." : "Pairing cancelled. Tap Pair to try again.");
        return accepted ? pin : 0;
    }
    bool onConfirmPIN(uint32_t pin) override {
        if(!explicitPair.load() || !enabled()) return false;
        portENTER_CRITICAL(&mutex);
        pairDecision = 0; state.pairing = true; state.passkeyEntry = false; state.passkey = pin;
        safeText(state.message, sizeof(state.message), "Compare this code with Flipper. Confirm on both devices.");
        ++state.revision;
        portEXIT_CRITICAL(&mutex);
        // NimBLE 1.x requires a synchronous bool callback. Only its BLE host task
        // waits here; the display and transport worker continue independently.
        int decision = 0;
        const uint32_t started = millis();
        while(millis() - started < PairMs) {
            portENTER_CRITICAL(&mutex); decision = pairDecision;
            const bool abort = !requestedEnabled || forgetRequested || pairRequested;
            portEXIT_CRITICAL(&mutex);
            if(decision || abort) break;
            vTaskDelay(pdMS_TO_TICKS(20));
        }
        const bool accepted = decision == 1 && !cancelled();
        confirmed = accepted;
        portENTER_CRITICAL(&mutex);
        state.pairing = false; state.passkey = 0; ++state.revision;
        portEXIT_CRITICAL(&mutex);
        message(accepted ? "Pairing approved; confirm on Flipper." : "Pairing cancelled. Tap Pair to try again.");
        return accepted;
    }
    void onAuthenticationComplete(ble_gap_conn_desc* desc) override {
        const bool ok = captureSecurity(*desc);
        secure = ok;
        if(!ok) receiveFailed = true;
    }
} callbacks;

class AdvertisedCallbacks final : public NimBLEAdvertisedDeviceCallbacks {
    void onResult(NimBLEAdvertisedDevice* device) override {
        if(!scanning.load() || candidateReady.load() || !device->haveName() ||
           device->getName() != GD_BLE_NAME) return;
        if(!device->isAdvertisingService(NimBLEUUID(GD_BLE_SERVICE_UUID)) &&
           !device->isAdvertisingService(NimBLEUUID((uint16_t)GD_BLE_ADVERTISED_SERVICE))) return;
        Candidate found;
        safeText(found.address, sizeof(found.address), device->getAddress().toString().c_str());
        if(scanRememberedOnly.load() && strcmp(found.address, rememberedAddress)) return;
        safeText(found.name, sizeof(found.name), device->getName().c_str());
        found.type = device->getAddress().getType();
        portENTER_CRITICAL(&mutex); candidate = found; portEXIT_CRITICAL(&mutex);
        candidateReady = true;
        NimBLEDevice::getScan()->stop();
    }
} advertisements;

void notifyAck(NimBLERemoteCharacteristic*, uint8_t* bytes, size_t len, bool) {
    uint32_t sequence;
    if(!received || !gd_ble_ack_decode(bytes, len, &sequence)) { receiveFailed = true; return; }
    Ack ack; memcpy(ack.bytes, bytes, sizeof(ack.bytes));
    if(xQueueSend(received, &ack, 0) != pdTRUE) receiveFailed = true;
}
void resetLink() {
    clearNodeControls();
    scanning = false;
    if(NimBLEDevice::getInitialized() && NimBLEDevice::getScan()->isScanning())
        NimBLEDevice::getScan()->stop();
    if(client && client->isConnected()) {
        client->disconnect();
        const uint32_t start = millis();
        while(client->isConnected() && millis() - start < 1500) vTaskDelay(pdMS_TO_TICKS(10));
    }
    rx = nullptr; secure = false; disconnected = true;
    if(received) xQueueReset(received);
    portENTER_CRITICAL(&mutex);
    enteredPasskey = 0; pairDecision = 0;
    state.connected = false; state.pairing = false; state.passkeyEntry = false; state.passkey = 0; ++state.revision;
    portEXIT_CRITICAL(&mutex);
}
bool initBle() {
    if(NimBLEDevice::getInitialized() && client) return true;
    // All BLE allocation/initialization happens only after explicit opt-in.
    message("Starting Bluetooth...");
    // ESP32-S3's coexistence library aborts during controller enable if Wi-Fi
    // modem sleep is disabled. Keep the network's modem-sleep policy in force
    // before NimBLE enters its nonrecoverable ESP_ERROR_CHECK init path.
    if(WiFi.getSleep() == WIFI_PS_NONE && !WiFi.setSleep(WIFI_PS_MIN_MODEM)) {
        message("Could not enable Wi-Fi/Bluetooth coexistence. Retry Pair.");
        return false;
    }
    // WiFi.getSleep() reports Arduino's cached policy; a failed setSleep call
    // can update that cache without changing the driver. Verify the live mode.
    wifi_ps_type_t actualSleep = WIFI_PS_NONE;
    esp_err_t sleepResult = esp_wifi_get_ps(&actualSleep);
    if(sleepResult == ESP_OK && actualSleep == WIFI_PS_NONE) {
        sleepResult = esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
        if(sleepResult == ESP_OK) sleepResult = esp_wifi_get_ps(&actualSleep);
        if(sleepResult != ESP_OK || actualSleep == WIFI_PS_NONE) {
            message("Wi-Fi modem sleep unavailable. Bluetooth start cancelled.");
            return false;
        }
    } else if(sleepResult != ESP_OK && sleepResult != ESP_ERR_WIFI_NOT_INIT) {
        message("Could not verify Wi-Fi/Bluetooth coexistence. Retry Pair.");
        return false;
    }
    if(!NimBLEDevice::getInitialized()) NimBLEDevice::init("Glimdock");
    if(!NimBLEDevice::getInitialized()) { message("Bluetooth initialization failed."); return false; }
    if(!gapListenerInstalled) {
        NimBLEDevice::setCustomGapHandler(gapEvents);
        gapListenerInstalled = true;
    }
    NimBLEDevice::setSecurityAuth(true, true, true);
    // Stock Flipper is DisplayOnly, so authenticated pairing requires PIN
    // entry here. KeyboardDisplay also supports numeric comparison peers.
    NimBLEDevice::setSecurityIOCap(BLE_HS_IO_KEYBOARD_DISPLAY);
    NimBLEDevice::setMTU(247);
    auto* scan = NimBLEDevice::getScan();
    scan->setActiveScan(true); scan->setInterval(100); scan->setWindow(40);
    scan->setMaxResults(0); // Callback only: do not retain an unbounded device list.
    scan->setAdvertisedDeviceCallbacks(&advertisements);
    client = NimBLEDevice::createClient();
    if(!client) { message("Not enough memory for Bluetooth."); return false; }
    client->setClientCallbacks(&callbacks, false);
    client->setConnectTimeout(8);
    // 30-60 ms connection interval leaves airtime for the existing Wi-Fi link.
    client->setConnectionParams(24, 48, 0, 400);
    return true;
}
bool discover(Candidate& found, bool rememberedOnly, uint32_t durationMs) {
    phase("scanning");
    message(rememberedOnly ? "Waiting for Glimdock app on your Flipper..." :
                            "Looking for Glimdock (20s). Open it on Flipper.");
    candidateReady = false; scanRememberedOnly = rememberedOnly; scanning = true;
    auto* scan = NimBLEDevice::getScan(); scan->clearResults();
    if(!scan->start(durationMs / 1000, [](NimBLEScanResults) {}, false)) {
        scanning = false; message("Could not start Bluetooth scan."); return false;
    }
    const uint32_t started = millis();
    while(millis() - started < durationMs && !cancelled() && !candidateReady.load())
        vTaskDelay(pdMS_TO_TICKS(50));
    scanning = false; scan->stop();
    if(!candidateReady.load() || cancelled()) {
        message(!enabled() ? "Bluetooth off." : rememberedOnly ?
            "Open Glimdock on Flipper. Waiting to reconnect." :
            "Glimdock not found. Open the app on Flipper, then Pair.");
        return false;
    }
    portENTER_CRITICAL(&mutex); found = candidate; portEXIT_CRITICAL(&mutex);
    return true;
}
bool connectPeer(const Candidate& found, bool allowPair) {
    portENTER_CRITICAL(&mutex);
    state.ackedFrames = 0; state.lastAckMs = 0; ++state.revision;
    state.securityStartRc = state.securityEventRc = -1;
    state.securityFlags = state.securityKeyBytes = 0;
    portEXIT_CRITICAL(&mutex);
    phase("connecting");
    peer(found.name); message("Connecting to Flipper...");
    disconnected = false; receiveFailed = false; secure = false; confirmed = false;
    explicitPair = allowPair;
    NimBLEAddress address(std::string(found.address), found.type);
    // connect() also waits for MTU exchange. NimBLE 1.x's synchronous PIN
    // callback can delay that exchange while the person enters the code.
    client->setConnectTimeout(allowPair ? 35 : 8);
    if(!client->connect(address, true) || cancelled()) {
        message("Glimdock unavailable. Open it on Flipper to reconnect."); return false;
    }
    phase("securing");
    message("Securing Bluetooth connection...");
    // The dedicated Flipper profile's GAP layer always sends a slave security
    // request on connect (gap.c: HCI_LE_CONNECTION_COMPLETE_SUBEVT_CODE).
    // Let that single initiator drive pairing/restore. NimBLE 1.4.3's duplicate
    // ble_sm_enc_initiate(EALREADY) path can remove the existing restore proc,
    // leaving encrypted-only state with no authenticated/bonded metadata.
    // A second central request is therefore both unnecessary and destructive.
    // Keep securityStartRc=-1 to show no local request was issued; never accept
    // a connection until the authoritative descriptor satisfies all checks.
    refreshSecurity();
    const uint32_t started = millis();
    while(client->isConnected() && !cancelled() && !receiveFailed.load() &&
          !secure.load() && millis() - started < PairMs + 8000) {
        if(refreshSecurity()) break;
        vTaskDelay(pdMS_TO_TICKS(20));
    }
    if(!secure.load() || !client->isConnected() || cancelled()) {
        message(allowPair ? "Pairing failed or cancelled. Tap Pair to retry." : "Flipper pairing expired. Tap Pair to approve again.");
        return false;
    }
    auto info = client->getConnInfo();
    if(!info.isAuthenticated() || !info.isEncrypted() || !info.isBonded()) return false;
    if(!rememberedAddress[0] && !confirmed.load()) {
        // A previously unknown device must have an explicit numeric comparison.
        message("Pairing approval required. Forget Flipper bond and Pair again.");
        return false;
    }
    explicitPair = false;
    phase("discovering");
    auto* service = client->getService(GD_BLE_SERVICE_UUID);
    if(!service) { message("Open the Glimdock app on Flipper, then tap Pair."); return false; }
    rx = service->getCharacteristic(GD_BLE_WRITE_UUID);
    auto* ack = service->getCharacteristic(GD_BLE_ACK_UUID);
    auto* control = service->getCharacteristic(GD_BLE_CONTROL_UUID);
    if(!rx || !rx->canWrite() || !ack || !ack->canRead() || !ack->canNotify() ||
       !control || !control->canRead() || !control->canNotify()) {
        message("Unsupported Glimdock Bluetooth service."); return false;
    }
    if(client->getMTU() < GD_CATALOG_ENTRY_BYTES + 3) {
        message("Bluetooth packet size too small for node cache. Reopen the Flipper app.");
        return false;
    }
    phase("subscribing");
    if(!ack->subscribe(true, notifyAck, true)) {
        message("Could not subscribe to Glimdock acknowledgments."); return false;
    }
    portENTER_CRITICAL(&mutex);
    const uint32_t generation = nodeSelections.begin();
    state.linkGeneration = generation; ++state.revision;
    portEXIT_CRITICAL(&mutex);
    if(!control->subscribe(true,
        [generation](NimBLERemoteCharacteristic*, uint8_t* bytes, size_t len, bool) {
            uint32_t sequence; char id[GD_NODE_ID_BYTES];
            if(!secure.load() || !gd_cache_control_decode(bytes, len, &sequence, id, sizeof(id))) return;
            portENTER_CRITICAL(&mutex);
            nodeSelections.push(generation, sequence, id);
            portEXIT_CRITICAL(&mutex);
        }, true)) {
        clearNodeControls();
        message("Could not subscribe to Glimdock node buttons."); return false;
    }
    if(allowPair && confirmed.load()) {
        const auto identity = info.getIdAddress();
        safeText(rememberedAddress, sizeof(rememberedAddress), identity.toString().c_str());
        rememberedType = identity.getType();
        safeText(rememberedName, sizeof(rememberedName), found.name);
        prefs.putString("peer", rememberedAddress); prefs.putUChar("type", rememberedType);
        prefs.putString("name", rememberedName);
        prefs.putString("transport", TransportVersion);
    }
    return true;
}

struct Session {
    uint32_t waiting = 0;
    bool acknowledged = false;
    bool drain() {
        Ack ack;
        while(xQueueReceive(received, &ack, 0) == pdTRUE) {
            uint32_t sequence;
            if(!gd_ble_ack_decode(ack.bytes, sizeof(ack.bytes), &sequence)) return false;
            if(sequence == waiting) acknowledged = true;
        }
        return !receiveFailed.load();
    }
    bool send(const uint8_t* data, size_t len) {
        if(!data || len != GD_DETAIL_BYTES || !gd_cache_packet_sequence(data, len, &waiting) ||
           !gd_detail_sequence_valid(waiting)) return false;
        // Acknowledgment applies to this frame's own wire generation, not to
        // the latest queued frame which may change while this send is active.
        acknowledged = false;
        uint32_t began = millis(); size_t pos = 0;
        const size_t mtu = client->getMTU();
        if(mtu <= 3 + GD_BLE_CHUNK_HEADER) return false;
        size_t payload = mtu - 3 - GD_BLE_CHUNK_HEADER;
        if(payload > GD_BLE_CHUNK_PAYLOAD) payload = GD_BLE_CHUNK_PAYLOAD;
        uint8_t packet[GD_BLE_CHUNK_MAX];
        while(pos < len) {
            if(cancelled() || !client->isConnected() || !drain() || millis() - began >= CommandMs) return false;
            size_t count = payload;
            if(count > len - pos) count = len - pos;
            const size_t size = gd_ble_chunk_encode(data, waiting, (uint16_t)pos, count, packet, sizeof(packet));
            // Write-with-response paces the small fixed receiver. Only one
            // frame is in flight; its complete-frame ACK gates the next send.
            if(!size || !rx->writeValue(packet, size, true)) return false;
            pos += count;
        }
        while(!acknowledged) {
            if(cancelled() || !client->isConnected() || !drain() || millis() - began >= CommandMs) return false;
            vTaskDelay(pdMS_TO_TICKS(10));
        }
        return true;
    }
    bool sendCatalog(const uint8_t* data, size_t len, uint32_t sequence) {
        if(!data || !len || len > client->getMTU() - 3 || !gd_catalog_sequence_valid(sequence)) return false;
        waiting = sequence; acknowledged = false;
        const uint32_t began = millis();
        if(cancelled() || !client->isConnected() || !drain() || !rx->writeValue(data, len, true)) return false;
        while(!acknowledged) {
            if(cancelled() || !client->isConnected() || !drain() || millis() - began >= CommandMs) return false;
            vTaskDelay(pdMS_TO_TICKS(10));
        }
        return true;
    }
};

void worker(void*) {
    bool suspended = false;
    uint32_t retryAt = 0, retryMs = 2000, catalogGeneration = 0;
    uint32_t catalogSequence = GD_CACHE_CATALOG_SEQUENCE_FLAG;
    for(;;) {
        bool saveEnable, pairNow, forgetNow, isEnabled;
        portENTER_CRITICAL(&mutex);
        saveEnable = enableDirty; enableDirty = false;
        pairNow = pairRequested; pairRequested = false;
        forgetNow = forgetRequested; forgetRequested = false;
        isEnabled = requestedEnabled;
        portEXIT_CRITICAL(&mutex);
        if(saveEnable) prefs.putBool("enabled", isEnabled);
        if(forgetNow) {
            resetLink();
            if(NimBLEDevice::getInitialized() && rememberedAddress[0])
                NimBLEDevice::deleteBond(NimBLEAddress(std::string(rememberedAddress), rememberedType));
            rememberedAddress[0] = 0; rememberedName[0] = 0;
            prefs.remove("peer"); prefs.remove("type"); prefs.remove("name");
            peer(""); message("Flipper forgotten. Tap Pair to choose a device.");
            suspended = true;
        }
        if(pairNow) { resetLink(); suspended = false; retryAt = 0; }
        if(!isEnabled) {
            if(client && client->isConnected()) resetLink();
            message("Bluetooth off.");
            vTaskDelay(pdMS_TO_TICKS(100)); continue;
        }
        if(suspended || (!pairNow && !rememberedAddress[0])) {
            if(!suspended) message("Open Glimdock on Flipper, then tap Pair.");
            vTaskDelay(pdMS_TO_TICKS(100)); continue;
        }
        if(!pairNow && (int32_t)(millis() - retryAt) < 0) { vTaskDelay(pdMS_TO_TICKS(100)); continue; }
        if(!initBle()) { retryAt = millis() + 30000; vTaskDelay(pdMS_TO_TICKS(100)); continue; }
        Candidate found;
        // A remembered address may advertise Flipper's stock phone service
        // after Back. Require the app's own advertisement before connecting;
        // reconnect never opens an app or claims that stock Bluetooth link.
        const bool known = rememberedAddress[0] != 0;
        bool chosen = discover(found, known, pairNow ? ScanMs : 3000);
        if(!chosen || !connectPeer(found, pairNow)) {
            resetLink();
            // Preserve an explicit first-pair failure instead of immediately
            // replacing its reason with the generic idle instruction.
            if(pairNow && !rememberedAddress[0]) suspended = true;
            // Delete a rejected/new candidate's transient bond, never another device's.
            if(found.address[0] && !rememberedAddress[0])
                NimBLEDevice::deleteBond(NimBLEAddress(std::string(found.address), found.type));
            explicitPair = false;
            retryAt = millis() + retryMs; retryMs = retryMs < 30000 ? retryMs * 2 : 60000;
            continue;
        }
        Session session;
        bool ok = true;
        phase("streaming");
        message("Connected. Up/Down nodes; Left/Right pages.", true);
        retryMs = 2000;
        uint32_t sentSerial = 0, consumedUrgentSerial = 0, lastStarted = 0;
        uint32_t catalogPublishedAt = 0, catalogCompletedAt = 0;
        uint8_t catalogCursor = 0, committedNodeCount = 0;
        bool catalogInFlight = false, initialCatalogComplete = false;
        while(!controlsPending() && enabled() && client->isConnected()) {
            if(!session.drain()) { ok = false; break; }
            uint8_t status[GD_DETAIL_BYTES]; uint32_t serial, urgentSerial;
            portENTER_CRITICAL(&mutex);
            serial = latestSerial; urgentSerial = latestUrgentSerial;
            if(serial != sentSerial) memcpy(status, latest, sizeof(status));
            portEXIT_CRITICAL(&mutex);
            bool knownDetail = false;
            if(initialCatalogComplete && serial != sentSerial) {
                for(uint8_t i = 0; i < committedNodeCount; ++i) {
                    if(!memcmp(status + 8, committedNodeIds[i], GD_NODE_ID_BYTES)) {
                        knownDetail = true; break;
                    }
                }
            }
            // Urgency is sticky until a send acknowledges a snapshot at or
            // after that request; normal publications cannot clear it. While
            // a frame is in flight, newer navigation coalesces into one latest
            // follow-up instead of building a backlog of old node selections.
            if(knownDetail && serial != sentSerial &&
               (urgentSerial != consumedUrgentSerial || millis() - lastStarted >= 950)) {
                const uint32_t started = millis();
                if(!session.send(status, sizeof(status))) { ok = false; break; }
                portENTER_CRITICAL(&mutex);
                ++state.ackedFrames; state.lastAckMs = millis(); ++state.revision;
                portEXIT_CRITICAL(&mutex);
                sentSerial = serial; consumedUrgentSerial = urgentSerial; lastStarted = started;
                // Re-read the latest urgent request after an in-flight detail
                // ACK before giving a catalog packet the link.
                continue;
            }
            // Keep an active catalog immutable through COMMIT. Root publishes
            // fresh ages every second; updates coalesce until the next refresh
            // rather than repeatedly restarting an unfinished transaction.
            if(!catalogInFlight && (!initialCatalogComplete || millis() - catalogCompletedAt >= 5000)) {
                portENTER_CRITICAL(&mutex);
                if(haveCatalog) {
                    activeCatalog = latestCatalog;
                    if(++catalogGeneration == 0) ++catalogGeneration;
                    activeCatalog.generation = catalogGeneration;
                    catalogPublishedAt = latestCatalogPublishedAt;
                    catalogInFlight = true; catalogCursor = 0;
                }
                portEXIT_CRITICAL(&mutex);
            }
            if(catalogInFlight) {
                portENTER_CRITICAL(&mutex);
                bool urgentWaiting = latestSerial != sentSerial && latestUrgentSerial != consumedUrgentSerial;
                // A detail for a new registry member cannot be accepted until
                // that member's catalog commits. Keep its latest snapshot
                // queued while allowing BEGIN/ENTRY/COMMIT to make progress.
                if(urgentWaiting) {
                    urgentWaiting = false;
                    for(uint8_t i = 0; i < committedNodeCount; ++i) {
                        if(!memcmp(latest + 8, committedNodeIds[i], GD_NODE_ID_BYTES)) {
                            urgentWaiting = true; break;
                        }
                    }
                }
                portEXIT_CRITICAL(&mutex);
                if(initialCatalogComplete && urgentWaiting) continue;
                const uint8_t type = catalogCursor == 0 ? GD_CATALOG_BEGIN :
                    catalogCursor <= activeCatalog.count ? GD_CATALOG_ENTRY : GD_CATALOG_COMMIT;
                const uint8_t index = type == GD_CATALOG_ENTRY ? catalogCursor - 1 : 0;
                uint8_t packet[GD_CATALOG_ENTRY_BYTES];
                GdStatusPage original{};
                if(type == GD_CATALOG_ENTRY) {
                    auto& page = activeCatalog.nodes[index].system;
                    original = page;
                    if(page.age_seconds != GD_STATUS_UNKNOWN_AGE) {
                        const uint32_t age = (uint32_t)page.age_seconds + (millis() - catalogPublishedAt) / 1000;
                        page.age_seconds = age >= GD_STATUS_UNKNOWN_AGE ? GD_STATUS_UNKNOWN_AGE - 1 : (uint16_t)age;
                    }
                }
                if(++catalogSequence == 0) catalogSequence = GD_CACHE_CATALOG_SEQUENCE_FLAG | 1u;
                const size_t size = gd_catalog_encode(&activeCatalog, type, index, catalogSequence,
                                                      packet, sizeof(packet));
                if(type == GD_CATALOG_ENTRY) activeCatalog.nodes[index].system = original;
                if(!size || !session.sendCatalog(packet, size, catalogSequence)) { ok = false; break; }
                if(type == GD_CATALOG_COMMIT) {
                    committedNodeCount = activeCatalog.count;
                    for(uint8_t i = 0; i < committedNodeCount; ++i)
                        memcpy(committedNodeIds[i], activeCatalog.nodes[i].id, GD_NODE_ID_BYTES);
                    catalogInFlight = false; initialCatalogComplete = true; catalogCompletedAt = millis();
                } else ++catalogCursor;
                // Exactly one catalog packet per iteration: urgent/latest
                // selected-node detail always gets the next opportunity.
            }
            vTaskDelay(pdMS_TO_TICKS(20));
        }
        // Preserve any final queued ACK before classifying an application exit
        // or radio loss. Back stops advertising the dedicated service.
        if(!session.drain()) ok = false;
        if(!ok && client->isConnected()) {
            suspended = true;
            message("Flipper did not acknowledge data. Tap Pair to retry.");
        } else if(!controlsPending()) message("Open Glimdock on Flipper to reconnect.");
        resetLink();
        retryAt = millis() + retryMs;
    }
}
} // namespace

void begin() {
    if(begun) return;
    begun = true;
    if(!prefs.begin("gd-flipper", false)) { message("Bluetooth settings unavailable."); return; }
    requestedEnabled = prefs.getBool("enabled", false);
    // The earlier RPC transport remembered the stock BLE address. The native
    // application uses a distinct identity; never constrain discovery to an
    // old stock peer. Keep opt-in and all unrelated preferences untouched.
    const bool currentTransport = prefs.getString("transport", "") == TransportVersion;
    if(!currentTransport) {
        prefs.remove("peer"); prefs.remove("type"); prefs.remove("name");
        if(!prefs.isKey("peer") && !prefs.isKey("type") && !prefs.isKey("name"))
            prefs.putString("transport", TransportVersion);
    }
    // Even if a flash write fails, stale values cannot enter this runtime.
    String address = currentTransport ? prefs.getString("peer", "") : "";
    String name = currentTransport ? prefs.getString("name", "") : "";
    // Do not pass malformed NVS strings into BLE address parsing.
    bool valid = address.length() == 17;
    for(size_t i = 0; valid && i < 17; ++i)
        valid = i % 3 == 2 ? address[i] == ':' : isxdigit((unsigned char)address[i]);
    if(valid) safeText(rememberedAddress, sizeof(rememberedAddress), address.c_str());
    safeText(rememberedName, sizeof(rememberedName), name.c_str());
    rememberedType = currentTransport ? prefs.getUChar("type", 0) : 0;
    if(rememberedType > 1) { rememberedAddress[0] = 0; rememberedType = 0; }
    state.enabled = requestedEnabled; peer(rememberedName);
    received = xQueueCreate(4, sizeof(Ack));
    if(!received || xTaskCreate(worker, "flipper-ble", 6144, nullptr, 1, &workerHandle) != pdPASS) {
        if(received) { vQueueDelete(received); received = nullptr; }
        workerHandle = nullptr; message("Not enough memory for Bluetooth worker.");
    }
}
void setEnabled(bool value) {
    if(!begun) begin();
    if(!workerHandle) return;
    portENTER_CRITICAL(&mutex);
    requestedEnabled = value; state.enabled = value; enableDirty = true;
    if(!value) {
        pairDecision = -1;
        nodeSelections.clear(); state.linkGeneration = nodeSelections.generation();
    }
    ++state.revision;
    portEXIT_CRITICAL(&mutex);
}
void pair() {
    if(!begun) begin();
    if(!workerHandle) return;
    portENTER_CRITICAL(&mutex);
    requestedEnabled = true; state.enabled = true; enableDirty = true;
    pairRequested = true; pairDecision = -1; ++state.revision;
    nodeSelections.clear(); state.linkGeneration = nodeSelections.generation();
    portEXIT_CRITICAL(&mutex);
}
void forget() {
    if(!workerHandle) return;
    portENTER_CRITICAL(&mutex);
    forgetRequested = true; pairDecision = -1; ++state.revision;
    nodeSelections.clear(); state.linkGeneration = nodeSelections.generation();
    portEXIT_CRITICAL(&mutex);
}
bool readState(State& out) {
    portENTER_CRITICAL(&mutex); out = state; portEXIT_CRITICAL(&mutex);
    return begun;
}
bool confirmPairing(bool accept) {
    portENTER_CRITICAL(&mutex);
    const bool waiting = state.pairing;
    if(waiting && (!accept || !state.passkeyEntry)) pairDecision = accept ? 1 : -1;
    portEXIT_CRITICAL(&mutex);
    return waiting;
}
bool submitPasskey(uint32_t passkey) {
    if(passkey > 999999) return false;
    portENTER_CRITICAL(&mutex);
    const bool waiting = state.pairing && state.passkeyEntry;
    if(waiting) { enteredPasskey = passkey; pairDecision = 2; }
    portEXIT_CRITICAL(&mutex);
    return waiting;
}
void publish(const uint8_t* wire, size_t len, bool urgent) {
    if(!wire || len != GD_DETAIL_BYTES) return;
    portENTER_CRITICAL(&mutex);
    memcpy(latest, wire, len);
    if(++latestSerial == 0) ++latestSerial;
    if(urgent) latestUrgentSerial = latestSerial;
    portEXIT_CRITICAL(&mutex);
}
void publishCatalog(const GdNodeCatalog& catalog) {
    // Validate before the shared critical section; only the bounded model copy
    // and its original publication timestamp are protected by the display lock.
    uint8_t packet[GD_CATALOG_ENTRY_BYTES];
    if(!gd_catalog_encode(&catalog, GD_CATALOG_BEGIN, 0, (GD_CACHE_CATALOG_SEQUENCE_FLAG | 1u), packet, sizeof(packet))) return;
    for(uint8_t i = 0; i < catalog.count; ++i)
        if(!gd_catalog_encode(&catalog, GD_CATALOG_ENTRY, i, (GD_CACHE_CATALOG_SEQUENCE_FLAG | 1u), packet, sizeof(packet))) return;
    portENTER_CRITICAL(&mutex);
    latestCatalog = catalog; latestCatalogPublishedAt = millis(); haveCatalog = true;
    portEXIT_CRITICAL(&mutex);
}
bool takeNodeStep(int8_t&) { return false; } // Compatibility while callers move to absolute IDs.
bool takeNodeSelection(char* id, size_t capacity, uint32_t& requestSequence) {
    portENTER_CRITICAL(&mutex);
    const bool available = requestedEnabled && !forgetRequested && !pairRequested &&
                           secure.load() && nodeSelections.take(id, capacity, requestSequence);
    portEXIT_CRITICAL(&mutex);
    return available;
}
} // namespace flipper
#endif
