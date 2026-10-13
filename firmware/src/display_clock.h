#pragma once
#include <stdint.h>
#include <time.h>

// Display-only timezone conversion leaves collector timestamps and freshness in
// UTC. Modern rules match IANA tzdb (Europe since 1996, US since 2007):
// https://data.iana.org/time-zones/tzdb/europe
// https://data.iana.org/time-zones/tzdb/northamerica
// Use explicit rules because browser libc localtime follows the browser's zone.
constexpr uint8_t DISPLAY_CLOCK_ZONE_COUNT = 7;

inline const char *displayClockZoneName(uint8_t zone) {
  static const char *const names[] = {"UTC", "Sofia", "London", "Berlin", "New York", "Los Angeles", "Tokyo"};
  return names[zone < DISPLAY_CLOCK_ZONE_COUNT ? zone : 0];
}

inline const char *displayClockZoneAbbreviation(uint8_t zone, bool daylight) {
  static const char *const standard[] = {"UTC", "EET", "GMT", "CET", "EST", "PST", "JST"};
  static const char *const summer[] = {"UTC", "EEST", "BST", "CEST", "EDT", "PDT", "JST"};
  return (daylight ? summer : standard)[zone < DISPLAY_CLOCK_ZONE_COUNT ? zone : 0];
}

namespace display_clock_detail {
inline int dayOfYear(int year, int month, int day) {
  static const int before[] = {0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334};
  bool leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
  return before[month - 1] + day - 1 + (leap && month > 2 ? 1 : 0);
}

inline int sunday(int year, int janWeekday, int month, int ordinal) {
  int first = dayOfYear(year, month, 1);
  if (ordinal == -1) {
    // Only March and October use a last-Sunday rule; both have 31 days.
    int last = first + 30;
    return last - (janWeekday + last) % 7;
  }
  return first + (7 - (janWeekday + first) % 7) % 7 + (ordinal - 1) * 7;
}
}

inline bool displayClockTm(time_t now, uint8_t zone, tm &out) {
  out = tm{};
  // An ESP32 starts at the Unix epoch until NTP has synchronized.
  if (int64_t(now) < 1700000000) return false;
  tm utc{};
  if (!gmtime_r(&now, &utc)) return false;
  if (zone >= DISPLAY_CLOCK_ZONE_COUNT) zone = 0;
  static const int8_t hours[] = {0, 2, 0, 1, -5, -8, 9};
  int offset = hours[zone] * 3600;
  int year = utc.tm_year + 1900;
  int janWeekday = (utc.tm_wday - utc.tm_yday % 7 + 7) % 7;
  int inYear = utc.tm_yday * 86400 + utc.tm_hour * 3600 + utc.tm_min * 60 + utc.tm_sec;
  bool daylight = false;
  if (zone >= 1 && zone <= 3) {
    int start = display_clock_detail::sunday(year, janWeekday, 3, -1) * 86400 + 3600;
    int end = display_clock_detail::sunday(year, janWeekday, 10, -1) * 86400 + 3600;
    daylight = inYear >= start && inYear < end;
  } else if (zone == 4 || zone == 5) {
    // Spring transition is 02:00 standard time, fall is 02:00 daylight time.
    int start = display_clock_detail::sunday(year, janWeekday, 3, 2) * 86400 + 7200 - offset;
    int end = display_clock_detail::sunday(year, janWeekday, 11, 1) * 86400 + 7200 - offset - 3600;
    daylight = inYear >= start && inYear < end;
  }
  if (daylight) offset += 3600;
  time_t local = time_t(int64_t(now) + offset);
  if (!gmtime_r(&local, &out)) return false;
  out.tm_isdst = daylight ? 1 : 0;
  return true;
}
