#!/bin/sh
# Review this file, then run on the Proxmox node as root:
#   ./deploy/install.sh                  # installs, does not start services
#   ./deploy/install.sh --start          # also enables/starts monitor services
# No package installs, firewall changes, or storage/guest configuration changes
# occur. Existing configuration/token files are retained on subsequent runs.
# The collector uses read-only probes; only it runs as root. The HTTP service
# receives a display-only random token, with no Proxmox credentials or actions.
# Copy the token into the ESP32's local secrets file via a private channel.
# Read it deliberately: python3 -c 'import pathlib; print(next(line.split("=",1)[1]
# for line in pathlib.Path("/etc/homelab-monitor/server.env").read_text().splitlines()
# if line.startswith("HOMELAB_DISPLAY_TOKEN=")))'
set -eu

START=false
if [ "$#" -gt 1 ]; then
    printf '%s\n' 'Usage: deploy/install.sh [--start]' >&2
    exit 2
fi
if [ "$#" -eq 1 ]; then
    if [ "$1" != '--start' ]; then
        printf '%s\n' 'Usage: deploy/install.sh [--start]' >&2
        exit 2
    fi
    START=true
fi
if [ "$(id -u)" -ne 0 ]; then
    printf '%s\n' 'Run this installer as root on the Proxmox node.' >&2
    exit 1
fi
command -v python3 >/dev/null
command -v systemctl >/dev/null
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SOURCE_DIR=$(dirname -- "$SCRIPT_DIR")

# Dedicated nologin identity: no reusable host/Proxmox login credentials.
if ! getent group homelab-monitor >/dev/null; then
    groupadd --system homelab-monitor
fi
if ! id homelab-monitor >/dev/null 2>&1; then
    useradd --system --gid homelab-monitor --home-dir /nonexistent --no-create-home --shell /usr/sbin/nologin homelab-monitor
fi
install -d -o root -g root -m 0755 /opt/homelab-monitor /opt/homelab-monitor/agent
install -d -o root -g root -m 0700 /etc/homelab-monitor
for filename in __init__.py collector.py server.py config_service.py remote_feeds.py faults.py guest_telemetry.py gpus.py printers.py truenas_probe.py demo.json demo-nodes.json; do
    install -o root -g root -m 0644 "$SOURCE_DIR/agent/$filename" "/opt/homelab-monitor/agent/$filename"
done
if [ ! -e /etc/homelab-monitor/config.json ]; then
    install -o root -g root -m 0600 "$SOURCE_DIR/agent/config.example.json" /etc/homelab-monitor/config.json
fi
if [ ! -e /etc/homelab-monitor/server.env ]; then
    python3 - <<'PY'
import os
import ipaddress
import json
from pathlib import Path
import secrets
path = Path('/etc/homelab-monitor/server.env')
config = json.loads(Path('/etc/homelab-monitor/config.json').read_text())
bind = str(ipaddress.ip_address(config.get('host_ip', '127.0.0.1')))
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, 'w') as stream:
    stream.write(f'HOMELAB_BIND={bind}\nHOMELAB_PORT=8765\n')
    stream.write('HOMELAB_DISPLAY_TOKEN=' + secrets.token_urlsafe(32) + '\n')
    stream.write('HOMELAB_SETUP_TOKEN=' + secrets.token_urlsafe(32) + '\n')
PY
fi
# Upgrades preserve all existing environment values, adding only a missing,
# separate setup credential. Neither token is printed by the installer.
python3 - <<'PY'
import os
from pathlib import Path
import secrets
import tempfile
path = Path('/etc/homelab-monitor/server.env')
raw = path.read_text()
if not any(line.startswith('HOMELAB_SETUP_TOKEN=') for line in raw.splitlines()):
    fd, temporary = tempfile.mkstemp(prefix='.server-env-', dir=path.parent)
    with os.fdopen(fd, 'w') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(raw.rstrip('\n') + '\nHOMELAB_SETUP_TOKEN=' + secrets.token_urlsafe(32) + '\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
PY
for service in homelab-monitor-config homelab-monitor-collector homelab-monitor-http; do
    install -o root -g root -m 0644 "$SCRIPT_DIR/$service.service" "/etc/systemd/system/$service.service"
done
systemctl daemon-reload
if [ "$START" = true ]; then
    systemctl enable --now homelab-monitor-config.service homelab-monitor-collector.service homelab-monitor-http.service
    # Restart also applies updated source files on subsequent installations.
    systemctl restart homelab-monitor-collector.service homelab-monitor-http.service
    printf '%s\n' 'Services active. Check: systemctl status homelab-monitor-config homelab-monitor-collector homelab-monitor-http'
else
    printf '%s\n' 'Installed. Review /etc/homelab-monitor/config.json and server.env, then:'
    printf '%s\n' 'systemctl enable --now homelab-monitor-config.service homelab-monitor-collector.service homelab-monitor-http.service'
fi
printf '%s\n' 'Separate display and setup tokens retained in /etc/homelab-monitor/server.env (root only).'
printf '%s\n' 'Health endpoint: http://<HOMELAB_BIND>:<HOMELAB_PORT>/healthz (values in server.env).'
