# Set up Glimdock

Start one central collector, register the devices you want to monitor, then pair Glimdock with the collector. The collector and its web console run together. The display does not need credentials or a direct connection to every monitored machine.

## 1. Run the collector

Use the `glimdock-collector` executable built for your Linux or macOS host. Rust is needed to build it, not to run a supplied executable. Windows machines are supported as monitored devices through `glimdock-agent`; the central configuration service currently requires Unix.

Choose a private directory for configuration, credentials and runtime state, then start:

```sh
glimdock-collector run --state-dir ./glimdock-state --bind 127.0.0.1 --port 8765
```

Open `http://127.0.0.1:8765/`. The command runs collection, node configuration and the built-in web interface together. Loopback access works without manually entering keys. It initializes missing state and retains existing configuration and tokens. Stop it with Ctrl-C; use your OS service manager to run it persistently.

The state directory contains `config.json`, separate `display.token` and `setup.token` credentials, `snapshot.json`, and nonsecret `run.json` metadata. It is private to the running account; configuration and token files are mode 0600. Keys are generated from OS randomness and are not printed. The private runtime socket and process lock are managed by the command. Omit `--state-dir` to use `.glimdock`, or supply `--config /ABSOLUTE/PATH/config.json` for an existing external configuration.

A fresh collector monitors its own native host as a Server / device node. Basic native telemetry does not require root. Linux SMART, CPU power, restricted kernel logs and Proxmox guest probes may require additional tools or permissions. Missing capabilities remain unavailable; they do not prevent ordinary server monitoring. Proxmox is an optional capability selected explicitly on a Linux Proxmox host.

### LAN access and keys

For LAN access, restart with the collector's LAN address in `--bind`, for example `--bind 192.0.2.10`, and set `host_ip` in the configuration to the address shown for its own node. Use your real address in place of documentation addresses. Keep HTTP on a trusted network, or use `--cert` and `--key` with your HTTPS certificate and private key.

Remote web access asks for the read-only **display key** and optional **management key**. Read them from `display.token` and `setup.token` privately. The management key is also called the setup token. Direct web access retains entered keys only in browser session memory. The display key reads telemetry; the setup key edits monitoring configuration. A key for one role cannot authorize the other.

Local convenience access is restricted to a loopback connection with a loopback Host; browser writes must come from the same origin. LAN clients and physical displays always need the appropriate key. Feed credentials belong to each registered device and are distinct from these collector keys.

## 2. Register devices in Nodes

Open **Nodes** → **Add node**. Choose **Server / device**, **Proxmox** or **Klipper printer**, then enter the source endpoint and its credential when required.

| Source | Preparation | Registration |
| --- | --- | --- |
| Linux, macOS or Windows machine | Run the native Rust `glimdock-agent` on that machine | Agent snapshot URL and its display token; platform Auto or the known OS |
| Router, switch, UPS or appliance | Run the Rust SNMP or mapped JSON adapter on a reachable machine | Adapter snapshot URL and its display token; Router or Other platform |
| Proxmox host | Run a collector with explicit Proxmox capability there | Its base snapshot URL and display token |
| Klipper printer | Enable read access to its Moonraker API | Moonraker origin, normally port 7125; API key if required |
| Collector's own host | Already available in a fresh configuration | Edit, pause, remove or restore **Hub host** |

[Device setup](public-docs/DEVICES.md) includes agent, SNMP and JSON examples. Adapters use native OS and platform-specific data as well as sensors. SNMP OIDs and JSON field mappings are configured on the adapter host; node management stores the normalized feed endpoint and read token on the central collector.

Leave a new node ID blank to assign one automatically. Renaming a node or changing its supported capability preserves the existing ID. Registration is explicit; the collector does not scan your network or discover credentials. Once registered, enabled nodes appear automatically in the web overview, browser firmware and physical display.

You can keep sixteen feeds configured and four nodes active at once, counting the optional hub host. **Pause** retains a feed's settings and credentials without polling it; **Resume** enables it again. Removing the hub host disables local probes and retains its restoration information. A collector with no active nodes keeps management available and clears the display's old cards.

The editor supports sampling interval, request timeout and freshness TTL. TTL must cover at least two polling intervals. Auto platform uses the upstream descriptor. A blank existing key retains it for the same service origin; explicit clearing removes the association. Changing host, scheme or port needs the destination's credential. Public configuration returns `has_secret`, never the key or its local file path. Concurrent edits require reloading the saved configuration before applying another change.

## 3. Install and pair the display

Use **Waveshare ESP32-S3-Touch-LCD-2.8 V1 / ST7789 + CST328** with 16 MiB flash and 8 MiB PSRAM. The V1 image does not support V2/CST3530. The current Pebble Landscape enclosure uses rotation 3, USB to the right, at 320 × 240.

Open **Display & firmware** in desktop Chrome or Edge. Connect the board by USB and choose **Connect & update**, then select it in the browser's device chooser. The updater checks the chip, manifest and all image hashes, writes the required flash segments and restarts the board. Saved Wi-Fi, collector pairing and preferences are retained; a new board opens setup. The same page provides the public image's source/relink materials.

If your collector is on LAN HTTP, run the same Rust interface on the computer connected to the board:

```sh
glimdock-collector serve --bind 127.0.0.1 --port 8766 \
  --upstream http://COLLECTOR_ADDRESS:8765 \
  --token-file /ABSOLUTE/PRIVATE/display.token \
  --setup-token-file /ABSOLUTE/PRIVATE/setup.token
```

