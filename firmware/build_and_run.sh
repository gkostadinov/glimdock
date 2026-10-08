#!/usr/bin/env bash
set -euo pipefail
firmware_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
command -v pio >/dev/null || { printf '%s\n' 'Install PlatformIO first.' >&2; exit 1; }
case "${1:-build}" in
  build) pio run --project-dir "$firmware_dir" --environment homelab_s3 ;;
  devices) pio device list ;;
  upload) [[ -n "${2:-}" ]] || { printf '%s\n' 'Supply the physical USB port.' >&2; exit 2; }; pio run --project-dir "$firmware_dir" --environment homelab_s3 --target upload --upload-port "$2" ;;
  *) printf '%s\n' 'Usage: build_and_run.sh build|devices|upload PORT' >&2; exit 2 ;;
esac
