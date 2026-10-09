#!/bin/sh
# Compatibility entry point; all current installs use the native Rust runtime.
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$SCRIPT_DIR/install.sh" "$@"
