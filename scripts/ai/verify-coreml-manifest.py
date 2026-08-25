#!/usr/bin/env python3
import hashlib
import json
import sys
from pathlib import Path


root = Path(sys.argv[1])
manifest = json.loads(
    (Path(__file__).with_name("coreml-artifacts.json")).read_text()
)
for slug, artifact in manifest["assets"].items():
    path = root / artifact["url"].rsplit("/", 1)[-1]
    if not path.is_file():
        raise SystemExit(f"missing Core ML artifact: {path}")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != artifact["sha256"] or path.stat().st_size != artifact["size"]:
        raise SystemExit(f"Core ML artifact does not match registry: {path}")
    print(f"verified {slug}: {digest}")
