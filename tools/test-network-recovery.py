#!/usr/bin/env python3
"""Exercise the production response reader and recovery policy without hardware."""
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
compiler = shutil.which('c++') or shutil.which('g++')
if not compiler:
    raise SystemExit('A C++17 compiler is required')
with tempfile.TemporaryDirectory(prefix='glimdock-network-') as temporary:
    binary = Path(temporary)/'network-recovery'
    subprocess.run([compiler, '-std=c++17', '-Wall', '-Wextra', '-Werror',
                    '-fsanitize=address,undefined', '-I', str(root/'firmware/src'),
                    str(root/'tools/test-network-recovery.cpp'), '-o', str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
