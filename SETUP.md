# Set up Glimdock

Start one central collector, pair agents that send their readings, then pair Glimdock with the collector. The collector and its web console run together. The display does not need credentials or a direct connection to every monitored machine.

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

Local convenience access is restricted to a loopback connection with a loopback Host; browser writes must come from the same origin. LAN clients and physical displays always need the appropriate key. One-time pairing and per-agent publisher credentials are distinct from these collector keys. The console shows an unknown inventory before authentication, not a zero-node collector.

## 2. Pair devices in Nodes

Open **Nodes** → **Add node** → **Pair a device**. Enter a name and a collector address reachable from the monitored machine, then choose **Create pairing key**. Platform detection is automatic unless you select a specific OS. The key is shown once and expires after ten minutes; save it privately on the device as `enrollment.key`.

Run the native Rust agent on the device with that key file:

```sh
chmod 600 /ABSOLUTE/PRIVATE/enrollment.key
glimdock-agent --collector-url https://COLLECTOR_ADDRESS:8765 \
  --state-dir /ABSOLUTE/PRIVATE/glimdock-agent \
  --enrollment-key-file /ABSOLUTE/PRIVATE/enrollment.key \
  --config /ABSOLUTE/PRIVATE/host.json
```

Use [examples/agents/host.json](examples/agents/host.json) for native host readings. The collector assigns the node identity; the agent stores its private publisher credential and sends readings periodically. No listener or incoming port on the monitored device is needed. After successful pairing, delete `enrollment.key` and omit `--enrollment-key-file` on later starts. Keep the same private state directory to retain identity. [Device setup](public-docs/DEVICES.md) includes Windows commands, services and adapter modes.

HTTPS certificates are verified. With a private CA, add `--collector-ca-cert /ABSOLUTE/PRIVATE/ca.pem`. For trusted LAN HTTP, use `http://COLLECTOR_ADDRESS:8765` and explicitly add `--allow-insecure-http`; HTTP does not encrypt credentials or telemetry. The console generates the appropriate command for the chosen address. A localhost address is usable only by agents on that collector computer.

| Source | How readings reach the collector |
| --- | --- |
| Linux, macOS or Windows machine | Pair the Rust agent and push native OS readings |
| Router, switch, UPS or appliance | Run a paired Rust SNMP or JSON adapter on a machine that can reach the device; it polls the device locally and pushes normalized readings |
| Proxmox host | Enable the collector's optional local Proxmox capability, or retain a compatible polling feed for its additional guest/storage/GPU data |
| Klipper printer | **Add a polling feed** using its Moonraker origin, normally port 7125, and API key when required |
| Collector's own host | Already available in a fresh configuration; edit, pause, remove or restore **Hub host** |

Devices are enrolled explicitly; the collector does not scan the network or discover credentials. After pairing, enabled nodes appear automatically in the web overview, browser firmware and physical display. SNMP OIDs and JSON field mappings remain in the adapter configuration; secrets for the source device remain on its adapter host.

You can keep sixteen remote nodes configured and four nodes active at once, counting the optional collector host. **Pause** retains an agent's enrollment and hides it from active monitoring. **Resume** restores it. **Revoke access** immediately rejects its publisher credential and clears its current readings, leaving a disabled node that can be paired again. **Remove node** revokes access and removes the registry entry. The agent process may continue running until stopped on its machine.

An agent row distinguishes **Awaiting agent**, **Paused**, **Revoked** and live health, and shows the last receipt time. Measurement age is independent of receipt time; delayed readings stay stale. Configure expiry to cover at least three sending intervals. Unavailable or expired values are cleared rather than presented as current measurements.

For existing HTTP feeds, **Edit** → **Switch to push** creates a pairing key while retaining the node ID. Start the replacement agent with the new key. Polling stops when the switch is saved; readings resume after enrollment. For an existing push node, **Create new pairing key** immediately revokes the old publisher. Stop the agent and run the console’s generated command with `--re-enroll`, the new `--enrollment-key-file` and the same absolute state directory. This preserves its durable device identity while rotating its publisher credential. After success, remove `--re-enroll` and the enrollment-file option from normal service arguments, and delete the used key file. Re-pairing is a deliberate action, not a way to retrieve a saved key. Renaming, pausing and ordinary firmware updates retain node identity and existing collector/display keys.

Polling feeds retain their separate interval, timeout and TTL controls. A blank saved feed key retains it for the same service origin; clearing it removes the association. Changing host, scheme or port requires the destination's credential. Public configuration returns credential presence only. Agent pairing keys and publisher credentials cannot be read back from the management API. Concurrent changes require reviewing refreshed configuration before retrying.

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

The web editor is the usual way to pair agents and manage feeds. Agent enrollment is stored separately from legacy `remote_collectors`; use the pairing flow rather than hand-writing publisher credentials. A manually configured feed uses `remote_collectors`, a stable `id`, `type` (`server` or `proxmox`), optional `platform`, fixed HTTP(S) `url`, private `token_file` and polling fields. Printers use `printers` and `api_key_file`. URLs contain no embedded credentials, queries or arbitrary API paths; device feeds accept an origin or the fixed snapshot endpoint. Redirects are refused and HTTPS is verified.

Proxmox probes, QEMU guest agents, restricted TrueNAS reads, SMART, ZFS, GPU and package-power tools are additional capabilities. [Native telemetry](collector-rs/docs/TELEMETRY.md) documents their setup and measurement meanings. Guest OS memory remains distinct from assigned RAM and hypervisor accounting; component watts are not guessed wall power; ZFS status is not SMART health. Optional Python NAS/QGA compatibility payloads run on those monitored systems, while the collector runtime is Rust.

## Troubleshooting

Check the foreground collector output or, for systemd, `journalctl -u homelab-monitor-collector -u homelab-monitor-http -u homelab-monitor-config`. HTTP 401 usually means the wrong key or role; a missing selected node returns 404; startup or publication failures can return 503. Use a private Bearer header to inspect telemetry, never a token in the URL.

For an offline push device, check its agent process, outbound access to the collector, private state and source data first. A consumed or expired pairing key requires a new key from the console; a revoked publisher requires a fresh key for the existing node and one explicit `--re-enroll` run. Keep the existing agent state; do not delete it as a recovery step. For legacy polling feeds, check the listener and read credential. Frozen upstream sequences and expired source timestamps stay stale even if HTTP requests succeed. For USB, use desktop Chrome/Edge on HTTPS or localhost and release any other serial monitor holding the board. For Wi-Fi, check 2.4 GHz support and the saved pairing. Exclude secrets and real snapshots from issue reports.
