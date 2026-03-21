#!/usr/bin/env bash
# Download ONNX Runtime shared library for the current platform.
# Places the library in vendor/onnxruntime/ for electron-builder to bundle.
set -euo pipefail

ORT_VERSION="1.20.1"
VENDOR_DIR="vendor/onnxruntime"
mkdir -p "$VENDOR_DIR"

detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin)
      case "$arch" in
        arm64) echo "osx-arm64" ;;
        x86_64) echo "osx-x86_64" ;;
        *) echo "unsupported" ;;
      esac
      ;;
    Linux)
      echo "linux-x64"
      ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      echo "win-x64"
      ;;
    *)
      echo "unsupported"
      ;;
  esac
}

PLATFORM="$(detect_platform)"
if [ "$PLATFORM" = "unsupported" ]; then
  echo "Unsupported platform, skipping ONNX Runtime download"
  exit 0
fi

ARCHIVE_NAME="onnxruntime-$PLATFORM-$ORT_VERSION"

# Windows uses .zip, everything else uses .tgz
if [[ "$PLATFORM" == win-* ]]; then
  URL="https://github.com/microsoft/onnxruntime/releases/download/v$ORT_VERSION/$ARCHIVE_NAME.zip"
  echo "Downloading ONNX Runtime $ORT_VERSION for $PLATFORM..."
  TMPFILE="$(mktemp).zip"
  curl -fSL "$URL" -o "$TMPFILE"
  echo "Extracting..."
  unzip -q -o "$TMPFILE" "$ARCHIVE_NAME/lib/*" -d "$VENDOR_DIR"
  # Flatten: move lib/* up
  if [ -d "$VENDOR_DIR/$ARCHIVE_NAME/lib" ]; then
    mv "$VENDOR_DIR/$ARCHIVE_NAME"/lib/* "$VENDOR_DIR/" 2>/dev/null || true
    rm -rf "$VENDOR_DIR/$ARCHIVE_NAME"
  fi
  rm -f "$TMPFILE"
else
  URL="https://github.com/microsoft/onnxruntime/releases/download/v$ORT_VERSION/$ARCHIVE_NAME.tgz"
  echo "Downloading ONNX Runtime $ORT_VERSION for $PLATFORM..."
  TMPFILE="$(mktemp)"
  curl -fSL "$URL" -o "$TMPFILE"
  echo "Extracting..."
  tar xzf "$TMPFILE" -C "$VENDOR_DIR" --strip-components=1 "$ARCHIVE_NAME/lib"
  # Flatten: move lib/* up
  if [ -d "$VENDOR_DIR/lib" ]; then
    mv "$VENDOR_DIR"/lib/* "$VENDOR_DIR/" 2>/dev/null || true
    rmdir "$VENDOR_DIR/lib" 2>/dev/null || true
  fi
  rm -f "$TMPFILE"
fi

echo "ONNX Runtime installed to $VENDOR_DIR/"
ls -la "$VENDOR_DIR/"
