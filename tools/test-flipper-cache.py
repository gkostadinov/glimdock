#!/usr/bin/env python3
"""Run all-node cache and protocol checks as C99 and C++17 without hardware."""
import os
from pathlib import Path
import shlex
import subprocess
import tempfile

def main():
    root = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="glimdock-cache-test-") as temporary:
        for variable, default, language, standard in (("CC", "cc", "c", "c99"), ("CXX", "c++", "c++", "c++17")):
            binary = Path(temporary) / ("test-" + language)
            subprocess.run([*shlex.split(os.environ.get(variable, default)), "-x", language, "-std=" + standard,
                            "-Wall", "-Wextra", "-Werror", "-Ifirmware/src", "-Iintegrations/flipper",
                            "tools/flipper-test-cache.c", "-o", str(binary)], cwd=root, check=True)
            subprocess.run([str(binary)], cwd=root, check=True)
if __name__ == "__main__":
    main()
