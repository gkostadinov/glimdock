# Website validation

## Push-agent enrollment — 10 October 2026

Homepage, setup and open-source copy now describe explicit one-time pairing,
automatically assigned stable agent nodes, explicit `--re-enroll` recovery and
device-to-collector push. Native
agents need no incoming port or feed URL; SNMP/JSON adapters collect locally and
push, while Moonraker and existing feeds retain polling. `/open/` and `/setup/`
link to the public [v0.3.0 release](https://github.com/gkostadinov/glimdock/releases/tag/v0.3.0), covering matching Linux/macOS
collector and agent packages and the Windows device agent without guessing
asset names. The release owner confirmed publication at 00:44:16 Europe/Sofia
on 10 October 2026 (2026-10-09T21:44:16Z), with all five packages and SHA256SUMS
uploaded and digests matching the audited artifacts. This establishes GitHub
release availability; website publication remains a separate root-owned step.

The production renderer was refreshed byte for byte from `collector-web/emulator/`
without changing CAD images. Its WASM SHA-256 is
`ef79a8f5d8bc2fb2c380121c57dd569730d755e60a1fefb048b55124825a58db`.
The synthetic All nodes fallback was recaptured from that renderer; its PNG hash
is `ae1df6ed6554d6a42ff71b9a53b83002ed659d066ee63c99d2bdcd0267a7b743`.
The product-provenance manifest binds these current assets.

Using Node.js 22.20.0, `npm run build`, `npm test` and `npm run check` pass. All
28 behavioral tests pass, including real WASM parsing/drawing, frame readiness,
commerce and consent boundaries. The Worker includes 43 assets and is
2,069,759 bytes, within the existing budget. The first generic npm run used the
checkout's Node 20 default and failed its SQLite prerequisite; rerunning with
the documented Node 22 runtime passed without changing those tests.

This records local source/asset validation. Root publication and any browser
review are separate evidence. Price, legal terms, payment gates, analytics, case
claims and public-demo read-only behavior were preserved.


## Central collector rewrite — 9 October 2026

The product site now describes one host-agnostic Rust collector, its integrated
web interface and API, explicit node registration, Windows/macOS/Linux agents,
SNMP/JSON adapters, optional Proxmox and Moonraker capabilities, the firmware
bento overview and Chrome/Edge USB updates. `/setup/` is a new local route with
collector → feed → registry → display pairing steps. `/open/` points to current
`main` guides and source ZIP, with v0.1.0 explicitly labelled as the older
Proxmox-first release. The v0.2 binaries remain unpublished; no new binary URL is
invented. Setup, device, build, CAD and source ZIP links now target the merged
main source.

Validation commands from `commercial/site/`:

```sh
npm run build
npm test
npm run check
```

Use Node.js 22.13+; the recorded run used 22.20.0. All **28 tests pass**:

- Existing checkout and commerce workflows retain server-owned price, allowed
  destinations, shipping, readiness dependencies, accepted-terms snapshots,
  signed webhook handling, payment verification and durable email retries.
- The website's firmware.js, firmware.wasm, wrapper, manifest and license notices
  match `collector-web/emulator/` byte for byte. The WASM digest is
  `e2c394bc4f8da8a149c70b96d178616a43b514947c2bfa645af13c4426a3b0c9`.
- The production snapshot parser and LVGL draw all four synthetic platform cards.
  Stable-ID selection opens the real Klipper page with parsed 68% progress and
  215 °C nozzle readings. No synthetic transport makes a LAN or device request;
  public configuration callbacks are absent.
- The parent frame reveals the firmware only after a trusted same-origin ready
  signal. Reset waits for startup; a failed renderer returns to a labelled
  native fallback capture.
- Only the demo document permits WebAssembly compilation and same-origin
  embedding. It has self-only connections and no form submission. The WASM
  route serves `application/wasm`. The main site's existing CSP stays strict.

The embedded Worker contains 43 files and is below the existing 3,000,000-byte
limit (approximately 2.06 MB). Static checks verify JavaScript syntax, local
routes/assets, one H1 and canonical/description metadata per page. Detailed
build/test logs are in `output/website-central-qa/`.

## Browser review

An isolated in-app-browser tab reviewed the local Worker at
http://127.0.0.1:8772/. It visibly showed the production All nodes canvas, with
macOS, Windows, router and printer bento cards. Tapping the router opened its
actual firmware overview; Reset returned to all four nodes. Desktop and 390 px
homepage checks had no horizontal overflow. The 320 px setup guide exposed a
flex-item/code overflow; it was fixed and rechecked. Homepage, setup, open and
order routes then fit the 320 px viewport. The order button remained disabled
with the existing readiness configuration. Temporary viewport overrides were
reset after review.

Saved evidence:

- `output/website-central-qa/firmware-browser.png`: actual embedded renderer with
  the surrounding website copy and all four cards.
- `output/website-central-qa/firmware-mobile390.png`: responsive firmware section.
- `public/assets/screens/all-nodes.png`: the exact public LVGL frame, produced
  with website synthetic data, not live private telemetry.
- `public/assets/product-provenance.json`: case revision, source/destination
  render hashes, renderer manifest and fallback data/PNG provenance.

No browser USB hardware transaction was performed by the website review. The
public marketing demo is read-only and has no updater. Device programming and
physical case USB-fit evidence belong to the collector/firmware validation.

## Case and publication boundaries

Hero, rear, exploded, ears, antennas and eyes images were encoded from the actual
`enclosure/pebble-landscape/output/renders/` geometry at 1400 × 1232. The source
revision is `r19-concentric-exact-surface-fit-landscape-pebble`. Captions identify
CAD renders and their earlier synthetic firmware artwork. No visual critique
score is marketed as dimensional fit. The main case is USB-powered; complete
assembly, the internal extension's fit/data path and a future battery remain
unverified. Historical portrait/Serein images and the old HTML demo are not
served as the current product.

The €99 price, quantity/destination/shipping rules, runtime readiness flags,
accepted terms, privacy text, payment APIs, analytics/consent and database schema
were not changed. Only the demo-document CSP extension was added to the Worker.
This local validation does not claim public deployment, Stripe activation or
physical case approval. Publication uses the original Sites project/version
workflow and requires separate deployment read-back.
