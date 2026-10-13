#!/usr/bin/env python3
"""Verify the production LVGL interface compiled to WebAssembly."""
import os,subprocess
from pathlib import Path
root=Path(__file__).resolve().parents[2]
for test in ('tools/web-emulator/test-operations.mjs','tools/web-emulator/test.mjs'):
    subprocess.run(['node',test],cwd=root,check=True)
