"""Generate license notices for every Python distribution frozen into a sidecar."""

from __future__ import annotations

import argparse
import importlib.metadata
from pathlib import Path


LICENSE_PREFIXES = ("license", "licence", "copying", "notice", "copyright")


def _license_files(distribution: importlib.metadata.Distribution) -> list[Path]:
    files: list[Path] = []
    for relative in distribution.files or ():
        name = Path(str(relative)).name.lower()
        if not name.startswith(LICENSE_PREFIXES):
            continue
        located = Path(distribution.locate_file(relative))
        if located.is_file():
            files.append(located)
    return sorted(set(files))


def write_python_license_notices(destination: Path) -> None:
    sections = [
        "Picto bundled Python sidecar dependency notices",
        "Generated from the exact isolated environment frozen by PyInstaller.",
        "",
    ]
    distributions = sorted(
        importlib.metadata.distributions(),
        key=lambda item: (item.metadata.get("Name", "").lower(), item.version),
    )
    for distribution in distributions:
        name = distribution.metadata.get("Name") or "unknown"
        expression = distribution.metadata.get("License-Expression")
        declared = expression or distribution.metadata.get("License") or "UNKNOWN"
        if declared == "UNKNOWN":
            classifiers = [
                value.removeprefix("License :: ")
                for value in distribution.metadata.get_all("Classifier", ())
                if value.startswith("License :: ")
            ]
            declared = "; ".join(classifiers) or "UNKNOWN"
        if declared == "UNKNOWN":
            raise RuntimeError(f"Python distribution {name}@{distribution.version} has no declared license")
        sections.extend(("-" * 80, f"{name}@{distribution.version}", f"License: {declared}", "-" * 80))
        files = _license_files(distribution)
        if not files:
            sections.append("The installed distribution contains no separate license file.")
        for license_file in files:
            sections.extend((license_file.name, "", license_file.read_text(encoding="utf-8", errors="replace")))
        sections.append("")
    destination.write_text("\n".join(sections), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    write_python_license_notices(args.destination)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
