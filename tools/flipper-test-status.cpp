// Synthetic wire/projection checks. No live collector, credentials, BLE or Flipper.
#include "flipper_status.h"

#include <array>
#include <cassert>
#include <iostream>
#include <limits>
#include <memory>
#include <string>

uint32_t millis() { return 0; }

namespace {
constexpr double Epoch = 1700000100;

template<size_t N> void set(char (&out)[N], const char* value) {
    const size_t n = std::min(N - 1, strlen(value));
    memset(out, 0, N);
    memcpy(out, value, n);
}

void row(const GdStatusPage& page, unsigned index, const char* expected) {
    if(strcmp(page.rows[index], expected)) {
        std::cerr << page.title << '[' << index << "]: expected [" << expected
                  << "] got [" << page.rows[index] << "]\n";
        assert(false);
    }
}

void same(const GdStatus& a, const GdStatus& b) {
    assert(a.sequence == b.sequence);
    for(unsigned i = 0; i < GD_STATUS_PAGES; ++i) {
        assert(a.pages[i].flags == b.pages[i].flags);
        assert(a.pages[i].age_seconds == b.pages[i].age_seconds);
        assert(a.pages[i].ttl_seconds == b.pages[i].ttl_seconds);
        assert(!memcmp(a.pages[i].title, b.pages[i].title, GD_STATUS_TITLE));
        assert(!memcmp(a.pages[i].rows, b.pages[i].rows,
                       GD_STATUS_ROWS * GD_STATUS_TEXT));
    }
}

using Wire = std::array<uint8_t, GD_STATUS_BYTES>;

void rejected(const uint8_t* wire, size_t size) {
    GdStatus displayed;
    memset(&displayed, 0xa7, sizeof(displayed));
    std::array<uint8_t, sizeof(GdStatus)> before{};
    memcpy(before.data(), &displayed, sizeof(displayed));
    assert(!gd_status_decode(wire, size, &displayed));
    assert(!memcmp(before.data(), &displayed, sizeof(displayed)));
}

void protocol() {
    static_assert(GD_STATUS_PAGE_BYTES == 109, "v1 page layout changed");
    static_assert(GD_STATUS_BYTES == 444, "v1 wire layout changed");
    GdStatus status{};
    status.sequence = 0x78563412;
    Wire expected{};
    expected[0] = 'G'; expected[1] = 'D'; expected[2] = 1; expected[3] = 4;
    expected[4] = 0x12; expected[5] = 0x34; expected[6] = 0x56; expected[7] = 0x78;
    for(unsigned i = 0; i < 4; ++i) {
        auto& page = status.pages[i];
        page.flags = i == 0 ? GD_STATUS_VALID : i == 1 ? GD_STATUS_STALE :
                     i == 2 ? GD_STATUS_VALID | GD_STATUS_DEMO :
                              GD_STATUS_VALID | GD_STATUS_STALE | GD_STATUS_DEMO;
        page.age_seconds = i == 3 ? GD_STATUS_UNKNOWN_AGE : 0x1234 + i;
        page.ttl_seconds = 0x2345 + i;
        set(page.title, "Status page");
        set(page.rows[0], "System ready");
        set(page.rows[3], "Last row");
        // Independent v1 fixture, including zero padding and little endian fields.
        const size_t offset = 8 + i * 109;
        expected[offset] = page.flags;
        expected[offset + 1] = i == 3 ? 0xff : 0x34 + i;
        expected[offset + 2] = i == 3 ? 0xff : 0x12;
        expected[offset + 3] = 0x45 + i;
        expected[offset + 4] = 0x23;
        memcpy(expected.data() + offset + 5, "Status page", 11);
        memcpy(expected.data() + offset + 21, "System ready", 12);
        memcpy(expected.data() + offset + 87, "Last row", 8);
    }
    std::array<uint8_t, GD_STATUS_BYTES + 2> guarded;
    guarded.fill(0xa5);
    assert(gd_status_encode(&status, guarded.data() + 1, GD_STATUS_BYTES));
    assert(guarded.front() == 0xa5 && guarded.back() == 0xa5);
    assert(!memcmp(expected.data(), guarded.data() + 1, expected.size()));
    GdStatus decoded{};
    assert(gd_status_decode(expected.data(), expected.size(), &decoded));
    same(status, decoded);
    Wire wire{};
    assert(gd_status_encode(&decoded, wire.data(), wire.size()));
    assert(wire == expected);

    for(size_t length = 0; length < wire.size(); ++length) rejected(wire.data(), length);
    rejected(guarded.data() + 1, GD_STATUS_BYTES + 1);
    rejected(nullptr, GD_STATUS_BYTES);
    assert(!gd_status_decode(wire.data(), wire.size(), nullptr));
    guarded.fill(0xa5);
    assert(!gd_status_encode(&status, guarded.data() + 1, GD_STATUS_BYTES - 1));
    for(uint8_t byte : guarded) assert(byte == 0xa5);
    assert(!gd_status_encode(nullptr, wire.data(), wire.size()));
    assert(!gd_status_encode(&status, nullptr, wire.size()));
    for(unsigned header = 0; header < 4; ++header) {
        Wire bad = expected; bad[header] ^= 0x80;
        rejected(bad.data(), bad.size());
    }
    for(unsigned page = 0; page < GD_STATUS_PAGES; ++page) {
        const size_t base = 8 + page * GD_STATUS_PAGE_BYTES;
        Wire bad = expected; bad[base] |= 0x80;
        rejected(bad.data(), bad.size());
        bad = expected; bad[base + 3] = bad[base + 4] = 0;
        rejected(bad.data(), bad.size());
        // Every title/row must terminate, contain printable ASCII, and have zero padding.
        for(unsigned field = 0; field <= GD_STATUS_ROWS; ++field) {
            const size_t offset = base + 5 + (field ? GD_STATUS_TITLE + (field - 1) * GD_STATUS_TEXT : 0);
            const size_t length = field ? GD_STATUS_TEXT : GD_STATUS_TITLE;
            for(uint8_t invalid : {uint8_t(1), uint8_t('\n'), uint8_t(127), uint8_t(0xc3)}) {
                bad = expected; bad[offset] = invalid;
                rejected(bad.data(), bad.size());
            }
            bad = expected; memset(bad.data() + offset, 'x', length);
            rejected(bad.data(), bad.size());
            bad = expected; memset(bad.data() + offset, 0, length);
            bad[offset + length - 1] = 'x';
            rejected(bad.data(), bad.size());
        }
    }
    status.pages[0].ttl_seconds = 0;
    assert(!gd_status_encode(&status, wire.data(), wire.size()));

    GdStatusPage page{};
    page.flags = GD_STATUS_VALID; page.age_seconds = 3; page.ttl_seconds = 15;
    assert(!gd_status_page_stale(&page, 12));
    assert(gd_status_page_stale(&page, 13));
    assert(gd_status_page_stale(&page, UINT32_MAX));
    page.age_seconds = GD_STATUS_UNKNOWN_AGE;
    assert(gd_status_page_stale(&page, 0));
    page.age_seconds = 0; page.flags |= GD_STATUS_STALE;
    assert(gd_status_page_stale(&page, 0));
    page.flags = GD_STATUS_DEMO;
    assert(gd_status_page_stale(&page, 0));
}

struct Fixture {
    // Both inventories exceed embedded task-stack budgets; fixtures live on the heap too.
    std::unique_ptr<Snapshot> s{new Snapshot{}};
    std::unique_ptr<ops::Snapshot> o{new ops::Snapshot{}};
    NetworkState net{};
    ops::Network onet{};
    uint32_t now = 50000;
    double epoch = Epoch + 1;
    GdStatus status{};

