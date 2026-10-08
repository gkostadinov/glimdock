#!/bin/sh
# Install the Rust binary and compatible service names; preserve existing secrets.
# Optional --start activates/restarts all three services. No package/firewall edits.
set -eu
START=false
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SOURCE_DIR=$(dirname -- "$SCRIPT_DIR")
BINARY="$SOURCE_DIR/bin/glimdock-collector"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --start) START=true; shift ;;
        --binary) [ "$#" -ge 2 ] || { printf '%s\n' '--binary needs a file.' >&2; exit 2; }; BINARY=$2; shift 2 ;;
        *) printf '%s\n' 'Usage: deploy/install-rust.sh [--binary FILE] [--start]' >&2; exit 2 ;;
    esac
done
[ "$(id -u)" -eq 0 ] || { printf '%s\n' 'Run as root on the Linux collector host.' >&2; exit 1; }
[ "$(uname -s)" = Linux ] || { printf '%s\n' 'Linux is required.' >&2; exit 1; }
command -v systemctl >/dev/null
[ -f "$BINARY" ] && [ -x "$BINARY" ] || { printf '%s\n' 'Supply the matching Linux release binary with --binary.' >&2; exit 1; }
"$BINARY" --version
if ! getent group homelab-monitor >/dev/null; then groupadd --system homelab-monitor; fi
if ! id homelab-monitor >/dev/null 2>&1; then
    useradd --system --gid homelab-monitor --home-dir /nonexistent --no-create-home --shell /usr/sbin/nologin homelab-monitor
fi
install -d -o root -g root -m 0755 /opt/homelab-monitor
install -d -o root -g root -m 0700 /etc/homelab-monitor
if [ ! -e /etc/homelab-monitor/config.json ]; then
    install -o root -g root -m 0600 "$SOURCE_DIR/agent/config.example.json" /etc/homelab-monitor/config.json
fi
"$BINARY" validate-config --config /etc/homelab-monitor/config.json
umask 077
random_token() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
if [ ! -e /etc/homelab-monitor/server.env ]; then
    # Fresh installs are deliberately loopback-only until the owner chooses a LAN bind.
    (set -C; {
        printf '%s\n' 'HOMELAB_BIND=127.0.0.1' 'HOMELAB_PORT=8765'
        printf 'HOMELAB_DISPLAY_TOKEN=%s\n' "$(random_token)"
        printf 'HOMELAB_SETUP_TOKEN=%s\n' "$(random_token)"
    } > /etc/homelab-monitor/server.env)
elif ! rg_setup=$(sed -n '/^HOMELAB_SETUP_TOKEN=/p' /etc/homelab-monitor/server.env) || [ -z "$rg_setup" ]; then
    ENV_TEMP=$(mktemp /etc/homelab-monitor/.server-env.XXXXXX)
    trap 'rm -f "$ENV_TEMP"' EXIT HUP INT TERM
    cat /etc/homelab-monitor/server.env > "$ENV_TEMP"
    printf '\nHOMELAB_SETUP_TOKEN=%s\n' "$(random_token)" >> "$ENV_TEMP"
    chmod 0600 "$ENV_TEMP"
    mv -f "$ENV_TEMP" /etc/homelab-monitor/server.env
    trap - EXIT HUP INT TERM
fi
install -o root -g root -m 0755 "$BINARY" /opt/homelab-monitor/.glimdock-collector.new
mv -f /opt/homelab-monitor/.glimdock-collector.new /opt/homelab-monitor/glimdock-collector
for service in homelab-monitor-config homelab-monitor-collector homelab-monitor-http; do
    install -o root -g root -m 0644 "$SCRIPT_DIR/rust/$service.service" "/etc/systemd/system/$service.service"
done
systemctl daemon-reload
if [ "$START" = true ]; then
    systemctl enable homelab-monitor-config.service homelab-monitor-collector.service homelab-monitor-http.service
    systemctl restart homelab-monitor-config.service homelab-monitor-collector.service homelab-monitor-http.service
    printf '%s\n' 'Rust services active. Inspect their systemctl status and /healthz.'
else
    printf '%s\n' 'Installed without starting services. Review config.json and server.env, then follow SETUP.md.'
fi
printf '%s\n' 'Existing configuration and tokens retained. Credentials were not printed.'
