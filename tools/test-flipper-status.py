#!/usr/bin/env python3
"""Compile and run synthetic status protocol/projection checks without hardware."""
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parents[1]
    compiler = shlex.split(os.environ.get("CXX", "c++"))
    if not shutil.which(compiler[0]):
        raise SystemExit("A C++17 compiler is required (set CXX if needed).")
    with tempfile.TemporaryDirectory(prefix="glimdock-flipper-test-") as temporary:
        binary = Path(temporary) / "flipper-status-test"
        subprocess.run([
            *compiler, "-std=c++17", "-Wall", "-Wextra", "-Werror",
            "-DHOMELAB_NATIVE_PREVIEW", "-Itools/native-preview/shims", "-Ifirmware/src",
            "tools/flipper-test-status.cpp", "-o", str(binary),
        ], cwd=root, check=True)
        subprocess.run([str(binary)], cwd=root, check=True)


if __name__ == "__main__":
    main()
