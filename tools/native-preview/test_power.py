#!/usr/bin/env python3
"""Check the production Power button gesture policy without powering down hardware."""
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix='glimdock-power-') as temporary:
    binary = Path(temporary)/'power-test'
    subprocess.run([shutil.which('c++') or 'c++', '-std=c++17', '-Wall', '-Wextra',
                    '-Werror', '-fsanitize=address,undefined', '-I', str(root/'firmware/src'),
                    str(root/'tools/native-preview/power_test.cpp'), '-o', str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
