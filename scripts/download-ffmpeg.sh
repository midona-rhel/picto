#!/usr/bin/env bash
# Download pre-built GPL ffmpeg + ffprobe binaries for the current platform.
#
# Sources (clean GPL, no --enable-nonfree):
#   macOS:         https://ffmpeg.martin-riedl.de (GPL, signed & notarized)
#   Linux/Windows: https://github.com/BtbN/FFmpeg-Builds (GPL)
#
# Usage:
#   bash scripts/download-ffmpeg.sh              # auto-detect platform
#   bash scripts/download-ffmpeg.sh darwin-arm64  # force platform

set -euo pipefail

DEST_DIR="vendor/ffmpeg"

# BtbN release branch for Linux/Windows
BTBN_BRANCH="n7.1"
BTBN_BASE="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"

# martin-riedl redirect API for macOS (resolves to latest release build)
RIEDL_BASE="https://ffmpeg.martin-riedl.de/redirect/latest/macos"

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
    return 0
  fi
  echo "  Downloading: $url"
  curl -fSL --progress-bar -o "$dest" "$url"
}

# ── Platform-specific download ───────────────────────────────────────────

case "$PLATFORM" in
  darwin-arm64|darwin-x64)
    # martin-riedl.de: macOS builds (GPL, signed, notarized)
    # Files come as .zip containing the bare binary.
    if [ "$PLATFORM" = "darwin-arm64" ]; then
      RIEDL_ARCH="arm64"
    else
      RIEDL_ARCH="amd64"
    fi

    for bin in ffmpeg ffprobe; do
      if [ -f "${DEST_DIR}/${bin}" ]; then
        echo "  Already exists: ${DEST_DIR}/${bin} (delete to re-download)"
        continue
      fi
      echo "Downloading ${bin} (macOS ${RIEDL_ARCH})..."
      TMP_ZIP="${DEST_DIR}/${bin}.zip"
      curl -fSL --progress-bar -o "$TMP_ZIP" "${RIEDL_BASE}/${RIEDL_ARCH}/release/${bin}.zip"
      unzip -qo "$TMP_ZIP" -d "$DEST_DIR"
      rm -f "$TMP_ZIP"
      chmod +x "${DEST_DIR}/${bin}"
    done
    ;;

  linux-x64)
    # BtbN: Linux x64 GPL build (.tar.xz archive with bin/ directory)
    ARCHIVE_NAME="ffmpeg-${BTBN_BRANCH}-latest-linux64-gpl-${BTBN_BRANCH#n}.tar.xz"
    ARCHIVE_PATH="${DEST_DIR}/${ARCHIVE_NAME}"

    if [ -f "${DEST_DIR}/ffmpeg" ] && [ -f "${DEST_DIR}/ffprobe" ]; then
      echo "  Already exists: ${DEST_DIR}/ffmpeg (delete to re-download)"
    else
      echo "Downloading BtbN FFmpeg (linux64-gpl)..."
      download "${BTBN_BASE}/${ARCHIVE_NAME}" "$ARCHIVE_PATH"

      echo "  Extracting ffmpeg + ffprobe..."
      # Extract only bin/ffmpeg and bin/ffprobe from the archive
      STRIP_DIR="ffmpeg-${BTBN_BRANCH}-latest-linux64-gpl-${BTBN_BRANCH#n}"
      tar xf "$ARCHIVE_PATH" -C "$DEST_DIR" --strip-components=2 \
        "${STRIP_DIR}/bin/ffmpeg" "${STRIP_DIR}/bin/ffprobe"
      rm -f "$ARCHIVE_PATH"
      chmod +x "${DEST_DIR}/ffmpeg" "${DEST_DIR}/ffprobe"
    fi
    ;;

  win32-x64)
    # BtbN: Windows x64 GPL build (.zip archive with bin/ directory)
    ARCHIVE_NAME="ffmpeg-${BTBN_BRANCH}-latest-win64-gpl-${BTBN_BRANCH#n}.zip"
    ARCHIVE_PATH="${DEST_DIR}/${ARCHIVE_NAME}"

    if [ -f "${DEST_DIR}/ffmpeg.exe" ] && [ -f "${DEST_DIR}/ffprobe.exe" ]; then
      echo "  Already exists: ${DEST_DIR}/ffmpeg.exe (delete to re-download)"
    else
      echo "Downloading BtbN FFmpeg (win64-gpl)..."
      download "${BTBN_BASE}/${ARCHIVE_NAME}" "$ARCHIVE_PATH"

      echo "  Extracting ffmpeg.exe + ffprobe.exe..."
      STRIP_DIR="ffmpeg-${BTBN_BRANCH}-latest-win64-gpl-${BTBN_BRANCH#n}"
      # unzip doesn't have --strip-components, extract then move
      TMP_EXTRACT="${DEST_DIR}/_extract"
      mkdir -p "$TMP_EXTRACT"
      unzip -qo "$ARCHIVE_PATH" "${STRIP_DIR}/bin/ffmpeg.exe" "${STRIP_DIR}/bin/ffprobe.exe" -d "$TMP_EXTRACT"
      mv "$TMP_EXTRACT/${STRIP_DIR}/bin/ffmpeg.exe" "${DEST_DIR}/ffmpeg.exe"
      mv "$TMP_EXTRACT/${STRIP_DIR}/bin/ffprobe.exe" "${DEST_DIR}/ffprobe.exe"
      rm -rf "$TMP_EXTRACT" "$ARCHIVE_PATH"
    fi
    ;;

  *)
    echo "Unsupported platform: $PLATFORM" >&2
    exit 1
    ;;
esac

echo "Done. Binaries in ${DEST_DIR}/"
ls -lh "${DEST_DIR}/"
