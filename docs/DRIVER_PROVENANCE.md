# Display and touch driver provenance

Reviewed 8 October 2026. This records the implementation used by the
Waveshare ESP32-S3-Touch-LCD-2.8 **V1** prototype. It does not cover the V2
board's different touch controller.

## Release scope and decision

`firmware/src/board.cpp`, `board.h` and `touch_state.h` are Glimdock's small
Arduino/LVGL hardware integration. The source release includes these original
implementations under the project's software license. It does **not** include
BabyStory source files, vendor example applications, audio libraries, vendor
schematics or the manufacturer's board STEP model.

The original development reference was the owner's local BabyStory V1 example,
in particular `src/Display_ST7789.cpp`, `src/Touch_CST328.cpp` and
`src/Touch_CST328.h`. No license header was found in those three files, and no
project license was found in that example. No license is inferred for that
example and no permission to redistribute its source is claimed.

The released integration does not reproduce those example files, function
bodies, touch-data structures, UI, logging, audio or sleep implementation. The
shared material is the board's pin assignment, required peripheral transactions,
controller register values, report layout and panel tuning data. These are
hardware interoperability facts, rather than a redistribution of the example's
implementation. The line at the top of `board.cpp` saying the register sequence
and protocol were “adapted” records this technical reference; it is not a claim
that the example source was relicensed.

## What was compared

| Glimdock behavior | Reference and comparison |
| --- | --- |
| ST7789 SPI pins: SCLK 40, MOSI 45, CS 42, DC 41, reset 39; backlight 5 | Board wiring used in the V1 example. Glimdock implements its own SPI command, byte-transfer and register helpers. It does not call or import `LCD_WriteCommand`, `LCD_WriteData`, `LCD_SetCursor` or `LCD_addWindow`. |
| ST7789 startup register values, including power settings and positive/negative gamma data | Panel configuration data from the V1 example. Glimdock emits it using its own register helper; reset timing, landscape MADCTL, RGB565 byte order, partial draw buffer and LVGL 9 display integration are project-specific. |
| CST328 I2C address `0x1a`; SDA 1, SCL 3, IRQ 4, reset 2 | Board wiring and controller addressing. Glimdock's wrappers bound transfer sizes, check received byte counts and propagate errors instead of importing the example's wrappers. |
| Touch reset: high for 50 ms, low for 5 ms, high for 50 ms | V1 electrical reset sequence recorded from the BabyStory board example. The reviewed ESP-IDF component uses different 10 ms reset delays; it is not a source for these V1 timings. |
| `0xd101` identification mode, a 24-byte read at `0xd1f4`, `0xcaca` signature, return through `0xd109` | The debug mode, read register and normal-mode register are corroborated by the official Waveshare component. The V1 `0xcaca` signature check comes from the BabyStory board example. Glimdock does not import the example's diagnostic dump implementation. |
| Read count at `0xd005`, read the report at `0xd000`, acknowledge with zero at `0xd005` | Controller protocol, corroborated by the official Waveshare component. Glimdock deliberately retains this V1 acknowledgement behavior. |
| First-point packed X/Y decoding | Controller packet layout. `TouchState` independently decodes one bounded contact from a 27-byte report. It does not import the example's multi-contact loop or `CST328_Touch` structure. |
| Pointer hold, release grace, report-loss cancellation, wake-touch consumption and display/touch rotation together | Original Glimdock behavior covered by the touch-state/native UI tests. |
| GPIO 7 power latch asserted before startup; PSRAM pool and LVGL callbacks | Original Glimdock integration. The latch is never pulsed low during initialization. |

## Permissive upstream corroboration

Waveshare publishes an ESP-IDF CST328 component with explicit Apache-2.0
licensing and these copyright notices:

- `2015-2024 Espressif Systems (Shanghai) CO LTD`
- `2025 Waveshare`

The reviewed component is
[`esp_lcd_touch_cst328.c`](https://github.com/waveshareteam/Waveshare-ESP32-components/blob/30d0ac3b8b6b27ebd402c5852819cbe0dab92749/display/touch/esp_lcd_touch_cst328/esp_lcd_touch_cst328.c)
at commit `30d0ac3b8b6b27ebd402c5852819cbe0dab92749`. Its file SHA-256 is
`a11bec982fd345d04e3ce4d6a5c4973cf668e3c5442422cb5ad531d9cba816d0`.
The component's exact Apache license is preserved in
[`LICENSES/Waveshare-CST328-Apache-2.0.txt`](../LICENSES/Waveshare-CST328-Apache-2.0.txt)
and its credit is retained in [the notices](../THIRD_PARTY_NOTICES.md).

That component was reviewed as a protocol corroboration source; it is not linked
or bundled in this firmware. Glimdock uses the Arduino `Wire1` API rather than
that component's ESP-IDF panel-I/O API. The attribution does not claim a license
for the separately supplied BabyStory files.

## Review fingerprints

These identify the reviewed source, not a promise that later versions are
unchanged. Re-review changes that introduce an upstream implementation.

| File | SHA-256 |
| --- | --- |
| Glimdock `firmware/src/board.cpp` | `7b9d0638c5b43f7bb90257468fe3706d9c3fc4ace4472e370b89f1b5da4e559b` |
| Glimdock `firmware/src/touch_state.h` | `53ae15847b7ed10e8cff9af18931305c6a548044a5887aba5f126e35260212ec` |
| Reference `Display_ST7789.cpp` | `5c42b74b9f0a7ac4f818d6ab6087bf00795fad01dbddffbeeb10647eb29709d8` |
| Reference `Touch_CST328.cpp` | `ee45754f88aabbb7b20dc6fbebd0de0926874349f12c00da48c5a57357f51026` |
| Reference `Touch_CST328.h` | `5f7f546463047f3a646ead8638256d5359858d10367636132158df05134b6e8f` |

## Distribution path

The source release can retain the current working driver: keep this provenance,
the project software license and dependency notices, and exclude the unlicensed
example source and manufacturer reference CAD. No electrical behavior needs to
change merely to prepare the source release.

A future port may use an original implementation of the documented controller
protocol or a properly attributed permissive component. If upstream code is
actually incorporated, retain that code's notices and license in the source
file, record modifications, and update the dependency inventory. A public
prebuilt ESP32 firmware release also needs the Arduino LGPL corresponding-source
and rebuild/relink materials described in [the notices](../THIRD_PARTY_NOTICES.md).
