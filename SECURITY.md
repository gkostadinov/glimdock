# Security

Glimdock is an early local-network monitoring project. Only the newest developer
release is maintained. The HTTP reader is not intended for direct internet exposure.

Report vulnerabilities privately through [GitHub's security advisory form](https://github.com/gkostadinov/glimdock/security/advisories/new).
Private vulnerability reporting is enabled for the public repository. Use that
private form for sensitive vulnerability details.
Do not open a public issue containing exploitable credentials or a private snapshot.

Include the version, relevant platform, steps to reproduce and a synthetic example.
Remove Wi-Fi credentials, Bearer/API tokens, SSH keys and personal hostnames/addresses.
Read and setup credentials must remain distinct. The setup token permits edits to
monitor node configuration; keep it off displays intended only for reading.

Rotate exposed tokens in the collector's root-only environment file and update
the display. Delete obsolete managed node secrets. HTTPS requires a verified CA;
plain HTTP should stay within a trusted LAN. SSH host keys must be pinned and NAS
monitor keys should be restricted to the fixed read-only helper.
