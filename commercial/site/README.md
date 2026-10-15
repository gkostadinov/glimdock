# Glimdock website

The public product, setup, source-download and preorder site at https://glimdock.com.
It describes one local Rust collector with an integrated node-management console,
telemetry API, production firmware emulator and browser USB updates. This
commercial website is separate from the collector UI in `collector-web/`.

The original Sites project and its D1 binding are retained. Publish through its
source/version workflow; Cloudflare manages DNS only. Product-copy work does not
activate payments, change accepted terms or attest to hardware readiness.

## Local development

Use Node.js 22.13+ (commerce workflow tests use the built-in SQLite module):

```sh
cd commercial/site
npm ci
npm run dev
```

Open http://127.0.0.1:8772/. The development server rebuilds `public/` and `worker/`
on the next request. Verify the complete embedded Worker before publishing:

```sh
npm run build
npm test
npm run check
```

Use the same Node version for npm and tests. The production Worker has no npm
runtime dependencies. HTML, local fonts, reviewed CAD images, the public firmware
WASM renderer and accepted-terms snapshots are embedded in one roughly 2.1 MB ES
module, checked against the existing 3 MB budget. Drizzle generates migrations;
Sites applies them before Worker upload. No schema or commerce behavior changed
for the central-collector rewrite.

## Product pages and architecture

- `/`: devices → central collector → browser and Glimdock; native OS telemetry,
  optional Proxmox and Klipper capabilities, node management, All nodes bento,
  real firmware demo, USB updates and the current Pebble landscape case.
- `/setup/`: one Linux/macOS collector, Rust push agents, single-use enrollment,
  keys and Wi-Fi pairing, production emulator and Chrome/Edge update steps.
- `/open/`: guides for the current main source, released archives and CAD. The earlier
  v0.1.0 release is explicitly historical; it is not presented as containing
  `glimdock-collector run` or the current integrated web interface.
- `/order/`: the same €99, destinations, delivery estimate and gated checkout.

Windows/macOS/Linux native agents are monitored devices; the central-server
setup currently targets Linux/macOS. SNMP and JSON API adapters run as configured
agents, collect locally and push normalized readings. Agents pair once and
receive stable node identities without per-device URLs or incoming ports.
Printers and existing APIs retain polling. Enrollment is explicit, without
automatic LAN discovery. The collector supports four active nodes and retains up
to sixteen configured remote nodes. Hardware-dependent readings remain optional.

## Production firmware demo

`public/demo/emulator/` is copied byte for byte from `collector-web/emulator/`.
It contains the production C++ UI, snapshot parser and LVGL software renderer,
compiled to WebAssembly. Canvas output is exactly 320 × 240; the browser only
scales those pixels and transports pointer events. The earlier HTML/SVG
simulation and its demo fonts are removed.

`public/demo/data.js` creates four synthetic feeds in memory: Mac, Proxmox,
router and Klipper with a child switch. Addresses are documentation-only `192.0.2.0/24`. No LAN,
collector or printer request is made. Configuration callbacks are absent, so the
public demo cannot change a node registry. Display theme and brightness changes
last only until reset. An advancing sequence keeps demonstration freshness
accurate. A verified child ready message reveals the canvas; a failed startup
returns to the labelled firmware capture.

Only `/demo/index.html` permits `wasm-unsafe-eval`, temporary inline canvas styles
and same-origin framing. Its connections are restricted to self, and forms are
disabled. The main site's existing CSP, consent and commerce rules stay intact.
Third-party renderer notices are in `public/demo/emulator/LICENSES.txt`; transport
and site code are MIT.

## CAD images and asset refresh

`enclosure/pebble-landscape/` is the current case source. Hero, rear, exploded and
accessory images come from its actual r19 printable CAD renders. The renderer
uses an older synthetic Proxmox screen texture; captions do not identify it as
the new All nodes view. Full hardware fit, assembly and USB-extension power/data
validation remain unfinished. The main case is USB-powered; its battery variant
is a future concept. A printed body or a visual critique does not prove assembly
fit or product conformity.

After the public firmware renderer or approved case renders change, regenerate
site assets with Python + Pillow and Node (optional asset tooling only):

```sh
python scripts/sync-product-assets.py
```

The script copies the public WASM files, encodes the reviewed CAD PNGs as WebP,
renders a fresh synthetic All nodes fallback and writes
`public/assets/product-provenance.json` with source and destination hashes.
It does not rebuild firmware, flash hardware or publish. Normal site builds and
its runtime require no Python. Rebuild and test the Worker after refreshing.

## Payments

Stripe hosts card entry. The server owns the €99 unit price, quantity limits 1–5, EU destination allowlist, €5 Bulgaria / €15 EU shipping per order, and all totals. The seller is an individual who reports not being VAT registered; no VAT is charged. No client total, tax number, address or invented stock count is trusted. An idempotency key makes retries safe. Shipping is limited to the selected country, so the customer cannot switch to another rate destination within checkout.

Checkout remains unavailable until actual setup is complete. Configure runtime values with Sites environment tools; never commit keys or put them into `.openai/hosting.json`, frontend code or a public repository.

- `STRIPE_SECRET_KEY`: live restricted or secret API key with Checkout Session write/read permissions.
- `STRIPE_WEBHOOK_SECRET`: signing secret for `/api/stripe-webhook`.
- `SELLER_POSTAL_ADDRESS`: actual public seller and returns postal address, escaped into terms/privacy.
- `SUPPORT_READY=true`: support inbox receives mail and the returns process is operational.
- `PRODUCTION_READY=true`: actual board, enclosure and applicable product conformity requirements verified for the sale.
- `ORDER_CONFIRMATIONS_READY=true`: merchant receipt/order confirmation workflow configured and tested; includes delivery estimate and terms.
- `RESEND_API_KEY` and `MAIL_FROM`: verified sending domain for durable withdrawal acknowledgements.
- `OPERATIONS_TOKEN`: secret with at least 32 characters for the existing retry endpoint.
- `OPERATIONS_READY=true`: authenticated retry schedule and pending-count monitoring verified independently of site visits.
- `CHECKOUT_ENABLED=true`: final activation after all above prerequisites are verified.

