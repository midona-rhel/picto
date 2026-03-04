#!/usr/bin/env bash
# Download gallery-dl binary for the current platform.
# Source: https://github.com/mikf/gallery-dl/releases
#
# - Windows x64:  standalone .exe
# - Linux x64:    standalone .bin
# - macOS:        Python wheel + wrapper script (requires python3)
#
# Usage:
#   bash scripts/download-gallery-dl.sh            # auto-detect platform
#   bash scripts/download-gallery-dl.sh darwin-arm64  # force platform

set -euo pipefail

VERSION="1.31.7"
REPO="mikf/gallery-dl"
BASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}"
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
    # macOS: no standalone binary available.
    # Download the Python wheel and create a wrapper script.
    WHL_NAME="gallery_dl-${VERSION}-py3-none-any.whl"
    WHEEL_DIR="${DEST_DIR}/wheel"

    echo "Downloading Python wheel..."
    download "${BASE_URL}/${WHL_NAME}" "${DEST_DIR}/${WHL_NAME}"

    # Extract wheel (it's a zip file) into wheel/ directory
    if [ ! -d "${WHEEL_DIR}/gallery_dl" ]; then
      echo "  Extracting wheel..."
      mkdir -p "$WHEEL_DIR"
      unzip -qo "${DEST_DIR}/${WHL_NAME}" -d "$WHEEL_DIR"
    else
      echo "  Wheel already extracted."
    fi

    # Ensure runtime deps are present inside wheel/ (the release wheel does not
    # bundle third-party deps like requests).
    if command -v python3 >/dev/null 2>&1; then
      echo "  Ensuring Python deps (requests)..."
      python3 -m pip install --disable-pip-version-check --quiet --target "$WHEEL_DIR" requests || \
        echo "  Warning: failed to preinstall requests; app will retry at runtime."
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
