#!/usr/bin/env python3
"""Install a fixed read-only TrueNAS probe and a restricted Proxmox SSH key.

Requires already authorized SSH logins from this workstation to both machines.
The private key is generated on Proxmox and never leaves it. Existing NAS keys
are preserved, with one backup before the first addition. No sudo rules, NAS
services, pools, packages or guest configuration are changed.
"""
import argparse
from pathlib import Path
import shlex
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SSH = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=yes"]

def run(argv, **kwargs):
    return subprocess.run(argv, check=True, capture_output=True, **kwargs)

def remote(host, command, payload=None):
    return run([*SSH, host, command], input=payload).stdout

def put_file(host, target, payload, mode=0o600):
    script = "from pathlib import Path; import sys,os; p=Path(" + repr(target) + "); p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(sys.stdin.buffer.read()); os.chmod(p," + str(mode) + ")"
    remote(host, "/usr/bin/python3 -c " + shlex.quote(script), payload)

def configure(pve, nas, source_ip, nas_ip):
    key_path = "/etc/homelab-monitor/truenas_ed25519"
    remote(pve, "install -d -m 0700 /etc/homelab-monitor; if [ ! -f " + shlex.quote(key_path) + " ]; then ssh-keygen -q -t ed25519 -N '' -C homelab-monitor-pve -f " + shlex.quote(key_path) + "; fi")
    public_key = remote(pve, "cat " + shlex.quote(key_path + ".pub")).decode().strip()
    fields = public_key.split()
    if len(fields) < 2 or fields[0] != "ssh-ed25519":
        raise ValueError("Invalid dedicated public key")
    # Reuse the host key already verified by StrictHostKeyChecking on this Mac.
    found = run(["ssh-keygen", "-F", nas_ip]).stdout.decode()
    trusted = []
    for line in found.splitlines():
        if line and not line.startswith("#"):
            parts = line.split()
            if len(parts) >= 3:
                trusted.append(nas_ip + " " + parts[1] + " " + parts[2])
    if not trusted:
        raise ValueError("No previously verified TrueNAS host key found locally")
    put_file(pve, "/etc/homelab-monitor/truenas_known_hosts", ("\n".join(trusted) + "\n").encode())
    home = remote(nas, "printf '%s' \"$HOME\"").decode()
    if not home.startswith("/") or any(c in home for c in '\r\n"'):
        raise ValueError("Unexpected NAS account home path")
    helper = home + "/.local/share/homelab-monitor/truenas_probe.py"
    put_file(nas, helper, (ROOT / "integrations/truenas/probe.py").read_bytes(), 0o700)
    line = 'restrict,from="' + source_ip + '",command="/usr/bin/python3 ' + helper + '" ' + public_key
    script = '''from pathlib import Path
import os,shutil,sys
p=Path.home()/'.ssh'/'authorized_keys'
p.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
original=p.read_text() if p.exists() else ''
line=sys.stdin.read().strip()
identity=line.split()[-2]
if identity not in original:
    backup=p.with_name('authorized_keys.before-homelab-monitor')
    if p.exists() and not backup.exists(): shutil.copy2(p,backup)
    p.write_text(original.rstrip()+'\\n'+line+'\\n')
    os.chmod(p,0o600)
'''
    remote(nas, "/usr/bin/python3 -c " + shlex.quote(script), line.encode())
    # Validate forced-command behavior and structure without printing metrics.
    result = remote(pve, "ssh -T -i " + shlex.quote(key_path) + " -o BatchMode=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=/etc/homelab-monitor/truenas_known_hosts -o ConnectTimeout=5 -o IdentitiesOnly=yes " + shlex.quote(nas) + " snapshot")
    import json
    snapshot = json.loads(result)
    if snapshot.get("schema") != 1 or snapshot.get("error"):
        raise ValueError("TrueNAS probe did not produce a valid complete snapshot")
    print(f"TrueNAS read-only feed verified: {len(snapshot['pools'])} pools, {len(snapshot['disks'])} physical disks. Private key stayed on Proxmox.")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proxmox", default="root@192.0.2.10")
    parser.add_argument("--nas", default="truenas_admin@192.0.2.10")
    parser.add_argument("--source-ip", default="192.0.2.10")
    parser.add_argument("--nas-ip", default="192.0.2.10")
    args = parser.parse_args()
    configure(args.proxmox, args.nas, args.source_ip, args.nas_ip)