`DB` is a Sites-provisioned D1 binding. Schema source and immutable generated SQL/snapshots are in `db/` and `drizzle/`. Session verification and signed webhooks record matching orders; a success URL does not prove payment. The verified paid-order workflow sends an idempotent confirmation through Resend with price, shipping, delivery estimate and an attached standalone copy of the accepted terms. Append-only snapshots in `worker/accepted-terms.js` are selected by the Checkout Session terms version; the public seller address is captured in session metadata when checkout starts. Keep every issued snapshot when adding a new terms version. Webhook failures request a safe Stripe retry; the status page distinguishes verified payment from pending email confirmation. Configure Stripe events `checkout.session.completed`, `checkout.session.async_payment_succeeded` and `checkout.session.async_payment_failed`. Price/currency/product metadata are verified before recording. Do not enable live checkout merely to test it; use isolated mocked tests or a separate Stripe sandbox. The public shop deliberately rejects test keys to avoid presenting simulated purchases as real orders.

The withdrawal form stays available when order storage is available, including after new sales are disabled. It matches a SHA-256 of the checkout email retained from the verified order, durably stores notice with timestamp, and sends an idempotent acknowledgement through Resend. Email failure is labelled accurately; never claim delivery when it failed. Unacknowledged withdrawal notices form a durable outbox with bounded exponential backoff and a five-minute claim lease. Shop GET requests attempt due acknowledgements opportunistically. For reliable recovery even when the site receives no visits, configure an authenticated scheduled POST to `/api/admin/retry-withdrawals` with `Authorization: Bearer <OPERATIONS_TOKEN>` from the chosen operations scheduler, and monitor pending counts. Configure this before activating payments; `OPERATIONS_READY` remains false until scheduling and monitoring are verified. The endpoint returns `sent`, total unacknowledged `pending` and currently eligible `due` counts only, never purchaser details. Support handles cancellations/refunds/returns through Stripe and the operational process. Customer withdrawal submissions are not analytics events.

## Analytics and consent

`GA_MEASUREMENT_ID=G-ENKHQR7PEK` is configured in Sites. `GOOGLE_ADS_ID` stays unset. IDs are public configuration, never payment secrets. Google scripts load only after the corresponding category opt-in; no Google preconnect or tag is loaded by default. Analytics and advertising are separate, unchecked choices. Equal reject/accept controls and footer preferences remain available. Basic Consent Mode v2 defaults all four Google consent fields denied. Tracking configuration IDs are included in the consent version, invalidating old choices when providers change. Preferences are device-local, expire after 180 days, and contain no customer identifiers.

Google Analytics was configured on 9 October 2026 with two-month user/event retention, retention reset on new activity off, Google Signals off, ad personalization off, detailed location/device collection off, and enhanced site-search/form measurement off. Keep these settings when changing the property. Page locations are stripped to origin/path to avoid email/address/order-session query parameters. Checkout tracking may emit `begin_checkout` only after a valid Stripe redirect is returned. No purchase conversion is sent based solely on a redirect or query flag.

## SEO and publication

Unique titles/descriptions, canonical URLs, Open Graph metadata, local logo/favicon, Product entity data, robots and sitemap are included. Product `offers` and availability claims are intentionally omitted while checkout is not accepting paid orders. Add accurate offer/shipping data only when activation is real. Search visibility requires a public Site, domain verification and crawler access; rankings are never guaranteed. Publish after tests through Sites, inspect successful deployment status, then map native Sites DNS/SSL records in Cloudflare. Do not guess an apex IP or copy an unrelated Site's CNAME.


The current source and guide links target `main`, including its source ZIP and
build guide. `/open/` and `/setup/` link to the public
[v0.3.0 release](https://github.com/gkostadinov/glimdock/releases/tag/v0.3.0) for matching Linux/macOS collector and agent packages
plus a Windows device agent. The release was published on 10 October 2026 at
00:44:16 Europe/Sofia (9 October 2026 at 21:44:16 UTC). Its five platform packages
and SHA256SUMS were uploaded and their digests matched the release audit. The
website uses one release-page link rather than guessed platform filenames.
The v0.1.0 artifacts describe the older architecture.
The explicitly labelled v0.1.0 link remains an archive. Do not
label old binaries as the current central collector. Local product-copy
validation does not confirm that a public Site version has been deployed.

## Public USB installer

`/flash/` installs the supported Waveshare V1 public firmware directly with Web Serial. Run `python3 scripts/sync-flash-assets.py --release v0.5.6` after building or selecting an audited public firmware bundle. Use an existing immutable release whose source-relink archive matches the local manifest. The script copies only four verified images, the shared USB updater, flash safeguards and notices; the larger corresponding source archive is linked from GitHub. It never copies local Wi-Fi settings, monitoring keys, device backups or private builds.

The website worker permits Serial only on the installer page. Firmware manifest and image hashes must match `collector-web`; tests and the public export enforce this. The worker budget is checked both uncompressed and gzip compressed. To update the UI, run `node scripts/build.mjs`, `node --test tests/*.test.mjs` and `node scripts/check.mjs`. The local collector remains an alternative installer and live monitoring console.
