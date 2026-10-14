# macOS RAM, compression and pressure

Glimdock's native macOS agent reads Mach VM statistics and fixed sysctl counters
directly, without root, shell scripts or a QEMU guest agent. Both Apple silicon
and Intel Macs use their actual kernel page size. Upgrade the binary while
preserving the agent's private state directory and restart its launch service.

The device's **Memory** detail shows used/total RAM, the kernel's memory pressure
state, app memory (estimate), wired RAM, physical compressed RAM, cached files and
swap. The same detail appears under a mapped Proxmox guest. A guest list shows
macOS RAM and pressure with its measurement age.

## How the readings work

Used RAM is `(internal pages − purgeable pages + wired pages + compressor pages)
× page size`. The subtraction saturates if counters race. App memory is the
non-purgeable anonymous portion, an estimate rather than summed process RSS.
Cached files are `(external pages + purgeable pages) × page size`; cache is
reported separately and is not added to used RAM. Compression counts physical
pages occupied by the compressor, rather than their larger uncompressed size.
Available is `physical total − used`, an estimate of headroom including cache.

Pressure comes from `kern.memorystatus_vm_pressure_level`: normal, warning or
critical. It is a kernel state, not a made-up percentage or a reproduction of
Activity Monitor's historical graph. Unknown states remain unavailable. Swap
comes from `vm.swapusage`; a valid zero is shown as zero. Counter failures remain
unknown instead of showing zero or falling back to a different accounting basis.

This is Activity Monitor-style physical accounting, not a guarantee of identical
values at different sample times. See [Apple's memory explanations](https://support.apple.com/guide/activity-monitor/actmntr1004/mac)
and [XNU's VM statistics definition](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/vm_statistics.h).

## Use a native agent for a Proxmox macOS VM

Proxmox can show all assigned RAM as the host footprint even while macOS has
headroom. A QEMU guest agent alone does not supply memory pressure or install a
balloon driver. A native Glimdock agent avoids changing VM hardware or rebooting.

1. Install the native agent inside macOS using [the service instructions](../deploy/agents/README.md).
   Pair it with the collector and retain its assigned stable node ID.
2. In the collector's private configuration, add an explicit mapping:

```json
{
  "guest_memory_sources": [
    {
      "node_id": "proxmox:example",
      "guest_id": 103,
      "agent_node_id": "agent:macos-guest"
    }
  ]
}
```

Merge this field into the existing config; do not replace the whole document.
`node_id` is the configured Proxmox node's stable ID. `agent_node_id` is an
authenticated push agent or authenticated server polling feed, not its hostname.
Use a current collector and native agent. Set the agent's `parent_id` to the
Proxmox node if you want it grouped as a subnode rather than on the root overview.
The integrated `run` collector watches valid saved configuration changes automatically.
For the optional split systemd installation, restart `homelab-monitor-collector`
after a manual file edit; keep the configuration and HTTP services running.

There must be one memory source per guest: remove the VM from `qga_guest_ids` if
it uses this mapping. A TrueNAS SSH memory source cannot also map the same guest.
Mappings support local and remote Proxmox nodes; publishers cannot select a VM
ID in telemetry. Remove affected bindings before deleting their configured feed
or Proxmox node.

The guest must be running, Proxmox guest status must be available, and both the
native feed and its memory probe must be within the configured feed TTL. Expired,
paused, revoked or invalid telemetry clears OS memory readings with an explicit
error. Assigned RAM and Proxmox host footprint remain separate detail fields.

## Snapshot fields

Native host `memory_basis` is `macos-native`; mapped guest `memory_basis` is
`guest-os-macos`. Optional numeric fields are `mem_available_bytes`,
`mem_app_bytes`, `mem_wired_bytes`, `mem_compressed_bytes`, `mem_cache_bytes`,
`swap_used_bytes` and `swap_total_bytes`. `memory_pressure` is `normal`, `warning`,
`critical` or null. All values are bytes, not kilobytes or percentages. The
mapped guest additionally includes `memory_source_node`, `mem_updated_at`,
`mem_age_s` and `mem_error`. No credentials or process inventories are included.

## CPU temperature, CPU power and battery

On physical Apple silicon Macs, the optional native hardware sampler uses the
pinned MIT-licensed macmon library for SMC CPU temperature and IOReport CPU
package power estimates. It runs on a dedicated thread and publishes bounded
scalar readings with a ten-second freshness limit. These are component estimates,
not wall-socket power. Missing or zero-fallback power counters remain unavailable.
The macOS native battery gauge reports charge, plugged/charging/full state and
remaining time when the OS supplies it. A desktop or VM without a battery has
no invented battery row. VM CPU power and temperature remain unavailable when
the virtual hardware does not expose real physical readings; host CPU power
is visible on the Proxmox system separately.
