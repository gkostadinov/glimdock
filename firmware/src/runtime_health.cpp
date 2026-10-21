#include "runtime_health.h"
#ifndef HOMELAB_NATIVE_PREVIEW
#include <Preferences.h>
#include <WiFi.h>
#include <esp_ota_ops.h>
#include <esp_system.h>
#include <esp_wifi.h>
#include <esp32s3/rom/rtc.h>
#include <esp_heap_caps.h>
#include <string.h>

namespace {
constexpr uint32_t Magic = 0x47444831;
struct Checkpoint { uint32_t magic, uptime, minimumHeap; };
RTC_NOINIT_ATTR Checkpoint checkpoint;
struct Boot { uint32_t reason, rom, previousUptime, previousMinHeap; };
struct History { uint32_t magic = Magic, count = 0; Boot boots[8]{}; } history;
uint32_t lastSample = 0;
bool stored = false;
void report(Stream& serial) {
    char hash[17]; const auto* app = esp_ota_get_app_description();
    for(unsigned i = 0; i < 8; ++i) snprintf(hash + i * 2, 3, "%02x", app->app_elf_sha256[i]);
    wifi_ps_type_t sleep = WIFI_PS_NONE; esp_wifi_get_ps(&sleep);
    serial.printf("GLIMDOCK_DIAGNOSTICS {\"schema\":1,\"elf\":\"%s\",\"uptime_ms\":%lu,\"free_heap\":%lu,\"min_free_heap\":%lu,\"largest_free_block\":%lu,\"free_psram\":%lu,\"ui_stack_free\":%lu,\"wifi_status\":%d,\"wifi_rssi\":%d,\"wifi_sleep\":%d,\"boot_count\":%lu,\"history_saved\":%s,\"boots\":[",
        hash, (unsigned long)millis(), (unsigned long)ESP.getFreeHeap(),
        (unsigned long)ESP.getMinFreeHeap(), (unsigned long)ESP.getMaxAllocHeap(),
        (unsigned long)ESP.getFreePsram(), (unsigned long)uxTaskGetStackHighWaterMark(nullptr),
        int(WiFi.status()), int(WiFi.RSSI()), int(sleep), (unsigned long)history.count, stored ? "true" : "false");
    unsigned count = history.count < 8 ? history.count : 8;
    for(unsigned i = 0; i < count; ++i) {
        const auto& boot = history.boots[(history.count - 1 - i) % 8];
        serial.printf("%s{\"reset\":%lu,\"rom\":%lu,\"previous_uptime_ms\":%lu,\"previous_min_heap\":%lu}",
            i ? "," : "", (unsigned long)boot.reason, (unsigned long)boot.rom,
            (unsigned long)boot.previousUptime, (unsigned long)boot.previousMinHeap);
    }
    serial.println("]}");
}
}
void startRuntimeHealth() {
    const Boot boot{uint32_t(esp_reset_reason()), uint32_t(rtc_get_reset_reason(0)),
        checkpoint.magic == Magic ? checkpoint.uptime : 0,
        checkpoint.magic == Magic ? checkpoint.minimumHeap : 0};
    checkpoint = Checkpoint{Magic, 0, ESP.getMinFreeHeap()};
    Preferences prefs;
    if(prefs.begin("gd-health", false)) {
        if(prefs.getBytesLength("boots") == sizeof(history)) prefs.getBytes("boots", &history, sizeof(history));
        if(history.magic != Magic || history.count == UINT32_MAX) history = History{};
        history.boots[history.count % 8] = boot; ++history.count;
        // One bounded write per boot. Running checkpoints stay in RTC RAM.
        stored = prefs.putBytes("boots", &history, sizeof(history)) == sizeof(history);
        prefs.end();
    } else { history.boots[0] = boot; history.count = 1; }
    Serial.printf("Device health: reset=%lu rom=%lu previous_uptime_ms=%lu; boot history %s\n",
        (unsigned long)boot.reason, (unsigned long)boot.rom, (unsigned long)boot.previousUptime,
        stored ? "saved" : "unavailable");
}
void tickRuntimeHealth() {
    const uint32_t now = millis();
    if(uint32_t(now - lastSample) < 1000) return;
    lastSample = now; checkpoint.uptime = now; checkpoint.minimumHeap = ESP.getMinFreeHeap();
}
bool handleRuntimeHealthLine(char* line, size_t length, Stream& serial) {
    constexpr char drop[] = "GLIMDOCK_DIAGNOSTICS_WIFI_DROP";
    constexpr char command[] = "GLIMDOCK_DIAGNOSTICS";
    size_t size = length; if(size && line[size - 1] == '\r') --size;
    if(size == sizeof(drop) - 1 && !memcmp(line, drop, size)) {
        memset(line, 0, length);
        // Physical USB-only recovery check. No radio shutdown or NVS erase.
        WiFi.disconnect(false, false);
        serial.println("GLIMDOCK_DIAGNOSTICS_EVENT {\"wifi_drop_test\":true}");
        return true;
    }
    if(size != sizeof(command) - 1 || memcmp(line, command, size)) return false;
    memset(line, 0, length); report(serial); return true;
}
#endif
