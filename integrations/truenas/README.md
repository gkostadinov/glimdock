# Optional TrueNAS SSH probe

`probe.py` is a fixed read-only NAS-side compatibility adapter. The central
collector is native Rust. This optional helper runs on the NAS and reports
middleware pool, disk, memory and alert data over the existing restricted SSH
integration; it is independent of the ordinary OS/SNMP/API device agents.

Use `tools/configure_truenas_ssh.py` only when configuring this specific integration.
The generic collector does not require a NAS or an SSH probe.
