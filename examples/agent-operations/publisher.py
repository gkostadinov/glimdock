#!/usr/bin/env python3
"""Validate and atomically publish an adapter-produced operations snapshot.

No source probing, timestamp rebasing or command execution happens here.
See public-docs/AGENT-INTEGRATION.md for lifecycle and privacy semantics.
"""
from __future__ import annotations
import importlib.util,json,os,tempfile
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('agent_ops_reference',ROOT/'tools/agent-operations/api.py')
reference=importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

def publish_snapshot(snapshot: dict, destination: str | Path) -> None:
    """Publish already sampled metadata without changing any source clock."""
    reference.validate(snapshot)
    payload=json.dumps(snapshot,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()
    target=Path(destination)
    target.parent.mkdir(parents=True,exist_ok=True)
    descriptor,name=tempfile.mkstemp(prefix='.'+target.name+'-',dir=target.parent)
    try:
        with os.fdopen(descriptor,'wb') as stream:
            stream.write(payload);stream.flush();os.fsync(stream.fileno())
        os.replace(name,target)
    finally:
        if Path(name).exists():Path(name).unlink()

if __name__=='__main__':
    raise SystemExit('Import publish_snapshot from your adapter; supply actual sampled metadata.')
