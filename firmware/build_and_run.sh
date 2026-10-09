#!/usr/bin/env bash
set -euo pipefail
firmware_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if command -v pio >/dev/null 2>&1; then
  pio_command="$(command -v pio)"
elif [[ -x ${HOME}/.miniconda3/bin/pio ]]; then
  pio_command=${HOME}/.miniconda3/bin/pio
else
  printf '%s\n' 'PlatformIO is required: python3 -m pip install platformio' >&2
  exit 1
fi
mode="${1:-build}"
select_environment() {
  build_environment="${1:-homelab_s3_landscape3}"
  case "$build_environment" in
    homelab_s3|homelab_s3_portrait0|homelab_s3_portrait2|homelab_s3_landscape1|homelab_s3_landscape3|homelab_s3_web|homelab_s3_portrait0_color_test) ;;
    *) printf '%s\n' 'Select a declared homelab_s3 build environment from platformio.ini.' >&2; exit 2 ;;
  esac
}
case "$mode" in
  build)
    select_environment "${2:-}"
    "$pio_command" run --project-dir "$firmware_dir" --environment "$build_environment"
    build_output_dir="$firmware_dir/build/$build_environment"
    mkdir -p "$build_output_dir"
    cp "$firmware_dir/.pio/build/$build_environment/firmware.bin" "$build_output_dir/glimdock-monitor-v1.bin"
    cp "$firmware_dir/.pio/build/$build_environment/bootloader.bin" "$build_output_dir/bootloader.bin"
    cp "$firmware_dir/.pio/build/$build_environment/partitions.bin" "$build_output_dir/partitions.bin"
    chmod 600 "$build_output_dir/glimdock-monitor-v1.bin" "$firmware_dir/.pio/build/$build_environment/firmware.bin" "$firmware_dir/.pio/build/$build_environment/firmware.elf"
    printf '\nBuild ready: %s\n' "$build_output_dir/glimdock-monitor-v1.bin"
    printf '%s\n' "To flash, select the physical display and orientation explicitly: ./build_and_run.sh upload /dev/cu.usbmodem... $build_environment"
    ;;
  upload)
    if [[ -z "${2:-}" ]]; then
      printf '%s\n' 'An explicit serial port is required to protect the BabyStory board. Use: ./build_and_run.sh devices' >&2
      exit 2
    fi
    select_environment "${3:-}"
    "$pio_command" run --project-dir "$firmware_dir" --environment "$build_environment" --target upload --upload-port "$2"
    ;;
  monitor)
    if [[ -z "${2:-}" ]]; then
      printf '%s\n' 'Select a serial port: ./build_and_run.sh monitor /dev/cu.usbmodem...' >&2
      exit 2
    fi
    "$pio_command" device monitor --port "$2" --baud 115200
    ;;
  devices) "$pio_command" device list ;;
  *) printf '%s\n' 'Usage: ./build_and_run.sh [build [ENV] | devices | upload PORT [ENV] | monitor PORT]' >&2; exit 2 ;;
esac
