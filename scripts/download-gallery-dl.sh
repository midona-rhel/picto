#!/usr/bin/env bash
# Download gallery-dl binary for the current platform.
# Source: https://codeberg.org/mikf/gallery-dl/releases
#
# - Windows x64:  standalone .exe
# - Linux x64:    standalone .bin
# - macOS:        Python wheel + wrapper script (requires python3)
#
# Usage:
#   bash scripts/download-gallery-dl.sh            # auto-detect platform
#   bash scripts/download-gallery-dl.sh darwin-arm64  # force platform

set -euo pipefail

VERSION="1.32.9"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNTIME_REQUIREMENTS="${SCRIPT_DIR}/gallery-dl-runtime-requirements.txt"
# Release assets moved to Codeberg from v1.32.0 (GitHub is a source mirror only).
BASE_URL="https://codeberg.org/mikf/gallery-dl/releases/download/v${VERSION}"
DEST_DIR="vendor/gallery-dl"

# ── Detect platform ──────────────────────────────────────────────────────
detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin)
      case "$arch" in
        arm64) echo "darwin-arm64" ;;
        x86_64) echo "darwin-x64" ;;
        *) echo "Unsupported macOS arch: $arch" >&2; exit 1 ;;
      esac
      ;;
    Linux)
      case "$arch" in
        x86_64) echo "linux-x64" ;;
        *) echo "Unsupported Linux arch: $arch" >&2; exit 1 ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      echo "win32-x64"
      ;;
    *)
      echo "Unsupported OS: $os" >&2; exit 1
      ;;
  esac
}

PLATFORM="${1:-$(detect_platform)}"
echo "Platform: $PLATFORM"

mkdir -p "$DEST_DIR"

download() {
  local url="$1" dest="$2"
  if [ -f "$dest" ]; then
    echo "  Already exists: $dest (delete to re-download)"
    return
  fi
  echo "  Downloading: $url"
  curl -fSL --progress-bar -o "$dest" "$url"
}

# ── Platform-specific download ───────────────────────────────────────────

case "$PLATFORM" in
  win32-x64)
    echo "Downloading gallery-dl.exe..."
    download "${BASE_URL}/gallery-dl.exe" "${DEST_DIR}/gallery-dl.exe"
    echo "Done."
    ;;

  linux-x64)
    echo "Downloading gallery-dl.bin..."
    download "${BASE_URL}/gallery-dl.bin" "${DEST_DIR}/gallery-dl"
    chmod +x "${DEST_DIR}/gallery-dl"
    echo "Done."
    ;;

  darwin-*)
    # macOS debug runs the source bridge against the exact same pinned Python
    # dependencies as the packaged sidecar.
    WHEEL_DIR="${DEST_DIR}/wheel"

    if command -v python3 >/dev/null 2>&1; then
      echo "Installing pinned gallery-dl development runtime..."
      mkdir -p "$WHEEL_DIR"
      python3 -m pip install --disable-pip-version-check --quiet --upgrade \
        --target "$WHEEL_DIR" --requirement "$RUNTIME_REQUIREMENTS"
    else
      echo "  Warning: python3 not found; gallery-dl wrapper will not run."
    fi

    # Create wrapper script
    cat > "${DEST_DIR}/gallery-dl" << 'WRAPPER'
#!/usr/bin/env bash
# gallery-dl wrapper — runs from the bundled Python wheel.
DIR="$(cd "$(dirname "$0")" && pwd)"
export PYTHONPATH="${DIR}/wheel${PYTHONPATH:+:$PYTHONPATH}"
exec python3 -m gallery_dl "$@"
WRAPPER
    chmod +x "${DEST_DIR}/gallery-dl"

    echo "Done. Wrapper + wheel in ${DEST_DIR}/"
    ;;

  *)
    echo "Unsupported platform: $PLATFORM" >&2
    exit 1
    ;;
esac

ls -lh "${DEST_DIR}/"
