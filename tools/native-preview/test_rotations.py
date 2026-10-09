#!/usr/bin/env python3
"""Validate all V1 touch rotations without opening serial ports or networking."""
from pathlib import Path
import argparse
import hashlib
import json
import subprocess
from render import ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', default='c++')
    parser.add_argument('--output', type=Path, default=ROOT/'output/native-rotations')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).with_name('rotation_test.cpp')
    results = []
    for rotation in range(4):
        binary = args.output/f'rotation-{rotation}'
        subprocess.run([args.compiler, '-std=c++17', '-Wall', '-Wextra', '-Werror',
                        f'-DHOMELAB_ROTATION={rotation}', '-I'+str(ROOT/'firmware/src'),
                        '-I'+str(ROOT/'firmware/include'), str(source), '-o', str(binary)], check=True)
        result = subprocess.run([str(binary.resolve())], capture_output=True, text=True, check=True)
        print(result.stdout, end='')
        results.append({'rotation': rotation, 'passed': True, 'result': result.stdout.strip()})
    (args.output/'provenance.json').write_text(json.dumps({
        'method': 'C++17 exhaustive bijection, explicit corners, invalid raw bounds and MADCTL assertions',
        'board_sha256': hashlib.sha256((ROOT/'firmware/src/board.h').read_bytes()).hexdigest(),
        'unique_panel_transformations_checked': 307200,
        'compile_time_rotations': results,
        'hardware_tested': False
    }, indent=2)+'\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
