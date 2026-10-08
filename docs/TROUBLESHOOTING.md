# Troubleshooting

## The display is offline

Check that the Wi-Fi is 2.4 GHz, that saved credentials are correct and that the
display can reach the collector's LAN address. In `server.env`, a fresh install
is loopback-only: set `HOMELAB_BIND` deliberately to your LAN address, then restart
the HTTP service. Check the host firewall separately; the installer does not edit it.

On the collector, inspect:

```sh
systemctl status homelab-monitor-config homelab-monitor-collector homelab-monitor-http
journalctl -u homelab-monitor-collector -u homelab-monitor-http -u homelab-monitor-config
curl http://192.0.2.10:8765/healthz
```

Replace the example address with your host. Use a private Bearer header for
authenticated snapshot checks. Keep tokens out of URLs, copied command history,
issues and screenshots.

| HTTP result | Likely cause |
| --- | --- |
| 401 | Wrong/missing token or incorrect read/setup role |
| 404 for a selected node | The node ID was removed or changed |
| 503 | No valid snapshot has been published yet; inspect collector logs and wait for the first sample |
| HTTPS validation failure | Untrusted/missing CA, wrong hostname or expired certificate; configure the correct CA rather than disabling verification |

## A measurement says unavailable

Check its source status and age before changing the UI. Sources need an initial
sample and can expire independently. Host sensors need `sensors -j` plus the
correct kernel drivers; CPU power needs supported `turbostat`/RAPL. Current is
only available when a real current sensor is exposed.

Proxmox cannot read disks owned by a passed-through NAS controller. Configure
the NAS helper and confirm its fixed-scope account can read the relevant disks
and pools. Avoid granting a general monitoring shell or broad passwordless sudo.

For guest RAM, enable the Proxmox QGA option, install/start the agent in the guest,
add the VM ID to `qga_guest_ids`, then verify `qm agent <VMID> ping`. A guest
starting up may be unavailable temporarily; it should recover after successful
guest telemetry. Do not substitute allocated RAM for guest OS usage.

## The screen flashes, loses power or touch is wrong

Confirm **V1 / CST328 hardware**. V2 has a different touch controller and this
release cannot drive it correctly. The V1 firmware asserts GPIO7 early and leaves
gyro, RTC, audio and sleep modules uninitialized. Use a stable USB supply.

If the board loses power during ROM upload, hold BAT_PWR during flashing and
release it after the firmware boots. Do not erase NVS merely to update firmware;
normal updates retain settings. Check the build's landscape rotation for the
enclosure orientation. See [SETUP.md](../SETUP.md).

## A swipe opens a row

Use a continuous drag on the list. The firmware separates taps from scrolls and
has native LVGL gesture regression checks. A new regression report should include
firmware version, V1 board confirmation, screen name and reproducible gesture
steps. Use synthetic screenshots rather than a private lab capture.

## Filing a useful issue

Describe the version, collector architecture/OS, board revision, affected source
and minimal reproduction. Include redacted logs or authored fixtures. Report
security vulnerabilities privately, not in a public issue.
