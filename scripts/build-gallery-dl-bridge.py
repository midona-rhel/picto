#!/usr/bin/env python3
"""Build Picto's self-contained gallery-dl subscription sidecar.

The bridge is frozen in an isolated temporary environment so packaged builds do
not depend on Python, gallery-dl, or pip being installed on the user's machine.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BRIDGE = ROOT / "scripts" / "gallery_dl_bridge.py"
REQUIREMENTS = ROOT / "scripts" / "gallery-dl-bridge-requirements.txt"
DEFAULT_OUTPUT = ROOT / "vendor" / "gallery-dl"


def executable_name(platform_name: str | None = None) -> str:
    """Return the sidecar name for the target platform."""
    name = platform_name or sys.platform
    return "picto-gallery-dl-bridge.exe" if name.startswith("win") else "picto-gallery-dl-bridge"


def venv_python(venv_dir: Path) -> Path:
    return venv_dir / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def command_for(venv_dir: Path, output_dir: Path, platform_name: str | None = None) -> list[str]:
    """Build the PyInstaller command without executing it."""
    name = executable_name(platform_name)
    work_dir = venv_dir.parent / "pyinstaller-work"
    return [
        str(venv_python(venv_dir)),
        "-m",
        "PyInstaller",
        "--clean",
        "--noconfirm",
        "--onefile",
        "--name",
        name.removesuffix(".exe"),
        "--distpath",
        str(output_dir),
        "--workpath",
        str(work_dir),
        "--specpath",
        str(work_dir),
        "--collect-all",
        "gallery_dl",
        "--collect-all",
        "requests",
        "--collect-all",
        "yt_dlp",
        str(BRIDGE),
    ]


def run(command: list[str], *, cwd: Path | None = None) -> None:
    print("+", " ".join(command))
    subprocess.run(command, cwd=cwd, check=True)


def build(output_dir: Path, platform_name: str | None, dry_run: bool) -> Path:
    if not BRIDGE.is_file():
        raise SystemExit(f"Missing bridge source: {BRIDGE}")
    if not REQUIREMENTS.is_file():
        raise SystemExit(f"Missing bridge requirements: {REQUIREMENTS}")

    output_dir = output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    output_path = output_dir / executable_name(platform_name)

    with tempfile.TemporaryDirectory(prefix="picto-gallery-dl-bridge-") as temp:
        temp_dir = Path(temp)
        venv_dir = temp_dir / "venv"
        staging_dir = temp_dir / "dist"

        if dry_run:
            print(f"output: {output_path}")
            print(f"requirements: {REQUIREMENTS}")
            print("+", sys.executable, "-m venv", venv_dir)
            print("+", venv_python(venv_dir), "-m pip install -r", REQUIREMENTS)
            print("+", " ".join(command_for(venv_dir, staging_dir, platform_name)))
            return output_path

        run([sys.executable, "-m", "venv", str(venv_dir)])
        builder_python = venv_python(venv_dir)
        run(
            [
                str(builder_python),
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--requirement",
                str(REQUIREMENTS),
            ]
        )
        staging_dir.mkdir()
        run(command_for(venv_dir, staging_dir, platform_name))

        built_path = staging_dir / executable_name(platform_name)
        if not built_path.is_file():
            raise SystemExit(f"PyInstaller did not produce {built_path}")

        output_path.unlink(missing_ok=True)
        shutil.copy2(built_path, output_path)
        if os.name != "nt":
            output_path.chmod(output_path.stat().st_mode | 0o111)

    print(f"Built {output_path}")
    return output_path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_OUTPUT,
        help="directory receiving the sidecar (default: vendor/gallery-dl)",
    )
    parser.add_argument(
        "--platform",
        choices=("linux", "darwin", "win32"),
        help="override the output suffix for a platform-specific build",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print the isolated build commands without installing or building",
    )
    args = parser.parse_args()
    build(args.output_dir, args.platform, args.dry_run)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
