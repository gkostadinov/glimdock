#pragma once
/* Glimdock status protocol v1. C99/C++17, no platform dependencies. */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#define GD_STATUS_PAGES 4u
#define GD_STATUS_ROWS 4u
#define GD_STATUS_TITLE 16u
#define GD_STATUS_TEXT 22u
#define GD_STATUS_VALID 1u
#define GD_STATUS_STALE 2u
#define GD_STATUS_DEMO 4u
#define GD_STATUS_UNKNOWN_AGE UINT16_MAX
#define GD_STATUS_PAGE_BYTES (5u + GD_STATUS_TITLE + GD_STATUS_ROWS * GD_STATUS_TEXT)
#define GD_STATUS_BYTES (8u + GD_STATUS_PAGES * GD_STATUS_PAGE_BYTES)

typedef struct {
    uint8_t flags;
    uint16_t age_seconds;
    uint16_t ttl_seconds;
    char title[GD_STATUS_TITLE];
    char rows[GD_STATUS_ROWS][GD_STATUS_TEXT];
} GdStatusPage;

typedef struct {
    uint32_t sequence;
    GdStatusPage pages[GD_STATUS_PAGES];
} GdStatus;

static inline uint16_t gd_read_u16(const uint8_t* p) {
    return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}
static inline void gd_write_u16(uint8_t* p, uint16_t value) {
    p[0] = (uint8_t)value;
    p[1] = (uint8_t)(value >> 8);
}
static inline bool gd_status_text_valid(const uint8_t* text, size_t size) {
    bool ended = false;
    for(size_t i = 0; i < size; ++i) {
        if(text[i] == 0) ended = true;
        else if(ended || text[i] < 32 || text[i] > 126) return false;
    }
    return ended;
}
/* A failed decode never modifies the caller's currently displayed status. */
static inline bool gd_status_decode(const uint8_t* wire, size_t size, GdStatus* out) {
    if(!wire || !out || size != GD_STATUS_BYTES || wire[0] != 'G' || wire[1] != 'D' ||
       wire[2] != 1 || wire[3] != GD_STATUS_PAGES) return false;
    for(size_t i = 0; i < GD_STATUS_PAGES; ++i) {
        const uint8_t* p = wire + 8 + i * GD_STATUS_PAGE_BYTES;
        if(p[0] & ~(GD_STATUS_VALID | GD_STATUS_STALE | GD_STATUS_DEMO)) return false;
        if(!gd_read_u16(p + 3) || !gd_status_text_valid(p + 5, GD_STATUS_TITLE)) return false;
        for(size_t row = 0; row < GD_STATUS_ROWS; ++row)
            if(!gd_status_text_valid(p + 5 + GD_STATUS_TITLE + row * GD_STATUS_TEXT,
                                     GD_STATUS_TEXT)) return false;
    }
    memset(out, 0, sizeof(*out));
    out->sequence = (uint32_t)wire[4] | ((uint32_t)wire[5] << 8) |
                    ((uint32_t)wire[6] << 16) | ((uint32_t)wire[7] << 24);
    for(size_t i = 0; i < GD_STATUS_PAGES; ++i) {
        const uint8_t* p = wire + 8 + i * GD_STATUS_PAGE_BYTES;
        GdStatusPage* page = &out->pages[i];
        page->flags = p[0];
        page->age_seconds = gd_read_u16(p + 1);
        page->ttl_seconds = gd_read_u16(p + 3);
        memcpy(page->title, p + 5, GD_STATUS_TITLE);
        memcpy(page->rows, p + 5 + GD_STATUS_TITLE, GD_STATUS_ROWS * GD_STATUS_TEXT);
    }
    return true;
}
static inline bool gd_status_encode(const GdStatus* status, uint8_t* wire, size_t capacity) {
    if(!status || !wire || capacity < GD_STATUS_BYTES) return false;
    memset(wire, 0, GD_STATUS_BYTES);
    wire[0] = 'G'; wire[1] = 'D'; wire[2] = 1; wire[3] = GD_STATUS_PAGES;
    for(size_t i = 0; i < 4; ++i) wire[4 + i] = (uint8_t)(status->sequence >> (i * 8));
    for(size_t i = 0; i < GD_STATUS_PAGES; ++i) {
        uint8_t* p = wire + 8 + i * GD_STATUS_PAGE_BYTES;
        const GdStatusPage* page = &status->pages[i];
        p[0] = page->flags;
        gd_write_u16(p + 1, page->age_seconds);
        gd_write_u16(p + 3, page->ttl_seconds);
        memcpy(p + 5, page->title, GD_STATUS_TITLE);
        memcpy(p + 5 + GD_STATUS_TITLE, page->rows, GD_STATUS_ROWS * GD_STATUS_TEXT);
    }
    GdStatus checked;
    return gd_status_decode(wire, GD_STATUS_BYTES, &checked);
}
static inline bool gd_status_page_stale(const GdStatusPage* page, uint32_t elapsed_seconds) {
    return !(page->flags & GD_STATUS_VALID) || (page->flags & GD_STATUS_STALE) ||
           page->age_seconds == GD_STATUS_UNKNOWN_AGE ||
           elapsed_seconds > page->ttl_seconds ||
           (uint32_t)page->age_seconds + elapsed_seconds > page->ttl_seconds;
}
