#!/usr/bin/env python3
"""Build Picto's self-contained OF-Scraper sidecar."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BRIDGE = ROOT / "scripts" / "onlyfans_bridge.py"
REQUIREMENTS = ROOT / "scripts" / "onlyfans-bridge-requirements.txt"
LICENSE_NOTICES = ROOT / "scripts" / "python_license_notices.py"
DEFAULT_OUTPUT = ROOT / "vendor" / "onlyfans"


def name(platform: str | None = None) -> str:
    return "picto-onlyfans-bridge.exe" if (platform or sys.platform).startswith("win") else "picto-onlyfans-bridge"


def python(venv: Path) -> Path:
    return venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def compatible_python() -> str:
    candidates = [
        os.environ.get("PICTO_ONLYFANS_PYTHON"),
        "python3.13",
        "python3.12",
        "python3.11",
        "python",
    ]
    for candidate in filter(None, candidates):
        executable = shutil.which(candidate)
        if not executable:
            continue
        version = subprocess.run(
            [executable, "-c", "import sys; print(int((3, 11) <= sys.version_info < (3, 14)))"],
            capture_output=True,
            text=True,
        )
        if version.returncode == 0 and version.stdout.strip() == "1":
            return executable
    raise SystemExit("OF-Scraper requires Python 3.11, 3.12, or 3.13 to build")


def run(command: list[str]) -> None:
    print("+", " ".join(command))
    subprocess.run(command, check=True)


def build(output: Path, platform: str | None, dry_run: bool) -> Path:
    output.mkdir(parents=True, exist_ok=True)
    target = output.resolve() / name(platform)
    with tempfile.TemporaryDirectory(prefix="picto-onlyfans-bridge-") as temporary:
        root = Path(temporary)
        venv = root / "venv"
        dist = root / "dist"
        commands = [
            [compatible_python(), "-m", "venv", str(venv)],
            [str(python(venv)), "-m", "pip", "install", "--disable-pip-version-check", "-r", str(REQUIREMENTS)],
            [str(python(venv)), "-m", "PyInstaller", "--clean", "--noconfirm", "--onefile",
             "--name", name(platform).removesuffix(".exe"), "--distpath", str(dist),
             "--workpath", str(root / "work"), "--specpath", str(root / "work"),
             "--collect-all", "ofscraper", str(BRIDGE)],
            [str(python(venv)), str(LICENSE_NOTICES), str(output.resolve() / "THIRD_PARTY_LICENSES.txt")],
        ]
        if dry_run:
            for command in commands: print("+", " ".join(command))
            return target
        for command in commands: run(command)
        built = dist / name(platform)
        if not built.is_file(): raise SystemExit(f"PyInstaller did not produce {built}")
        target.unlink(missing_ok=True)
        shutil.copyfile(built, target)
        if os.name != "nt": target.chmod(target.stat().st_mode | 0o111)
        if sys.platform == "darwin":
            run(["codesign", "--force", "--sign", "-", str(target)])
    return target


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--platform", choices=("linux", "darwin", "win32"))
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    build(args.output_dir, args.platform, args.dry_run)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
