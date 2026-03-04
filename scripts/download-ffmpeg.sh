#!/usr/bin/env bash
# Download pre-built GPL ffmpeg + ffprobe binaries for the current platform.
#
# Sources (pinned, no floating "latest" release URL path):
#   macOS/Linux: https://ffmpeg.martin-riedl.de (release 8.0.1)
#   Windows:     https://github.com/GyanD/codexffmpeg (release 8.0.1)
#
# Usage:
#   bash scripts/download-ffmpeg.sh              # auto-detect platform
#   bash scripts/download-ffmpeg.sh darwin-arm64  # force platform

set -euo pipefail

DEST_DIR="vendor/ffmpeg"

FFMPEG_VERSION="8.0.1"
GYAN_VERSION="8.0.1"
GYAN_WIN64_ZIP_URL="https://github.com/GyanD/codexffmpeg/releases/download/${GYAN_VERSION}/ffmpeg-${GYAN_VERSION}-full_build.zip"

# Pinned Martin-Riedl release URLs (8.0.1)
MAC_AMD64_FFMPEG_URL="https://ffmpeg.martin-riedl.de/download/macos/amd64/1766437297_8.0.1/ffmpeg.zip"
MAC_AMD64_FFPROBE_URL="https://ffmpeg.martin-riedl.de/download/macos/amd64/1766437297_8.0.1/ffprobe.zip"
MAC_ARM64_FFMPEG_URL="https://ffmpeg.martin-riedl.de/download/macos/arm64/1766430132_8.0.1/ffmpeg.zip"
MAC_ARM64_FFPROBE_URL="https://ffmpeg.martin-riedl.de/download/macos/arm64/1766430132_8.0.1/ffprobe.zip"
LINUX_AMD64_FFMPEG_URL="https://ffmpeg.martin-riedl.de/download/linux/amd64/1766430728_8.0.1/ffmpeg.zip"
LINUX_AMD64_FFPROBE_URL="https://ffmpeg.martin-riedl.de/download/linux/amd64/1766430728_8.0.1/ffprobe.zip"

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
    # martin-riedl.de: macOS builds (GPL, signed, notarized), pinned to 8.0.1
    if [ "$PLATFORM" = "darwin-arm64" ]; then
      FFMPEG_URL="$MAC_ARM64_FFMPEG_URL"
      FFPROBE_URL="$MAC_ARM64_FFPROBE_URL"
    else
      FFMPEG_URL="$MAC_AMD64_FFMPEG_URL"
      FFPROBE_URL="$MAC_AMD64_FFPROBE_URL"
    fi

    if [ ! -f "${DEST_DIR}/ffmpeg" ]; then
      TMP_ZIP="${DEST_DIR}/ffmpeg.zip"
      echo "Downloading ffmpeg (macOS, release ${FFMPEG_VERSION})..."
      download "$FFMPEG_URL" "$TMP_ZIP"
      unzip -qo "$TMP_ZIP" -d "$DEST_DIR"
      rm -f "$TMP_ZIP"
      chmod +x "${DEST_DIR}/ffmpeg"
    else
      echo "  Already exists: ${DEST_DIR}/ffmpeg (delete to re-download)"
    fi

    if [ ! -f "${DEST_DIR}/ffprobe" ]; then
      TMP_ZIP="${DEST_DIR}/ffprobe.zip"
      echo "Downloading ffprobe (macOS, release ${FFMPEG_VERSION})..."
      download "$FFPROBE_URL" "$TMP_ZIP"
      unzip -qo "$TMP_ZIP" -d "$DEST_DIR"
      rm -f "$TMP_ZIP"
      chmod +x "${DEST_DIR}/ffprobe"
    else
      echo "  Already exists: ${DEST_DIR}/ffprobe (delete to re-download)"
    fi
    ;;

  linux-x64)
    # martin-riedl.de: Linux x64 release build (pinned to 8.0.1)
    if [ -f "${DEST_DIR}/ffmpeg" ] && [ -f "${DEST_DIR}/ffprobe" ]; then
      echo "  Already exists: ${DEST_DIR}/ffmpeg + ffprobe (delete to re-download)"
    else
      TMP_FFMPEG="${DEST_DIR}/ffmpeg.zip"
      TMP_FFPROBE="${DEST_DIR}/ffprobe.zip"
      echo "Downloading ffmpeg + ffprobe (linux, release ${FFMPEG_VERSION})..."
      download "$LINUX_AMD64_FFMPEG_URL" "$TMP_FFMPEG"
      download "$LINUX_AMD64_FFPROBE_URL" "$TMP_FFPROBE"
      unzip -qo "$TMP_FFMPEG" -d "$DEST_DIR"
      unzip -qo "$TMP_FFPROBE" -d "$DEST_DIR"
      rm -f "$TMP_FFMPEG" "$TMP_FFPROBE"
      chmod +x "${DEST_DIR}/ffmpeg" "${DEST_DIR}/ffprobe"
    fi
    ;;

  win32-x64)
    # Gyan mirror: Windows x64 release build (pinned to 8.0.1)
    ARCHIVE_NAME="ffmpeg-${GYAN_VERSION}-full_build.zip"
    ARCHIVE_PATH="${DEST_DIR}/${ARCHIVE_NAME}"

    if [ -f "${DEST_DIR}/ffmpeg.exe" ] && [ -f "${DEST_DIR}/ffprobe.exe" ]; then
      echo "  Already exists: ${DEST_DIR}/ffmpeg.exe (delete to re-download)"
    else
      echo "Downloading Gyan FFmpeg (win64, release ${GYAN_VERSION})..."
      download "$GYAN_WIN64_ZIP_URL" "$ARCHIVE_PATH"
      echo "  Extracting ffmpeg.exe + ffprobe.exe..."
      TMP_EXTRACT="${DEST_DIR}/_extract"
      mkdir -p "$TMP_EXTRACT"
      unzip -qo "$ARCHIVE_PATH" -d "$TMP_EXTRACT"
      FFMPEG_EXE_PATH="$(find "$TMP_EXTRACT" -type f -name 'ffmpeg.exe' | head -n 1)"
      FFPROBE_EXE_PATH="$(find "$TMP_EXTRACT" -type f -name 'ffprobe.exe' | head -n 1)"
      if [ -z "$FFMPEG_EXE_PATH" ] || [ -z "$FFPROBE_EXE_PATH" ]; then
        echo "Failed to locate ffmpeg.exe/ffprobe.exe in ${ARCHIVE_NAME}" >&2
        exit 1
      fi
      mv "$FFMPEG_EXE_PATH" "${DEST_DIR}/ffmpeg.exe"
      mv "$FFPROBE_EXE_PATH" "${DEST_DIR}/ffprobe.exe"
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