    Fixture() {
        s->valid = true; s->generated = Epoch; s->received = 49000;
        set(s->node.id, "selected"); set(s->node.name, "Selected server");
        set(s->node.status, "healthy"); set(s->host, "Other host name");
        set(s->ip, "192.0.2.23"); set(s->node.address, "secret-endpoint.invalid");
        s->cpu = 24; s->memUsed = 2; s->memTotal = 4; s->temp = 49;
        net.wifi = true; onet.connected = true;
        o->valid = true; o->generated = Epoch; o->received = 49000;
        o->nSources = 1; set(o->sources[0].status, "healthy");
        o->sources[0].sampled = Epoch; o->sources[0].ttl = 15;
    }
    void build(uint32_t sequence = 42, bool operations = true) {
        flipper::buildStatus(*s, net, operations ? o.get() : nullptr, onet,
                             now, epoch, sequence, status);
        Wire wire{};
        assert(gd_status_encode(&status, wire.data(), wire.size()));
        const std::string encoded(reinterpret_cast<const char*>(wire.data()), wire.size());
        assert(encoded.find("192.0.2.23") == std::string::npos);
        assert(encoded.find("secret-endpoint") == std::string::npos);
    }
};

void systemAndAlerts() {
    auto f = std::make_unique<Fixture>();
    f->s->nNodes = 1;
    set(f->s->nodes[0].name, "Different node"); f->s->nodes[0].summary.cpu = 99;
    f->s->nAlerts = 4;
    set(f->s->alerts[0].severity, "warning"); set(f->s->alerts[0].message, "Low priority");
    set(f->s->alerts[1].severity, "critical"); set(f->s->alerts[1].message, "Disk failed");
    set(f->s->alerts[2].severity, "error"); set(f->s->alerts[2].message, "Temperature critical");
    set(f->s->alerts[3].severity, "warning"); set(f->s->alerts[3].message, "Other warning");
    f->build();
    assert(f->status.sequence == 42);
    assert(!gd_status_page_stale(&f->status.pages[0], 0));
    row(f->status.pages[0], 0, "Selected server");
    row(f->status.pages[0], 1, "CPU 24% RAM 50%");
    row(f->status.pages[0], 2, "Temp 49C");
    row(f->status.pages[1], 0, "Selected server");
    row(f->status.pages[1], 1, "4 active alerts");
    row(f->status.pages[1], 2, "Disk failed");
    row(f->status.pages[1], 3, "Temperature critical");
    f->s->nAlerts = 0; f->build();
    row(f->status.pages[1], 2, "No reported alerts");
    f->s->nSources = 1; f->s->sources[0].ok = false; f->build();
    row(f->status.pages[1], 3, "Some sources missing");
    f->s->cpu = UNKNOWN; f->s->memTotal = 0; f->s->temp = UNKNOWN;
    f->s->node.name[0] = 0; f->build();
    row(f->status.pages[0], 0, "Other host name");
    row(f->status.pages[0], 1, "CPU --% RAM --%");
    row(f->status.pages[0], 2, "Temp --C");
    set(f->s->node.name, "Long name \xf0\x9f\x98\x80\nfor the selected system");
    f->build(); row(f->status.pages[0], 0, "Long name ??for the s");
    assert(strlen(f->status.pages[0].rows[0]) == GD_STATUS_TEXT - 1);
}

void registrySummary() {
    auto f = std::make_unique<Fixture>();
    NodeDescriptor node{};
    set(node.id, "other-node"); set(node.name, "Other server"); set(node.status, "healthy");
    node.summary.available = true; node.summary.cpu = 73; node.summary.memory = 41;
    node.summary.temp = 37; node.summary.age = 2; node.summary.generated = Epoch - 1;
    node.summary.ttl = 15; set(node.summary.status, "healthy");
    GdStatusPage page{};
    auto build = [&] { flipper::buildNodeSummary(node, *f->s, f->net, f->now, f->epoch, page); };
    build();
    row(page, 0, "Other server"); row(page, 1, "CPU 73% RAM 41%"); row(page, 2, "Temp 37C");
    assert(page.age_seconds == 3 && !gd_status_page_stale(&page, 0));
    // Selected detail invalidation cannot remove the independent registry cache.
    f->s->valid = false; f->net.loading = true; build();
    assert(page.flags & GD_STATUS_VALID); assert(!gd_status_page_stale(&page, 0));
    f->now += 16000; f->epoch += 16; build();
    assert(page.age_seconds == 19 && gd_status_page_stale(&page, 0));
    f->now -= 16000; f->epoch -= 16;
    set(node.status, "offline"); build();
    row(page, 3, "System offline"); assert(gd_status_page_stale(&page, 0));
    set(node.status, "healthy"); f->net.wifi = false; build();
    assert(gd_status_page_stale(&page, 0));
    f->s->demo = true; build();
    assert(page.flags & GD_STATUS_DEMO); assert(!gd_status_page_stale(&page, 0));
    f->net.wifi = true; node.summary.generated = Epoch + 90; build();
    assert(gd_status_page_stale(&page, 0));
    node.summary.generated = NAN; node.summary.age = NAN; f->s->generated = NAN; build();
    assert(page.age_seconds == GD_STATUS_UNKNOWN_AGE && gd_status_page_stale(&page, 0));
    f->s->generated = Epoch; node.summary.age = 2;
    set(node.type, "klipper"); set(node.summary.printState, "printing"); node.summary.progress = 54; build();
    row(page, 2, "printing 54%");
    node.summary.available = false; build();
    assert(!(page.flags & GD_STATUS_VALID)); row(page, 1, "CPU --% RAM --%");
    row(page, 3, "Summary unavailable");
}

void loadingSummary() {
    auto f = std::make_unique<Fixture>();
    f->s->sequence = 123; f->s->uptime = 9876;
    f->s->nAlerts = 1; set(f->s->alerts[0].message, "Previous node alert");
    f->s->printer.present = true; set(f->s->printer.filename, "previous-node.gcode");
    f->s->nNodes = 2;
    f->s->nodes[0] = f->s->node;
    auto& target = f->s->nodes[1];
    set(target.id, "next-server"); set(target.name, "Next server"); set(target.status, "healthy");
    auto& summary = target.summary;
    summary.available = true; summary.cpu = 77; summary.memory = 66; summary.temp = 31;
    summary.generated = Epoch; summary.age = 0; summary.ttl = 15;
    set(summary.status, "healthy");
    clearForNode(*f->s, target);
    assert(!f->s->valid && f->s->sequence == 0);
    assert(f->s->generated == Epoch && f->s->received == 49000);
    assert(f->s->nNodes == 2 && !strcmp(f->s->nodes[0].id, "selected"));
    assert(!strcmp(f->s->node.id, "next-server") && f->s->node.summary.available);
    assert(std::isnan(f->s->cpu) && std::isnan(f->s->memUsed) && std::isnan(f->s->memTotal));
    assert(std::isnan(f->s->temp) && std::isnan(f->s->uptime));
    assert(!f->s->nAlerts && !f->s->host[0] && !f->s->ip[0] && !f->s->printer.present);
    f->net.loading = true; f->build();
    const auto& page = f->status.pages[0];
    assert(page.flags & GD_STATUS_VALID);
    assert(!gd_status_page_stale(&page, 0) && page.age_seconds == 1);
    row(page, 0, "Next server"); row(page, 1, "CPU 77% RAM 66%");
    row(page, 2, "Temp 31C"); row(page, 3, "Summary; loading...");
    assert(!(f->status.pages[1].flags & GD_STATUS_VALID));
    row(f->status.pages[1], 1, "Alerts unavailable");
    row(f->status.pages[1], 2, ""); row(f->status.pages[1], 3, "");

    // Older envelopes and newly emitted BLE frames cannot refresh a registry sample.
    f->now = 70000; f->epoch = Epoch + 21; f->build(60);
    assert(gd_status_page_stale(&page, 0) && page.age_seconds == 21);
    row(page, 1, "CPU 77% RAM 66%");
    f->build(61); assert(gd_status_page_stale(&page, 0) && page.age_seconds == 21);
    f->epoch = 0; f->build(62);
    assert(gd_status_page_stale(&page, 0) && page.age_seconds == 21);
    clearForNode(*f->s, f->s->node); f->build(63);
    assert(gd_status_page_stale(&page, 0) && page.age_seconds == 21);

    // Source TTL, not receipt of a node-selection command, controls the preview.
    f->s->node.summary.ttl = 5; f->now = 54000; f->epoch = Epoch + 5; f->build();
    assert(!gd_status_page_stale(&page, 0) && page.ttl_seconds == 5);
    f->now = 55000; f->epoch = Epoch + 6; f->build();
    assert(gd_status_page_stale(&page, 0));
    f->now = 50000; f->epoch = Epoch + 1; f->s->node.summary.ttl = 15;
    f->s->node.summary.age = NAN; f->s->node.summary.generated = NAN; f->build();
    assert(gd_status_page_stale(&page, 0) && page.age_seconds == GD_STATUS_UNKNOWN_AGE);
    f->s->node.summary.age = 0; f->s->node.summary.generated = Epoch + 100; f->build();
    assert(gd_status_page_stale(&page, 0));
    f->s->node.summary.generated = Epoch; set(f->s->node.summary.status, "offline"); f->build();
    assert(gd_status_page_stale(&page, 0));
    set(f->s->node.summary.status, "healthy"); f->net.wifi = false; f->build();
    assert(gd_status_page_stale(&page, 0));
    f->s->demo = true; clearForNode(*f->s, f->s->node); f->build();
    assert(f->s->demo && (page.flags & GD_STATUS_DEMO) && !gd_status_page_stale(&page, 0));

    // Defensive projection also ignores legacy details left in an invalid model.
    f->s->cpu = 91; f->s->memUsed = 3; f->s->memTotal = 4; f->s->temp = 98;
    set(f->s->host, "Previous host"); f->s->nAlerts = 1;
    set(f->s->alerts[0].message, "Previous node alert");
    f->s->printer.present = true; set(f->s->printer.filename, "previous-node.gcode");
    f->s->node.summary.available = false; f->build();
    assert(!(page.flags & GD_STATUS_VALID));
    row(page, 1, "CPU --% RAM --%"); row(page, 2, "Temp --C");
    row(page, 3, "Loading selected node"); row(f->status.pages[1], 2, "");
    row(f->status.pages[3], 0, "No printer configured");
    f->s->node.name[0] = 0; f->build(); row(page, 0, "next-server");

    // Once details arrive, the live node's readings replace the cached summary.
    f->s->valid = true; f->net.loading = false; f->net.wifi = true;
    f->s->cpu = 12; f->s->memUsed = 4; f->s->memTotal = 8; f->s->temp = 44;
    f->s->node.summary.available = true; f->build();
    row(page, 1, "CPU 12% RAM 50%"); row(page, 2, "Temp 44C"); row(page, 3, "healthy");
}

void freshness() {
    auto f = std::make_unique<Fixture>();
    f->s->valid = false; f->build();
    assert(!(f->status.pages[0].flags & GD_STATUS_VALID));
    assert(gd_status_page_stale(&f->status.pages[0], 0));
    row(f->status.pages[0], 3, "Waiting for readings");
    f->s->valid = true; f->net.wifi = false; f->build();
    assert(gd_status_page_stale(&f->status.pages[0], 0));
    f->s->demo = true; f->build();
    assert(f->status.pages[0].flags & GD_STATUS_DEMO);
    assert(!gd_status_page_stale(&f->status.pages[0], 0));
    f->s->demo = false; f->net.wifi = true; f->net.loading = true; f->build();
    assert(gd_status_page_stale(&f->status.pages[0], 0));
    row(f->status.pages[0], 3, "Loading selected node");
    f->net.loading = false; set(f->s->node.status, "offline"); f->build();
    assert(gd_status_page_stale(&f->status.pages[0], 0));
    row(f->status.pages[0], 3, "System offline");
    set(f->s->node.status, "healthy");
    f->now = 70000; f->epoch = Epoch + 21; f->build(50);
    assert(f->status.pages[0].age_seconds == 21);
    assert(gd_status_page_stale(&f->status.pages[0], 0));
    // A new BLE sequence/reconnection cannot renew the source snapshot's age.
    f->net.wifi = false; f->build(51);
    f->net.wifi = true; f->build(52);
    assert(f->status.pages[0].age_seconds == 21);
    assert(gd_status_page_stale(&f->status.pages[0], 0));
    // Even newly received collector data keeps an old generated timestamp stale.
    f->s->received = f->now; f->build(53);
    assert(f->status.pages[0].age_seconds == 21);
    assert(gd_status_page_stale(&f->status.pages[0], 0));
    f->s->generated = Epoch + 100; f->build();
    assert(f->status.pages[0].age_seconds == GD_STATUS_UNKNOWN_AGE);
    assert(gd_status_page_stale(&f->status.pages[0], 0));
    // millis wraps every ~49 days; unsigned subtraction must still age the packet.
    f->epoch = 0; f->s->generated = Epoch;
    f->s->received = UINT32_MAX - 1499; f->now = 500; f->build();
    assert(f->status.pages[0].age_seconds == 2);
    assert(!gd_status_page_stale(&f->status.pages[0], 0));
    f->now = 15000; f->build();
    assert(f->status.pages[0].age_seconds == 16);
    assert(gd_status_page_stale(&f->status.pages[0], 0));
}

void printer() {
    auto f = std::make_unique<Fixture>();
    f->build();
    assert(!(f->status.pages[3].flags & GD_STATUS_VALID));
    row(f->status.pages[3], 0, "No printer configured");
    f->s->nNodes = 1;
    auto& node = f->s->nodes[0]; set(node.type, "klipper"); set(node.name, "Workshop printer");
    set(node.status, "healthy"); node.summary.available = true;
    node.summary.generated = Epoch; node.summary.age = 0; node.summary.progress = 65;
    set(node.summary.status, "healthy"); set(node.summary.printState, "printing");
    f->build();
    assert(!gd_status_page_stale(&f->status.pages[3], 0));
    row(f->status.pages[3], 0, "Workshop printer");
    row(f->status.pages[3], 1, "printing 65%");
    row(f->status.pages[3], 2, "Select on Glimdock");
    node.summary.age = 80; f->build();
    assert(gd_status_page_stale(&f->status.pages[3], 0));
    node.summary.age = NAN; node.summary.generated = NAN; f->build();
    assert(gd_status_page_stale(&f->status.pages[3], 0));
    node.summary.age = 0; node.summary.available = false; f->build();
    assert(!(f->status.pages[3].flags & GD_STATUS_VALID));
    auto& p = f->s->printer; p.present = true; set(p.name, "Selected printer");
    set(p.state, "printing"); set(p.klippyState, "ready"); set(p.filename, "bracket.gcode");
    p.progress = 37; p.remaining = 3660; p.updated = Epoch; p.age = 0; p.ttl = 15;
    f->build();
    assert(!gd_status_page_stale(&f->status.pages[3], 0));
    row(f->status.pages[3], 0, "Selected printer");
    row(f->status.pages[3], 1, "printing 37%");
    row(f->status.pages[3], 2, "Remaining 1h 01m");
    row(f->status.pages[3], 3, "bracket.gcode");
    p.age = NAN; p.updated = NAN; f->build();
    assert(gd_status_page_stale(&f->status.pages[3], 0));
    // Cached printer metadata is not renewed by a new collector envelope or BLE packet.
    p.age = 80; p.updated = Epoch; f->build(60);
    assert(f->status.pages[3].age_seconds >= 80);
    assert(gd_status_page_stale(&f->status.pages[3], 0));
    p.age = 0; p.updated = Epoch - 80; f->build(61);
    assert(gd_status_page_stale(&f->status.pages[3], 0));
    p.updated = Epoch; set(p.error, "Thermal shutdown"); f->build();
    assert(gd_status_page_stale(&f->status.pages[3], 0));
    row(f->status.pages[3], 3, "Thermal shutdown");
    p.error[0] = 0; set(p.klippyState, "disconnected"); f->build();
    assert(gd_status_page_stale(&f->status.pages[3], 0));
    row(f->status.pages[3], 1, "disconnected --%");
    set(p.klippyState, "shutdown"); f->build();
    assert(gd_status_page_stale(&f->status.pages[3], 0));
    row(f->status.pages[3], 1, "shutdown --%");
    row(f->status.pages[3], 2, "Remaining --");
    set(p.klippyState, "ready"); p.remaining = -1; f->build();
    row(f->status.pages[3], 2, "Remaining --");
    p.remaining = std::numeric_limits<double>::infinity(); f->build();
    row(f->status.pages[3], 2, "Remaining --");
    p.remaining = 604800; f->build(); row(f->status.pages[3], 2, "Remaining --");
    f->s->nSources = 1; f->s->sources[0].ok = false; f->build();
    assert(gd_status_page_stale(&f->status.pages[3], 0));
}

void agents() {
    auto f = std::make_unique<Fixture>();
    f->build(1, false);
    assert(!(f->status.pages[2].flags & GD_STATUS_VALID));
    row(f->status.pages[2], 0, "Operations unavailabl");
    // Source-provided summaries may cover unseen data; count only real bounded records.
    f->o->nSystems = 1; f->o->systems[0].running = 999;
    f->o->nAgents = 5;
    for(unsigned i = 0; i < 5; ++i) {
        auto& a = f->o->agents[i]; a.source = 0; a.heartbeat = Epoch; a.ttl = 15;
        set(a.state, i ? "idle" : "running");
    }
    set(f->o->agents[1].state, "running"); f->o->agents[1].heartbeat = Epoch - 50;
    set(f->o->agents[2].state, "running"); f->o->agents[2].source = 255;
    set(f->o->agents[3].state, "running"); f->o->agents[3].heartbeat = NAN;
    f->o->nTasks = 5;
    const char* states[] = {"queued", "blocked", "waiting", "failed", "queued"};
    for(unsigned i = 0; i < 5; ++i) {
        f->o->tasks[i].source = i == 4 ? 255 : 0;
        set(f->o->tasks[i].state, states[i]);
    }
    f->build();
    assert(!gd_status_page_stale(&f->status.pages[2], 0));
    row(f->status.pages[2], 0, "1 running agents");
    row(f->status.pages[2], 1, "1 queued 1 blocked");
    row(f->status.pages[2], 2, "1 waiting 1 failed");
    row(f->status.pages[2], 3, "3 stale src/agents");
    // The parser orders truncated[] as systems, agents, tasks, events, resources, channels.
    for(unsigned index = 0; index < 6; ++index) {
        memset(f->o->truncated, 0, sizeof(f->o->truncated)); f->o->truncated[index] = true;
        f->build();
        row(f->status.pages[2], 3, index == 1 || index == 2 ? "Partial inventory" : "3 stale src/agents");
    }
    memset(f->o->truncated, 0, sizeof(f->o->truncated));
    f->o->nSources = 2; set(f->o->sources[1].status, "healthy");
    f->o->sources[1].sampled = Epoch - 80; f->o->sources[1].ttl = 15;
    f->o->agents[2].source = 1; f->o->tasks[4].source = 1;
    f->build();
    assert(gd_status_page_stale(&f->status.pages[2], 0));
    row(f->status.pages[2], 0, "1 running agents");
    row(f->status.pages[2], 1, "1 queued 1 blocked");
    row(f->status.pages[2], 3, "4 stale src/agents");
    // Fresh operations envelopes and BLE reconnects cannot revive an old source sample.
    f->o->sources[0].sampled = Epoch - 80; f->o->received = f->now;
    f->onet.connected = false; f->build(70);
    f->onet.connected = true; f->build(71);
    assert(!(f->status.pages[2].flags & GD_STATUS_VALID));
    assert(gd_status_page_stale(&f->status.pages[2], 0));
    row(f->status.pages[2], 1, "No fresh sources");
    f->o->nSources = 1; f->o->sources[0].sampled = Epoch;
    set(f->o->sources[0].status, "degraded"); f->build();
    assert(f->status.pages[2].flags & GD_STATUS_VALID);
    set(f->o->sources[0].status, "offline"); f->build();
    assert(!(f->status.pages[2].flags & GD_STATUS_VALID));
    set(f->o->sources[0].status, "healthy"); f->onet.connected = false;
    f->o->demo = true; f->build();
    assert(f->status.pages[2].flags & GD_STATUS_DEMO);
    assert(f->status.pages[2].flags & GD_STATUS_VALID);
}
} // namespace

int main() {
    protocol();
    systemAndAlerts();
    registrySummary();
    loadingSummary();
    freshness();
    printer();
    agents();
    std::cout << "Flipper wire, compact pages, freshness, rollover and bounded inventory checks passed.\n";
}
