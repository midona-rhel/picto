#!/usr/bin/env python3
import hashlib
import argparse
import json
from pathlib import Path


parser = argparse.ArgumentParser()
parser.add_argument("root", type=Path)
parser.add_argument("--slug", action="append", dest="slugs")
args = parser.parse_args()
root = args.root
manifest = json.loads(
    (Path(__file__).with_name("coreml-artifacts.json")).read_text()
)
for slug, artifact in manifest["assets"].items():
    if args.slugs and slug not in args.slugs:
        continue
    path = root / artifact["url"].rsplit("/", 1)[-1]
    if not path.is_file():
        raise SystemExit(f"missing Core ML artifact: {path}")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != artifact["sha256"] or path.stat().st_size != artifact["size"]:
        raise SystemExit(f"Core ML artifact does not match registry: {path}")
    print(f"verified {slug}: {digest}")
