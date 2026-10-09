#!/usr/bin/env bash
# Build all public web/firmware assets before embedding them in the Rust runtime.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
mode=${1:-full}
case "$mode" in
  full)
    npm ci --prefix collector-web
    npm run build --prefix collector-web
    pio pkg install --project-dir firmware --environment homelab_s3
    python3 tools/build-web-firmware.py
    python3 tools/web-emulator/build.py
    ;;
  rust) ;; # Use already verified, checked-in public assets.
  *) printf '%s\n' 'Usage: tools/build-release.sh [full|rust] [TARGET]' >&2; exit 2 ;;
esac
target_args=()
if [[ -n ${2:-} ]]; then target_args=(--target "$2"); fi
cargo build --workspace --locked --release "${target_args[@]}"
printf '%s\n' 'Built Glimdock collector (console + firmware updates) and native device agent.'
