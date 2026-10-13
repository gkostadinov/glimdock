#!/usr/bin/env python3
"""Compare display timezone conversion with the system IANA database."""
from __future__ import annotations
import argparse
import datetime as dt
from pathlib import Path
import subprocess
import tempfile
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parents[2]
ZONES = ("UTC", "Europe/Sofia", "Europe/London", "Europe/Berlin", "America/New_York", "America/Los_Angeles", "Asia/Tokyo")
DRIVER = r'''
#include "display_clock.h"
#include <iostream>
int main() {
  tm out{};
  if (displayClockTm(0, 0, out)) return 2;
  int64_t epoch; unsigned zone;
  while (std::cin >> epoch >> zone) {
    if (!displayClockTm(time_t(epoch), uint8_t(zone), out)) return 3;
    std::cout << out.tm_year + 1900 << ' ' << out.tm_mon + 1 << ' '
      << out.tm_mday << ' ' << out.tm_hour << ' ' << out.tm_min << ' '
      << out.tm_sec << ' ' << out.tm_wday << ' ' << out.tm_yday << ' '
      << out.tm_isdst << '\n';
  }
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", default="c++")
    args = parser.parse_args()
    zones = [ZoneInfo(name) for name in ZONES]
    instants = set()
    # Daily checks cover leap years and date rollover. Detect changes using the
    # independent IANA database, then test the exact second of each DST boundary.
    for year in range(2024, 2038):
        instant = dt.datetime(year, 1, 1, tzinfo=dt.timezone.utc)
        while instant.year == year:
            instants.update(int((instant + dt.timedelta(hours=h)).timestamp()) for h in (0, 12, 23))
            tomorrow = instant + dt.timedelta(days=1)
            for zone in zones:
                if instant.astimezone(zone).utcoffset() == tomorrow.astimezone(zone).utcoffset():
                    continue
                previous = instant.astimezone(zone).utcoffset()
                for minute in range(1, 1441):
                    point = instant + dt.timedelta(minutes=minute)
                    if point.astimezone(zone).utcoffset() != previous:
                        epoch = int(point.timestamp())
                        instants.update((epoch - 1, epoch, epoch + 1))
                        break
            instant = tomorrow
    cases = [(epoch, zone) for epoch in sorted(instants) for zone in range(len(zones))]
    # Invalid saved preferences fall back to UTC; unsynchronized epoch is tested
    # directly by the C++ driver before any comparisons.
    cases.append((int(dt.datetime(2026, 12, 31, 23, 59, 59, tzinfo=dt.timezone.utc).timestamp()), 255))
    with tempfile.TemporaryDirectory(prefix="glimdock-clock-") as directory:
        source, binary = Path(directory) / "clock.cpp", Path(directory) / "clock"
        source.write_text(DRIVER)
        subprocess.run([args.compiler, "-std=c++17", "-Wall", "-Wextra", "-Werror", "-I" + str(ROOT / "firmware/src"), str(source), "-o", str(binary)], check=True)
        result = subprocess.run([str(binary)], input="".join(f"{epoch} {zone}\n" for epoch, zone in cases), text=True, capture_output=True, check=True)
    lines = result.stdout.splitlines()
    assert len(lines) == len(cases)
    for (epoch, zone), line in zip(cases, lines):
        expected = dt.datetime.fromtimestamp(epoch, zones[zone if zone < len(zones) else 0])
        parts = (expected.year, expected.month, expected.day, expected.hour, expected.minute, expected.second, (expected.weekday() + 1) % 7, expected.timetuple().tm_yday - 1, int(bool(expected.dst())))
        actual = tuple(map(int, line.split()))
        assert actual == parts, f"zone={zone} epoch={epoch}: {actual} != {parts}"
    print(f"Passed {len(cases)} timezone comparisons against IANA tzdb (2024–2037), including exact DST boundaries, leap years, midnight rollover and invalid preferences.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