Open `http://127.0.0.1:8766/`. The companion serves the same embedded interface and keeps both keys server-side, assigning the appropriate role to each fixed upstream API request. A directly served HTTPS collector supports USB without this companion.

On the physical display, choose **Configure on this display** or **Settings → Wi-Fi & collector setup**. Enter your 2.4 GHz Wi-Fi details, display key and this base endpoint:

```text
http://COLLECTOR_ADDRESS:8765/api/v1/snapshot
```

Do not add a node query; Glimdock selects registered nodes itself. Pair the optional setup token if you want to manage nodes from the touchscreen. Node inventory belongs to the collector; Wi-Fi, pairing and appearance belong to the display.

For first-boot or HTTPS CA setup, join the password-protected **Homelab-Setup** Wi-Fi network using the password displayed on screen, then open `http://192.168.4.1/`. **Advanced browser setup / HTTPS CA** accepts the issuing CA certificate. TLS verification stays enabled.

Glimdock starts on **All nodes**, a bento overview of every active node. Tap a card for its detailed dashboard, tap the logo to return, or use the header picker. Readings that the platform does not expose remain `--`; stale and offline feeds clear their current measurements. The web's firmware canvas uses the same production LVGL screens, parsers and touch behavior, with browser transport replacing the board hardware.

A private build with a complete valid pairing saves it after its first successful authenticated snapshot when no pairing is already saved. Public firmware contains blank build defaults and reconnects using saved NVS. Ordinary browser updates preserve this state; a full flash erase clears it. [Firmware setup](firmware/README.md) covers source builds, other orientations and recovery.

## Optional Linux systemd deployment

Use the supplied deployment for a persistent Linux service with separate privileged collection/configuration and an unprivileged HTTP reader. Copy the matching executable and `deploy/` files to the host, then run:

```sh
sudo ./deploy/install.sh --binary /ABSOLUTE/PATH/glimdock-collector
sudoedit /etc/homelab-monitor/config.json
sudoedit /etc/homelab-monitor/server.env
sudo /opt/homelab-monitor/glimdock-collector validate-config --config /etc/homelab-monitor/config.json
sudo systemctl enable --now homelab-monitor-config homelab-monitor-collector homelab-monitor-http
```

The installer retains existing configuration and tokens. New installations bind to loopback until `HOMELAB_BIND` in `server.env` is set to the desired listening address. The private environment file contains `HOMELAB_DISPLAY_TOKEN` and `HOMELAB_SETUP_TOKEN`. The installer changes no firewall rules or probe packages. For an update, use the same installer with `--start`. `deploy/install-rust.sh` remains a compatibility wrapper.

Systemd node edits apply through the private configuration socket and restart only monitoring collection. The HTTP service keeps running. After manual configuration edits, validate the file and restart `homelab-monitor-collector`. Existing `homelab-monitor-*` names and `/etc/homelab-monitor` paths are retained for upgrade compatibility.

## Configuration and optional integrations

[examples/collector.json](examples/collector.json) is the generic starting point, also distributed as [deploy/config.example.json](deploy/config.example.json):

```json
{
  "host_ip": "127.0.0.1",
  "node": "",
  "display_name": "",
  "enable_local": true,
  "local_type": "server",
  "printers": [],
  "remote_collectors": []
}
```

An empty `node` uses the actual hostname. `display_name` changes its visible label; `local_node_id` retains its selection identity. `enable_local: false` makes the collector a remote-only aggregator. Set `local_type: "proxmox"` explicitly for Proxmox-specific collection on a Linux Proxmox host. Legacy `enable_proxmox` configurations retain their mode and identity when `local_type` is absent; use either that alias or `enable_local`, never both.

The web editor is the usual way to manage feeds. A manually configured feed uses `remote_collectors`, a stable `id`, `type` (`server` or `proxmox`), optional `platform`, fixed HTTP(S) `url`, private `token_file` and polling fields. Printers use `printers` and `api_key_file`. URLs contain no embedded credentials, queries or arbitrary API paths; device feeds accept an origin or the fixed snapshot endpoint. Redirects are refused and HTTPS is verified.

Proxmox probes, QEMU guest agents, restricted TrueNAS reads, SMART, ZFS, GPU and package-power tools are additional capabilities. [Native telemetry](collector-rs/docs/TELEMETRY.md) documents their setup and measurement meanings. Guest OS memory remains distinct from assigned RAM and hypervisor accounting; component watts are not guessed wall power; ZFS status is not SMART health. Optional Python NAS/QGA compatibility payloads run on those monitored systems, while the collector runtime is Rust.

## Troubleshooting

Check the foreground collector output or, for systemd, `journalctl -u homelab-monitor-collector -u homelab-monitor-http -u homelab-monitor-config`. HTTP 401 usually means the wrong key or role; a missing selected node returns 404; startup or publication failures can return 503. Use a private Bearer header to inspect telemetry, never a token in the URL.

For an offline remote device, check its agent/adapter listener, credential and source data first. Frozen upstream sequences and expired source timestamps stay stale even if HTTP requests succeed. For USB, use desktop Chrome/Edge on HTTPS or localhost and release any other serial monitor holding the board. For Wi-Fi, check 2.4 GHz support and the saved pairing. Exclude secrets and real snapshots from issue reports.
