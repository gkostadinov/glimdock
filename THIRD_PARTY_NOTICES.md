# Third-party notices

Reviewed 8 October 2026; native device-agent inventory added 9 October 2026.
Original Glimdock software uses the root project
license; third-party components retain their own terms. The original enclosure
is separately scoped by `LICENSE_POLICY.md`. A dependency license does not
relicense either the Waveshare module or its manufacturer reference CAD.

## Firmware dependencies

The supported V1 firmware pins PlatformIO Espressif32 `6.13.0`, LVGL `9.3.0`
and ArduinoJson `7.4.2`. The installed Arduino framework reports
`3.20017.241212+sha.dcc1105b`, core version `2.0.17`, and its bundled SDK reports
ESP-IDF `4.4.7`. The reviewed Arduino commit is
[`dcc1105b0cf1322a437b354c336f2abf72b7e512`](https://github.com/espressif/arduino-esp32/tree/dcc1105b0cf1322a437b354c336f2abf72b7e512).
The ESP-IDF license/component inventory is anchored to tag `v4.4.7`, commit
[`38eeba213aa695aabfd6d89aa9f5078dbe5a94c3`](https://github.com/espressif/esp-idf/tree/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3).

| Component | License and scope | Retained material |
| --- | --- | --- |
| Arduino-ESP32 core | LGPL-2.1-or-later, as declared by the installed package; individual files can have other notices | Exact upstream license; libb64's installed public-domain dedication. Do not label the complete linked firmware MIT-only. |
| ESP-IDF | Apache-2.0 by default; component exceptions apply | Exact top-level license plus component/submodule notices listed below. |
| ESP-IDF FreeRTOS | MIT | Exact v4.4.7 FreeRTOS license and installed Amazon copyright/license header. |
| ESP-IDF newlib | Multiple permissive component terms | Exact `COPYING.NEWLIB`; no blanket single-license substitution. |
| ESP-IDF lwIP | BSD-style terms | Exact submodule `COPYING`. |
| ESP-IDF wpa_supplicant | BSD terms; upstream COPYING explains the former BSD/GPL choice | Exact COPYING and README with current terms. |
| ESP-IDF Mbed TLS | Apache-2.0 OR GPL-2.0-or-later | Full upstream dual-license text retained. Apache-2.0 is the permissive option; neither the project MIT license nor this inventory changes it. |
| ESP-IDF Wi-Fi and PHY precompiled libraries | Their respective repositories provide Apache-2.0 texts | Exact submodule licenses retained. They are not original Glimdock source. |
| Other ESP-IDF components | Component-specific terms | cJSON, TinyUSB, SPIFFS, micro-ecc, protobuf-c, libsodium, TinyCBOR, Expat, Asio, argtable3, linenoise, nghttp and FreeModbus texts retained as SDK inventory. Inclusion here does not assert every component is linked into the final image. |
| LVGL 9.3.0 | MIT; embedded helpers keep their own notices | Installed MIT, TLSF and sprintf notices copied byte-for-byte. |
| ArduinoJson 7.4.2 | MIT | Installed copyright/license copied byte-for-byte. |
| Montserrat glyphs in LVGL | SIL OFL 1.1 | Installed Montserrat notice. Enabled sizes are 8, 10, 12, 14, 18, 24, 28 and 32 px. |
| Font Awesome font glyphs in LVGL's Montserrat tables | SIL OFL 1.1 for font files | Full installed Font Awesome notice retained; it separately describes CC BY 4.0 for SVG/JS icons and MIT for code. No SVG/JS icon license is silently substituted for the embedded font glyph license. |

LVGL's bundled optional decoders and font systems are not enabled by Glimdock's
configuration. Their complete dependency trees are not redistributed in this
source release. If enabling them or vendoring LVGL, preserve the corresponding
upstream notices, including any DejaVu/Source Han font data and decoder licenses.

## Board protocol reference

The V1 display/touch integration uses hardware interoperability facts from the
owner's BabyStory example; it does not bundle that example's unlicensed source.
See [`docs/DRIVER_PROVENANCE.md`](docs/DRIVER_PROVENANCE.md) for the implementation
comparison and source fingerprints.

Protocol corroboration credit is retained for Waveshare's Apache-2.0 CST328
component: **2015-2024 Espressif Systems (Shanghai) CO LTD; 2025 Waveshare**.
The reviewed component at commit `30d0ac3b8b6b27ebd402c5852819cbe0dab92749` is not
linked or bundled. Its exact license is included for reference; this attribution
does not grant a license to BabyStory source.

## Branding and website fonts

Space Grotesk: **Copyright 2020 The Space Grotesk Project Authors**
([project](https://github.com/floriankarsten/space-grotesk)), SIL OFL 1.1.
The wordmark SVG contains outlines made from Space Grotesk; its OFL notice is
retained in both `branding/Space-Grotesk-OFL.txt` and `LICENSES/`.
The geometric Glimdock mark and firmware LVGL drawing are original project work.
Font notices do not grant product-name or trademark rights.

The separate commercial website also uses DM Sans under SIL OFL 1.1. Its exact
notice is retained here for website redistribution. This font is not linked into
the ESP32 firmware or Linux collector.

## Static Linux collector runtime

The Linux releases use Rust `1.98.1`, commit
`48a229ceaefd4985c50990b14116b6d856af0985`, with the `x86_64-unknown-linux-musl`
and `aarch64-unknown-linux-musl` target runtimes. Cargo's application dependency
inventory does not include the standard library or the target's self-contained
C runtime, so the following notices accompany it.

| Embedded runtime | Terms and retained notices |
| --- | --- |
| Rust standard library, core and alloc | Apache-2.0 OR MIT by default, with the exact toolchain copyright inventory and its exceptions retained. These include Unicode-3.0 data. |
| Rust compiler-builtins | The exact upstream notice declares **MIT AND Apache-2.0 WITH LLVM-exception** for compiler-builtins, with other original source available under its stated alternatives. The full notice is retained; this is not simplified to MIT-only. |
| Rust libm code | Exact upstream license, contributor attribution and musl/CORE-MATH copyright notices retained. |
| LLVM compiler-rt and libunwind | Exact notices from Rust's LLVM submodule commit `52ed14fcd56afc30f9cccd8ca8ce237c2eef7e04`; Apache-2.0 with LLVM exceptions and the additional notices in those files. |
| musl libc and CRT | The self-contained target libc reports musl **1.2.5**. Its full MIT copyright file, contributor list, third-party exceptions and special public-header/CRT attribution permission are retained. Additional verbatim source notices cover math, regex, architecture string routines, crypt and sorting exceptions. |

`LICENSES/Rust-1.98.1-Standard-Library-COPYRIGHT.txt` is the **exact installed
`COPYRIGHT-library.html` document**, with its original HTML markup preserved
byte-for-byte despite the `.txt` extension. It is the named `1.98.1` toolchain's
document, not a substitute from a different `stable` installation. It contains
the complete supplied library copyright/license inventory, including the Unicode
license and dependencies for other platforms; retaining that broad inventory
does not claim every listed dependency is linked into these Linux executables.

The target's `crtbegin.o`, `crtbeginS.o`, `crtend.o` and `crtendS.o` come from
LLVM compiler-rt, as shown by the pinned Rust
[`CrtBeginEnd` build step](https://github.com/rust-lang/rust/blob/48a229ceaefd4985c50990b14116b6d856af0985/src/bootstrap/src/core/build_steps/llvm.rs#L1609),
so its runtime license is retained as well as musl's CRT notice. A GCC compiler
version string inside those objects identifies the build compiler; it does not
change the source provenance into GCC runtime code.

The standalone Rust and compiler-builtins/libm texts were fetched from the
compiler's exact commit. The compiler-builtins and libunwind texts also match
the installed source copies by SHA-256. The musl exception notice preserves
46 distinct copyright/license comments verbatim from 121 source files, with
source-path labels and separators added. Its source archive is the official
`musl-1.2.5.tar.gz`, SHA-256
`a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4`.

The Rust compiler, its LLVM build tool dependencies and Zig are build tools,
not software automatically embedded simply because they compile or link the
collector. The binary packages do not redistribute those tools. The retained
LLVM notices above apply to runtime code, rather than asserting a license for
the whole toolchain. Changing the compiler, target runtime or linker-supplied
runtime requires updating this inventory.

## Native device agent

`device-agent-rs/` builds the native Rust `glimdock-agent` for OS telemetry and
SNMP/JSON device adapters. Its dependency graph is separate from the Linux hub's
graph. [LICENSES/Rust-Device-Agent/inventory.json](LICENSES/Rust-Device-Agent/inventory.json)
records the exact locked versions, declared SPDX expressions, registry checksums,
target membership and hashes of retained notices for Linux musl `x86_64` and
`aarch64`, macOS `x86_64` and `aarch64`, and Windows `x86_64` MSVC. Normal and
build dependencies are retained conservatively; dev-only crates are excluded.
See its [regeneration instructions](LICENSES/Rust-Device-Agent/README.md).

Native OS reads use `sysinfo` and `battery`; authenticated HTTP, JSON mapping and
HTTPS use the locked Rust networking/serialization dependencies, including
Rustls and ring. These crates keep their individual licenses and copyright
notices. Platform-specific crates appear only in their applicable target graphs.
The original software's MIT license does not replace those terms.

The published `objc2-core-foundation` and `objc2-io-kit` 0.3.2 tarballs omit the
repository's licensing document. Their published Cargo VCS metadata pins
`madsmtm/objc2` commit `7b1abfd750a2cacaea71d6a56ecfb83cb7de560b`; the exact
[upstream licensing document](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md)
is retained for each crate with its source URL, commit and content hash. It
states their alternative Zlib/Apache-2.0/MIT licenses and includes the upstream
Apple SDK licensing note. The source package does not redistribute macOS
frameworks, Windows system DLLs or SDKs. Toolchain/runtime notices above remain
separate from application Cargo inventories; distributed native binaries need
the notices applicable to their actual toolchain and target runtime.

SNMP mode invokes a separately installed Net-SNMP `snmpget` executable. Net-SNMP
is neither linked into `glimdock-agent` nor bundled by the source export or CI
artifacts. Its [official license](https://www.net-snmp.org/about/license.html)
contains multiple component copyright notices and BSD-style terms; a separate
Net-SNMP distribution retains the notices supplied with that exact version.

## Source and binary distribution

The embedded collector web interface uses `esptool-js` 0.7.0 (Apache-2.0),
`@noble/hashes` 2.4.0 (MIT), and the locked esptool-js dependency `pako`
(MIT AND Zlib) for browser USB flashing, image verification and compression.
Exact license texts and the Zlib source notice are served in
`collector-web/assets/licenses/`; `collector-web/package-lock.json` records
the exact package versions and integrity checksums. The local Space Grotesk
font retains its SIL Open Font License in `assets/Space-Grotesk-OFL.txt`.
The actual firmware WebAssembly renderer retains its LVGL, ArduinoJson and
font notices in `collector-web/emulator/`. Emscripten and esbuild are build
tools; their presence does not replace the licenses of bundled runtime code.

The public source package excludes personalized firmware, private configuration,
installed dependency directories and the manufacturer's reference STEP file.
Build instructions fetch the pinned dependencies, whose terms remain applicable.

The supplied Linux collector release has a separate Cargo-lockfile inventory and
crate notices. Its notices and exact corresponding source travel with each
binary archive; this firmware dependency inventory is not a substitute for that
Linux inventory.

The general source export includes no personalized ESP32 firmware binary. The
separate collector web firmware bundle is built with
`tools/build-web-firmware.py` and accompanied by its
`firmware/source-relink.tar.gz`. That archive retains the exact installed
Arduino core/library sources and selected board variant, LVGL and ArduinoJson
sources/notices, original application source/object files, library archives,
actual compile/link commands, linker map, package versions and SDK link inventory.
Its rebuild helper uses an isolated pinned PlatformIO package cache and can
relink unchanged application objects with modified Arduino sources. The SDK
and toolchain packages are fetched at the recorded versions; their applicable
notices are included. No private pairing defaults or runtime data enter this
public bundle. When
redistributing a prebuilt firmware image or a finished flashed product, retain
these notices and accompany it with the actual corresponding LGPL library
source, any modifications, and application/build materials sufficient to rebuild
and relink with a modified library. The exact installed package can differ from
the upstream core tag, so a tag URL alone is not a complete corresponding-source
package. Preserve the recipient's modification and reverse-engineering rights
and the hardware's reflash path. See the
[Arduino-ESP32 LGPL text](LICENSES/Arduino-ESP32-2.0.17-LGPL-2.1.md), especially
section 6, and [Arduino's product guidance](https://support.arduino.cc/hc/en-us/articles/4415094490770-Licensing-for-products-based-on-Arduino).

The SDK texts below record the component exceptions reviewed for this source
release. A public firmware binary additionally needs a linked-component
inventory from its exact build, including compiler-runtime notices and any
enabled optional libraries. Build tools themselves are not redistributed by
this source package.

## Exact retained texts and fingerprints

Local dependency notices were copied byte-for-byte from the installed versions;
the FreeRTOS header notice is the exact initial comment from installed `task.h`.
Other texts were fetched from the pinned upstream revisions linked below.
The Rust standard-library inventory keeps its original HTML markup, and the
musl exception notice consists of exact source comments with path labels added.
The CERN-OHL-P text comes from the official CERN OHL link on the
[Open Hardware Repository licensing page](https://ohwr.org/licences/); it is the
license text used for Glimdock's original enclosure, not a license for the board.
SHA-256 values below verify the included text, rather than a whole component or
binary. The complete terms in each file prevail over the summaries above.

| Included text | SHA-256 | Source |
| --- | --- | --- |
| [Arduino-ESP32-2.0.17-LGPL-2.1.md](LICENSES/Arduino-ESP32-2.0.17-LGPL-2.1.md) | `994ce1902a23c74519b5d118c6bbb657edabce91dd6b7cc825d86bb8f5c9780f` | [upstream](https://github.com/espressif/arduino-esp32/blob/dcc1105b0cf1322a437b354c336f2abf72b7e512/LICENSE.md) |
| [Arduino-ESP32-libb64-Public-Domain.txt](LICENSES/Arduino-ESP32-libb64-Public-Domain.txt) | `834b7afa1b3c40289a3be775d3625016be1c0d7ea7a4a26c1eb207f53dc961d8` | [upstream](https://github.com/espressif/arduino-esp32/blob/dcc1105b0cf1322a437b354c336f2abf72b7e512/cores/esp32/libb64/LICENSE) |
| [ArduinoJson-7.4.2-MIT.txt](LICENSES/ArduinoJson-7.4.2-MIT.txt) | `ebba0906d6c8b3daa8a1b59acb2d3a416485dd77fc10f6dd4c9ed2cbe467ee2d` | [upstream](https://github.com/bblanchon/ArduinoJson/blob/v7.4.2/LICENSE.txt) |
| [CERN-OHL-P-2.0.txt](LICENSES/CERN-OHL-P-2.0.txt) | `6e471d647db50527d0c75ee5f1a55a74012ab177c73282bef0bdc7f6562b7ded` | [upstream](https://gitlab.com/ohwr/project/cernohl/-/wikis/uploads/3eff4154d05e7a0459f3ddbf0674cae4/cern_ohl_p_v2.txt) |
| [DM-Sans-OFL-1.1.txt](LICENSES/DM-Sans-OFL-1.1.txt) | `9af36190332437f5ecd09974de43c1f7c77a310a996cdd8ceb25628b458840e1` | [upstream](https://github.com/googlefonts/dm-fonts/blob/main/OFL.txt) |
| [ESP-IDF-4.4.7-Apache-2.0.txt](LICENSES/ESP-IDF-4.4.7-Apache-2.0.txt) | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/LICENSE) |
| [ESP-IDF-4.4.7-Asio-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-Asio-LICENSE.txt) | `c9bff75738922193e67fa726fa225535870d2aa1059f91452c411736284ad566` | [upstream](https://github.com/espressif/asio/blob/f31694c9f1746ba189a4bcae2e34db15135ddb22/asio/LICENSE_1_0.txt) |
| [ESP-IDF-4.4.7-Expat-COPYING.txt](LICENSES/ESP-IDF-4.4.7-Expat-COPYING.txt) | `122f2c27000472a201d337b9b31f7eb2b52d091b02857061a8880371612d9534` | [upstream](https://github.com/libexpat/libexpat/blob/454c6105bc2d0ea2521b8f8f7a5161c2abd8c386/expat/COPYING) |
| [ESP-IDF-4.4.7-FreeRTOS-MIT.md](LICENSES/ESP-IDF-4.4.7-FreeRTOS-MIT.md) | `508a77d2e7b51d98adeed32648ad124b7b30241a8e70b2e72c99f92d8e5874d1` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/freertos/LICENSE.md) |
| [ESP-IDF-4.4.7-FreeRTOS-header-NOTICE.txt](LICENSES/ESP-IDF-4.4.7-FreeRTOS-header-NOTICE.txt) | `a1bd102a542acdb8bdd1b17607891c1b3247273de438cda97c6ce26230b6bfee` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/freertos/include/freertos/task.h) |
| [ESP-IDF-4.4.7-Mbed-TLS-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-Mbed-TLS-LICENSE.txt) | `9b405ef4c89342f5eae1dd828882f931747f71001cfba7d114801039b52ad09b` | [upstream](https://github.com/espressif/mbedtls/blob/2b8e772fc1cb0732cda3bae7d1e9d6f4cfaf63d9/LICENSE) |
| [ESP-IDF-4.4.7-Newlib-COPYING.txt](LICENSES/ESP-IDF-4.4.7-Newlib-COPYING.txt) | `0681089a556e93791da82718d68011ba452de245f7f59c3846936304756ac0c0` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/newlib/COPYING.NEWLIB) |
| [ESP-IDF-4.4.7-PHY-Binary-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-PHY-Binary-LICENSE.txt) | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | [upstream](https://github.com/espressif/esp-phy-lib/blob/dcfdccf6cc2fc02d0886624b7998c890d1a19b28/LICENSE) |
| [ESP-IDF-4.4.7-SPIFFS-MIT.txt](LICENSES/ESP-IDF-4.4.7-SPIFFS-MIT.txt) | `d19257540156a51ed50c2378e7159066415d8bbbe147779f8deb0d3e73f1b0a7` | [upstream](https://github.com/pellepl/spiffs/blob/0dbb3f71c5f6fae3747a9d935372773762baf852/LICENSE) |
| [ESP-IDF-4.4.7-TinyCBOR-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-TinyCBOR-LICENSE.txt) | `3c6ba0b5bfa7830505301ffb336a17b0748e0d61c4d34216e9dc98f10e40395e` | [upstream](https://github.com/intel/tinycbor/blob/7c349dbb6b8d76db39383b226d3ebdf59b8ab37d/LICENSE) |
| [ESP-IDF-4.4.7-TinyUSB-MIT.txt](LICENSES/ESP-IDF-4.4.7-TinyUSB-MIT.txt) | `b171720e8a442e7a3957d83c62cd3299dbb29da3db534cc626f9dded0de2ca44` | [upstream](https://github.com/espressif/tinyusb/blob/c4badd394eda18199c0196ed0be1e2d635f0a5f6/LICENSE) |
| [ESP-IDF-4.4.7-WiFi-Binary-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-WiFi-Binary-LICENSE.txt) | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | [upstream](https://github.com/espressif/esp32-wifi-lib/blob/f2aae4d44ec7908013066e69d29b9948846c335c/LICENSE) |
| [ESP-IDF-4.4.7-argtable3-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-argtable3-LICENSE.txt) | `a97df648b7db77f9f6f5e1ef5fe44eb6b154ecb8ae26ad39c92ea44201cce45c` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/console/argtable3/LICENSE) |
| [ESP-IDF-4.4.7-cJSON-MIT.txt](LICENSES/ESP-IDF-4.4.7-cJSON-MIT.txt) | `a36dda207c36db5818729c54e7ad4e8b0c6fba847491ba64f372c1a2037b6d5c` | [upstream](https://github.com/DaveGamble/cJSON/blob/87d8f0961a01bf09bef98ff89bae9fdec42181ee/LICENSE) |
| [ESP-IDF-4.4.7-freemodbus-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-freemodbus-LICENSE.txt) | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/freemodbus/LICENSE) |
| [ESP-IDF-4.4.7-libsodium-ISC.txt](LICENSES/ESP-IDF-4.4.7-libsodium-ISC.txt) | `dea1855c9809f3faf22aa4a1fba20ec8af5a5587f23115012e5b98279cedc4af` | [upstream](https://github.com/jedisct1/libsodium/blob/4f5e89fa84ce1d178a6765b8b46f2b6f91216677/LICENSE) |
| [ESP-IDF-4.4.7-linenoise-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-linenoise-LICENSE.txt) | `efe1692aa5b869edaa0ac88f95eb0276c1125304ffc636a299db3ebbab47f62b` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/console/linenoise/LICENSE) |
| [ESP-IDF-4.4.7-lwIP-COPYING.txt](LICENSES/ESP-IDF-4.4.7-lwIP-COPYING.txt) | `8fb15ebdb19eb669e1d37fcd8e57a44c477fcc0e93a9ab0d181760965f94d5ed` | [upstream](https://github.com/espressif/esp-lwip/blob/a45be9e438f6cf9c54ec150581819c3b95d5af6b/COPYING) |
| [ESP-IDF-4.4.7-micro-ecc-BSD.txt](LICENSES/ESP-IDF-4.4.7-micro-ecc-BSD.txt) | `ffd8b033d2df7568c25a98866bd92b1656f7da82d7f813c0b2eb85ec36611193` | [upstream](https://github.com/kmackay/micro-ecc/blob/24c60e243580c7868f4334a1ba3123481fe1aa48/LICENSE.txt) |
| [ESP-IDF-4.4.7-nghttp-COPYING.txt](LICENSES/ESP-IDF-4.4.7-nghttp-COPYING.txt) | `6b94f3abc1aabd0c72a7c7d92a77f79dda7c8a0cb3df839a97890b4116a2de2a` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/nghttp/COPYING) |
| [ESP-IDF-4.4.7-nghttp-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-nghttp-LICENSE.txt) | `f0518afc0a555337bac2f0fb37d33cdee6111983da69aaa504a1876beeddfde0` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/nghttp/LICENSE) |
| [ESP-IDF-4.4.7-protobuf-c-LICENSE.txt](LICENSES/ESP-IDF-4.4.7-protobuf-c-LICENSE.txt) | `b8999cb392cc5bbe8cd679de59584ad8d2f26033123e76f1d662fa14b9d4f287` | [upstream](https://github.com/protobuf-c/protobuf-c/blob/abc67a11c6db271bedbb9f58be85d6f4e2ea8389/LICENSE) |
| [ESP-IDF-4.4.7-wpa-supplicant-COPYING.txt](LICENSES/ESP-IDF-4.4.7-wpa-supplicant-COPYING.txt) | `e9f84e02bb9ba965464e21604014f13d0f3c9299a1b5f7882574a9b4a261abe7` | [upstream](https://github.com/espressif/esp-idf/blob/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/wpa_supplicant/COPYING) |
| [ESP-IDF-4.4.7-wpa-supplicant-README.txt](LICENSES/ESP-IDF-4.4.7-wpa-supplicant-README.txt) | `17ccbcf4b17a910795292fb4a95f4742d2ecda37d06fcf20d60ffbd70cfbc609` | [upstream](https://raw.githubusercontent.com/espressif/esp-idf/38eeba213aa695aabfd6d89aa9f5078dbe5a94c3/components/wpa_supplicant/README) |
| [Font-Awesome-5-NOTICES.txt](LICENSES/Font-Awesome-5-NOTICES.txt) | `de784a808496d49f5f80dd9d1d6e9a51cff712cfbc339e7c25155631ae3cdb5d` | [upstream](https://github.com/lvgl/lvgl/blob/v9.3.0/scripts/built_in_font/font_license/FontAwesome5/LICENSE.txt) |
| [LVGL-9.3.0-MIT.txt](LICENSES/LVGL-9.3.0-MIT.txt) | `27a80bd36832ab42d35ad60c08b2b230a807a9bc0d58e94ec1531543dc49cbe8` | [upstream](https://github.com/lvgl/lvgl/blob/v9.3.0/LICENCE.txt) |
| [LVGL-9.3.0-SPRINTF.txt](LICENSES/LVGL-9.3.0-SPRINTF.txt) | `ec49d3c5a052dba6392f3e5e6d51134d102624c19afab6b3dd9ff8921d85403d` | [upstream](https://github.com/lvgl/lvgl/blob/v9.3.0/src/stdlib/builtin/LICENSE_SPRINTF.txt) |
| [LVGL-9.3.0-TLSF.txt](LICENSES/LVGL-9.3.0-TLSF.txt) | `2b32f144050a1236599765ed1ca2c88d5b99e8e3364eadbb547ab0fbc4b01220` | [upstream](https://github.com/lvgl/lvgl/blob/v9.3.0/src/stdlib/builtin/LICENSE_TLSF.txt) |
| [Montserrat-OFL-1.1.txt](LICENSES/Montserrat-OFL-1.1.txt) | `41f82bb4d24b304f30f7136bc47abdd083782e4265c984160f5649d1e78ea49c` | [upstream](https://github.com/lvgl/lvgl/blob/v9.3.0/scripts/built_in_font/font_license/Montserrat/OFL.txt) |
| [Musl-1.2.5-COPYRIGHT.txt](LICENSES/Musl-1.2.5-COPYRIGHT.txt) | `f9bc4423732350eb0b3f7ed7e91d530298476f8fec0c6c427a1c04ade22655af` | [upstream](https://git.musl-libc.org/cgit/musl/plain/COPYRIGHT?h=v1.2.5) |
| [Musl-1.2.5-exception-NOTICES.txt](LICENSES/Musl-1.2.5-exception-NOTICES.txt) | `2b44f0dcf066e32c91a9ce86b0894c62361790ffb9bd143fe5d883f3fb6b2da7` | [upstream](https://www.musl-libc.org/releases/musl-1.2.5.tar.gz) |
| [Rust-1.98.1-Apache-2.0.txt](LICENSES/Rust-1.98.1-Apache-2.0.txt) | `62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a` | [upstream](https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/LICENSE-APACHE) |
| [Rust-1.98.1-Compiler-Builtins-LICENSE.txt](LICENSES/Rust-1.98.1-Compiler-Builtins-LICENSE.txt) | `ab6eec6caf0fa5775e411c7a8bc6a45c4ef2956b0980b157ab74fc5cd62a928b` | [upstream](https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/library/compiler-builtins/LICENSE.txt) |
| [Rust-1.98.1-LLVM-compiler-rt-LICENSE.txt](LICENSES/Rust-1.98.1-LLVM-compiler-rt-LICENSE.txt) | `1a8f1058753f1ba890de984e48f0242a3a5c29a6a8f2ed9fd813f36985387e8d` | [upstream](https://raw.githubusercontent.com/rust-lang/llvm-project/52ed14fcd56afc30f9cccd8ca8ce237c2eef7e04/compiler-rt/LICENSE.TXT) |
| [Rust-1.98.1-LLVM-libunwind-LICENSE.txt](LICENSES/Rust-1.98.1-LLVM-libunwind-LICENSE.txt) | `b5efebcaca80879234098e52d1725e6d9eb8fb96a19fce625d39184b705f7b6d` | [upstream](https://raw.githubusercontent.com/rust-lang/llvm-project/52ed14fcd56afc30f9cccd8ca8ce237c2eef7e04/libunwind/LICENSE.TXT) |
| [Rust-1.98.1-MIT.txt](LICENSES/Rust-1.98.1-MIT.txt) | `b71bd43a069ca0641a9ecfe585ca7b3c53b5cc1608f8b68321168698e28b5ea1` | [upstream](https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/LICENSE-MIT) |
| [Rust-1.98.1-Standard-Library-COPYRIGHT.txt](LICENSES/Rust-1.98.1-Standard-Library-COPYRIGHT.txt) | `68129500b616d5838629e68f55ff3aed5e096dacf60ce9eb41bbe599a563afa6` | `Rust 1.98.1 installed toolchain share/doc/rust/COPYRIGHT-library.html` |
| [Rust-1.98.1-libm-LICENSE.txt](LICENSES/Rust-1.98.1-libm-LICENSE.txt) | `3823dda7cf046602f4b4e77ec8e227863dc4736037cc85bb33d9f19febe16bb7` | [upstream](https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/library/compiler-builtins/libm/LICENSE.txt) |
| [Space-Grotesk-OFL-1.1.txt](LICENSES/Space-Grotesk-OFL-1.1.txt) | `c6dec685825f73b18c20926fddc65e8315642e12986f15db0699170940a09efc` | [upstream](https://github.com/floriankarsten/space-grotesk/blob/master/OFL.txt) |
| [Waveshare-CST328-Apache-2.0.txt](LICENSES/Waveshare-CST328-Apache-2.0.txt) | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | [upstream](https://github.com/waveshareteam/Waveshare-ESP32-components/blob/30d0ac3b8b6b27ebd402c5852819cbe0dab92749/display/touch/esp_lcd_touch_cst328/license.txt) |


## Shared Rust workspace 0.2.0

The current collector and device-agent packages share the root `Cargo.lock`.
`LICENSES/Rust/inventory.json` records the four-target dependency audit for Linux
x86_64/aarch64, macOS arm64 and Windows x86_64: 197 crate inventories and 365
retained notice files. Existing standalone-agent notice files remain retained
as an earlier inventory; the shared workspace inventory governs current builds.
The inventory is regenerated with `tools/cargo-notices.py`, using the root
workspace lockfile discovered through Cargo metadata.
