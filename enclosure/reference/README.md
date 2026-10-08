# External fitting reference

Manufacturer geometry is not redistributed. The CAD validation scripts expect
`esp32-s3-touch-lcd-2_8.stp` in this directory. Obtain the correct V1 drawing from
the official resources page and verify its revision before use:
https://docs.waveshare.com/ESP32-S3-Touch-LCD-2.8/Resources-And-Documents

The original printed case/keeper geometry is defined by `build.py`, `design.json`
and `tools/board_keepers.py`; r7 internal USB walls and mounting repair are in `tools/usb_baffle.py` and
`tools/repair_mount.py`. See BUILDING.md for the baseline-generation sequence.
No manufacturer PCB design, raw drawing, image, user slicer profile or G-code is
licensed or included by this source release. Physical fit is still a release gate.
