#pragma once
#include "model.h"
#include "operations_model.h"
#include "flipper_protocol.h"
#include "flipper_cache_protocol.h"
#include <stdarg.h>
#include <stdio.h>

namespace flipper {
// Only monitoring and public operations models enter this projection.
// It never reads private conversations, credentials, endpoints or device IPs.
inline void text(char* out, size_t capacity, const char* input) {
    memset(out, 0, capacity);
    size_t n = 0;
    for(size_t i = 0; input && input[i] && n + 1 < capacity; ++i) {
        const unsigned char c = input[i];
        if(c >= 32 && c <= 126) out[n++] = char(c);
        else if(c < 128 || (c & 0xc0) != 0x80) out[n++] = '?';
    }
}
inline void line(GdStatusPage& page, unsigned row, const char* format, ...) {
    char buffer[160];
    va_list args;
    va_start(args, format);
    vsnprintf(buffer, sizeof(buffer), format, args);
    va_end(args);
    text(page.rows[row], GD_STATUS_TEXT, buffer);
}
inline uint16_t seconds(double value) {
    return !isfinite(value) || value < 0 ? GD_STATUS_UNKNOWN_AGE :
           uint16_t(value > 65534 ? 65534 : value);
}
inline double later(double a, double b) {
    return !isfinite(a) ? b : !isfinite(b) ? a : (a > b ? a : b);
}
inline double timestampAge(double timestamp, double generated, double elapsed, double epoch) {
    if(!isfinite(timestamp)) return NAN;
    double now = epoch >= 1700000000 ? epoch : generated + elapsed;
    if(!isfinite(now) || timestamp > now + 30) return NAN;
    const double age = now - timestamp;
    return age > 0 ? age : 0;
}
inline double envelopeAge(double generated, uint32_t received, uint32_t now, double epoch) {
    const double elapsed = uint32_t(now - received) / 1000.0;
    if(isfinite(generated) && epoch >= 1700000000 && generated > epoch + 30) return NAN;
    return later(elapsed, timestampAge(generated, generated, elapsed, epoch));
}
inline double sampleAge(double age, double updated, double generated, double elapsed, double epoch) {
    const double stored = isfinite(age) && age >= 0 ? age + elapsed : NAN;
    return later(stored, timestampAge(updated, generated, elapsed, epoch));
}
inline bool offline(const char* state) { return !strcmp(state, "offline") || !strcmp(state, "error"); }
inline void freshness(GdStatusPage& page, bool valid, bool fresh, bool demo, double age, double ttl = 15) {
    page.flags = (valid ? GD_STATUS_VALID : 0) | (fresh ? 0 : GD_STATUS_STALE) |
                 (demo ? GD_STATUS_DEMO : 0);
    page.age_seconds = seconds(age);
    page.ttl_seconds = isfinite(ttl) && ttl >= 1 ? uint16_t(ttl > 65534 ? 65534 : ttl) : 1;
}
inline void metric(char* out, size_t cap, double value, const char* unit) {
    if(isfinite(value)) snprintf(out, cap, "%.0f%s", value, unit);
    else snprintf(out, cap, "--%s", unit);
}
// Registry summaries are independent of the currently fetched node. Keep their
// original sample age and envelope age when sending them to the Flipper cache.
inline void buildNodeSummary(const NodeDescriptor& node, const Snapshot& s,
                             const NetworkState& net, uint32_t now, double epoch,
                             GdStatusPage& out) {
    memset(&out, 0, sizeof(out));
    text(out.title, sizeof(out.title), "System");
    const auto& summary = node.summary;
    const double elapsed = uint32_t(now - s.received) / 1000.0;
    const double envelope = envelopeAge(s.generated, s.received, now, epoch);
    const double sample = sampleAge(summary.age, summary.generated, s.generated, elapsed, epoch);
    const double age = isfinite(sample) && isfinite(envelope) ? later(sample, envelope) : NAN;
    const double ttl = isfinite(summary.ttl) && summary.ttl >= 1 ?
                       (summary.ttl < 15 ? summary.ttl : 15) : 1;
    const char* state = summary.status[0] ? summary.status : node.status;
    const bool fresh = summary.available && (net.wifi || s.demo) &&
                       isfinite(age) && age <= ttl && isfinite(summary.ttl) && summary.ttl >= 1 &&
                       (!strcmp(state, "healthy") || !strcmp(state, "degraded")) &&
                       !offline(node.status) &&
                       !(isfinite(summary.generated) && epoch >= 1700000000 && summary.generated > epoch + 30);
    freshness(out, summary.available, fresh, s.demo, age, ttl);
    line(out, 0, "%s", node.name[0] ? node.name : node.id);
    char cpu[16], ram[16], temp[16];
    metric(cpu, sizeof(cpu), summary.available ? summary.cpu : NAN, "%");
    metric(ram, sizeof(ram), summary.available ? summary.memory : NAN, "%");
    metric(temp, sizeof(temp), summary.available ? summary.temp : NAN, "C");
    line(out, 1, "CPU %s RAM %s", cpu, ram);
    if(!strcmp(node.type, "klipper")) {
        metric(temp, sizeof(temp), summary.available ? summary.progress : NAN, "%");
        line(out, 2, "%s %s", summary.printState[0] ? summary.printState : "Printer", temp);
    } else line(out, 2, "Temp %s", temp);
    line(out, 3, "%s", !summary.available ? "Summary unavailable" :
         offline(node.status) || offline(state) ? "System offline" : state[0] ? state : "Status unknown");
}
inline void buildStatus(const Snapshot& s, const NetworkState& net, const ops::Snapshot* o,
                        const ops::Network& onet, uint32_t now, double epoch,
                        uint32_t sequence, GdStatus& out) {
    memset(&out, 0, sizeof(out));
    out.sequence = sequence;
    const double elapsed = uint32_t(now - s.received) / 1000.0;
    const double age = envelopeAge(s.generated, s.received, now, epoch);
    const bool valid = s.valid;
    const bool fresh = valid && (net.wifi || s.demo) && isfinite(age) && age <= 15 &&
                       !offline(s.node.status) && !net.loading;
    // Node selection clears all detail but retains its registry and original
    // timing/provenance. Project only that selected descriptor while fetching.
    const auto& selectedSummary = s.node.summary;
    const bool summary = !valid && net.loading && selectedSummary.available;
    const double summaryAge = summary ? sampleAge(selectedSummary.age, selectedSummary.generated,
                                                   s.generated, elapsed, epoch) : NAN;
    const double summaryTtl = isfinite(selectedSummary.ttl) && selectedSummary.ttl >= 1 ?
                              (selectedSummary.ttl < 15 ? selectedSummary.ttl : 15) : 1;
    const char* summaryState = selectedSummary.status[0] ? selectedSummary.status : s.node.status;
    const bool summaryFresh = summary && (net.wifi || s.demo) &&
                              isfinite(selectedSummary.ttl) && selectedSummary.ttl >= 1 &&
                              isfinite(summaryAge) && summaryAge <= summaryTtl &&
                              (!strcmp(summaryState, "healthy") || !strcmp(summaryState, "degraded")) &&
                              !offline(s.node.status) &&
                              !(isfinite(selectedSummary.generated) && epoch >= 1700000000 &&
                                selectedSummary.generated > epoch + 30);
    auto& system = out.pages[0];
    text(system.title, sizeof(system.title), "System");
    freshness(system, valid || summary, summary ? summaryFresh : fresh, s.demo,
              summary ? summaryAge : age, summary ? summaryTtl : 15);
    line(system, 0, "%s", s.node.name[0] ? s.node.name : valid && s.host[0] ? s.host :
         s.node.id[0] ? s.node.id : "No selected system");
    char cpu[16], ram[16], temp[16];
    metric(cpu, sizeof(cpu), summary ? selectedSummary.cpu : valid ? s.cpu : NAN, "%");
    metric(ram, sizeof(ram), summary ? selectedSummary.memory :
           valid && isfinite(s.memUsed) && isfinite(s.memTotal) && s.memTotal > 0 ?
           s.memUsed / s.memTotal * 100.0 : NAN, "%");
    metric(temp, sizeof(temp), summary ? selectedSummary.temp : valid ? s.temp : NAN, "C");
    line(system, 1, "CPU %s RAM %s", cpu, ram);
    line(system, 2, "Temp %s", temp);
    line(system, 3, "%s", summary ? "Summary; loading..." : !valid ?
         (net.loading ? "Loading selected node" : "Waiting for readings") : offline(s.node.status) ? "System offline" :
         net.loading ? "Loading selected node" : s.node.status[0] ? s.node.status : "Status unknown");

    auto& alerts = out.pages[1];
    text(alerts.title, sizeof(alerts.title), "Alerts");
    freshness(alerts, valid, fresh, s.demo, age);
    line(alerts, 0, "%s", s.node.name[0] ? s.node.name : "Selected system");
    line(alerts, 1, valid ? "%u active alerts" : "Alerts unavailable", unsigned(s.nAlerts));
    unsigned dest = 2;
    // Show critical/error first, preserving order within each priority.
    for(unsigned priority = 0; priority < 2; ++priority)
        for(unsigned i = 0; valid && i < s.nAlerts && i < MAX_ALERTS && dest < GD_STATUS_ROWS; ++i) {
            const auto& a = s.alerts[i];
            bool severe = !strcmp(a.severity, "critical") || !strcmp(a.severity, "error");
            if(severe == (priority == 0)) line(alerts, dest++, "%s", a.message);
        }
    if(valid && !s.nAlerts) {
        line(alerts, 2, fresh ? "No reported alerts" : "Readings stale");
        bool incomplete = false;
        for(unsigned i = 0; i < s.nSources && i < MAX_SOURCES; ++i)
            if(s.sources[i].enabled && !s.sources[i].ok) incomplete = true;
        line(alerts, 3, incomplete ? "Some sources missing" : "");
    }

    auto& agents = out.pages[2];
    text(agents.title, sizeof(agents.title), "Agents");
    const double oelapsed = o ? uint32_t(now - o->received) / 1000.0 : 0;
    const double oage = o ? envelopeAge(o->generated, o->received, now, epoch) : NAN;
    bool sources[ops::MAX_SOURCES]{};
    unsigned goodSources = 0, staleSources = 0;
    double maxAge = oage, minTtl = 15;
    if(o && o->valid) for(unsigned i = 0; i < o->nSources && i < ops::MAX_SOURCES; ++i) {
        const auto& source = o->sources[i];
        const double sourceAge = timestampAge(source.sampled, o->generated, oelapsed, epoch);
        sources[i] = (onet.connected || o->demo) && isfinite(sourceAge) &&
                     isfinite(source.ttl) && source.ttl >= 1 && sourceAge <= source.ttl &&
                     (!strcmp(source.status, "healthy") || !strcmp(source.status, "degraded"));
        if(sources[i]) ++goodSources; else ++staleSources;
        maxAge = later(maxAge, sourceAge);
        if(isfinite(source.ttl) && source.ttl < minTtl) minTtl = source.ttl;
    }
    const bool ofresh = o && o->valid && goodSources && !staleSources && isfinite(oage) && oage <= 15;
    freshness(agents, o && o->valid && goodSources, ofresh, o && o->demo, maxAge, minTtl);
    unsigned running = 0, queued = 0, blocked = 0, waiting = 0, failed = 0, staleAgents = 0;
    if(o && o->valid) {
        for(unsigned i = 0; i < o->nAgents && i < ops::MAX_AGENTS; ++i) {
            const auto& a = o->agents[i];
            const double aage = timestampAge(a.heartbeat, o->generated, oelapsed, epoch);
            if(a.source >= ops::MAX_SOURCES || !sources[a.source] || !isfinite(aage) ||
               !isfinite(a.ttl) || aage > a.ttl) { ++staleAgents; continue; }
            if(!strcmp(a.state, "running")) ++running;
        }
        for(unsigned i = 0; i < o->nTasks && i < ops::MAX_TASKS; ++i) {
            const auto& t = o->tasks[i];
            if(t.source >= ops::MAX_SOURCES || !sources[t.source]) continue;
            if(!strcmp(t.state, "queued")) ++queued;
            else if(!strcmp(t.state, "blocked")) ++blocked;
            else if(!strcmp(t.state, "waiting")) ++waiting;
            else if(!strcmp(t.state, "failed")) ++failed;
        }
    }
    if(!o || !o->valid || !goodSources) {
        line(agents, 0, "Operations unavailable");
        line(agents, 1, "No fresh sources");
    } else {
        line(agents, 0, "%u running agents", running);
        line(agents, 1, "%u queued %u blocked", queued, blocked);
        line(agents, 2, "%u waiting %u failed", waiting, failed);
        line(agents, 3, "%u stale src/agents", staleSources + staleAgents);
    }
    // Counts come from bounded inventories; never imply a complete fleet count.
    if(o && (o->truncated[1] || o->truncated[2])) line(agents, 3, "Partial inventory");

    auto& printer = out.pages[3];
    text(printer.title, sizeof(printer.title), "Printer");
    const Printer& p = s.printer;
    if(valid && p.present) {
        const double readingAge = sampleAge(p.age, p.updated, s.generated, elapsed, epoch);
        const double pageAge = isfinite(readingAge) ? later(age, readingAge) : NAN;
        const bool klippyReady = !strcmp(p.klippyState, "ready");
        bool pFresh = fresh && isfinite(p.ttl) && isfinite(pageAge) && pageAge <= p.ttl &&
                      !p.error[0] && klippyReady;
        for(unsigned i = 0; i < s.nSources && i < MAX_SOURCES; ++i)
            if(s.sources[i].enabled && !s.sources[i].ok) pFresh = false;
        freshness(printer, valid, pFresh, s.demo, pageAge, p.ttl < 15 ? p.ttl : 15);
        line(printer, 0, "%s", p.name[0] ? p.name : s.node.name);
        metric(temp, sizeof(temp), klippyReady ? p.progress : NAN, "%");
        line(printer, 1, "%s %s", klippyReady ? (p.state[0] ? p.state : "Unknown") :
             (p.klippyState[0] ? p.klippyState : "Unknown"), temp);
        if(klippyReady && isfinite(p.remaining) && p.remaining >= 0 && p.remaining < 604800)
            line(printer, 2, "Remaining %uh %02um", unsigned(p.remaining) / 3600, (unsigned(p.remaining) / 60) % 60);
        else line(printer, 2, "Remaining --");
        line(printer, 3, "%s", p.error[0] ? p.error : p.filename[0] ? p.filename : "No active file");
    } else {
        bool found = false;
        for(unsigned i = 0; i < s.nNodes && i < MAX_NODES; ++i) if(!strcmp(s.nodes[i].type, "klipper")) {
            const auto& n = s.nodes[i]; const auto& v = n.summary;
            const double readingAge = sampleAge(v.age, v.generated, s.generated, elapsed, epoch);
            const double pageAge = isfinite(readingAge) ? later(age, readingAge) : NAN;
            const double ttl = isfinite(v.ttl) && v.ttl < 15 ? v.ttl : 15;
            freshness(printer, valid && v.available, fresh && v.available && isfinite(pageAge) &&
                      pageAge <= ttl && !offline(n.status) && !offline(v.status), s.demo, pageAge, ttl);
            line(printer, 0, "%s", n.name);
            metric(temp, sizeof(temp), v.progress, "%");
            line(printer, 1, "%s %s", v.printState[0] ? v.printState : "Unknown", temp);
            line(printer, 2, "Select on Glimdock");
            line(printer, 3, "for job details");
            found = true; break;
        }
        if(!found) {
            freshness(printer, false, false, s.demo, NAN);
            line(printer, 0, "No printer configured");
            line(printer, 1, "Add a Klipper node");
        }
    }
}
}
