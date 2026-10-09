# Outbound agent services

The native agent samples locally and pushes to one central collector. It opens no inbound listener. Each service uses a durable private state directory, which must survive upgrades: it holds the agent identity, scoped publishing credential, collector URL, and assigned node ID. Do not clone this directory onto another device.

Pair in the collector's **Nodes → Pair device** view and put its single-use key into a private file. First run:

```sh
glimdock-agent --collector-url https://collector.example.com \
  --state-dir /absolute/private/agent-state \
  --enrollment-key-file /absolute/private/enrollment.key \
  --config /absolute/path/host.json
```

Copy `examples/agents/push-host.json` for automatic native OS detection; its blank name uses the machine hostname. A stable node ID is assigned by the collector. After successful pairing the key file may be deleted. Keeping its argument in the service is safe: enrolled agents use saved credentials and do not reopen the pairing file. For HTTPS with a private CA append `--collector-ca-cert /absolute/path/ca.pem`. For unencrypted HTTP on a trusted LAN append `--allow-insecure-http`. The URL must identify the collector root and must not include credentials.

To recover after publishing access is revoked, issue **Re-pair** for the existing node in the console, stop the agent service, and run the same command once with `--re-enroll` and the new `--enrollment-key-file`. The agent retains its stable identity, rotates its publishing credential, and binds to the node targeted by the new grant. Remove `--re-enroll` after successful pairing and restart the service; do not add it to permanent startup arguments. If the private state was lost, enroll from a fresh state directory with a grant for that existing node.

Use `--snmp-config /absolute/path/snmp.json` or `--json-config /absolute/path/json.json` instead of `--config` to push readings gathered by a native adapter. Such adapters still need to query the target router or appliance; their connection to the central collector is outbound. Give each device/adapter its own state directory and pairing key.

## Linux

Install the binary at `/usr/local/bin/glimdock-agent`. Create a dedicated `glimdock-agent` system user and `/etc/glimdock-agent` directory. Put the host configuration and pairing key there, owned by the service user; use directory mode `0700` and key mode `0600`. Copy `glimdock-agent.service` to `/etc/systemd/system/`, replace its collector URL, then run `systemctl daemon-reload` and `systemctl enable --now glimdock-agent`. Systemd creates the private state directory. Read status with `systemctl status glimdock-agent` and logs with `journalctl -u glimdock-agent`. Adapter instances need separate service names and state directories.

## macOS

Install the binary at the absolute path in `com.glimdock.agent.plist`. Replace every placeholder path and the collector URL, copy the plist into `~/Library/LaunchAgents/`, and create the referenced host configuration and private pairing key. Use `chmod 700` on the Glimdock configuration directory and `chmod 600` on the pairing file. Load with `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.glimdock.agent.plist`. The agent runs while that user is logged in. Stop with `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.glimdock.agent.plist`.

## Windows

Put the executable, `host.json`, and private `enrollment.key` in `%LOCALAPPDATA%\Glimdock`. Run `install-windows-task.ps1 -CollectorUrl https://collector.example.com` from PowerShell. The script registers a Task Scheduler task for the current logged-in user, restricts state and enrollment file ACLs to that user, and starts it. The native agent also creates a protected DACL for its state directory. Use Task Scheduler's **Glimdock Agent** entry to inspect or stop it. To remove it, run `Unregister-ScheduledTask -TaskName 'Glimdock Agent' -Confirm:$false`. The task runs at user logon; install it separately for the account that owns this node.

The collector controls sampling cadence after pairing; accepted push replies update it. During an outage sampling continues, retries use bounded jittered backoff, and only the latest reading is retained in memory. Expired readings are dropped. SIGTERM or Ctrl+C stops without replaying a telemetry backlog. `--serve-http --token-file FILE` remains available for older collectors that poll an agent; `--once` prints one schema-1 sample for integrations.
