#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
source_library="${1:?Usage: run.sh /path/to/Main.library}"
source_library="$(cd "$source_library" && pwd)"
snapshot="$(mktemp -d /tmp/picto-gpui-library.XXXXXX)"
# SQLite's online backup includes committed WAL data without touching the live DB.
sqlite3 -readonly "$source_library/library.sqlite" ".backup '$snapshot/library.sqlite'"
touch "$snapshot/.gpui-snapshot"
cargo build --offline
app="$PWD/target/Picto GPUI.app"
mkdir -p "$app/Contents/MacOS"
cp target/debug/picto-gpui-spike "$app/Contents/MacOS/PictoGPUI"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.picto.gpui-experiment</string>
<key>CFBundleName</key><string>Picto GPUI</string>
<key>CFBundleExecutable</key><string>PictoGPUI</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf 'Snapshot: %s\nApp: %s\n' "$snapshot" "$app"
printf '%s\n' "$snapshot" > target/last-snapshot
document_env=()
for name in SOFFICE PDFTOPPM; do
  variable="PICTO_$name"
  command_name="$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')"
  executable="${!variable:-$(command -v "$command_name" || true)}"
  if [[ -n "$executable" ]]; then document_env+=(--env "$variable=$executable"); fi
done
open -n "${document_env[@]}" "$app" --args "$snapshot/library.sqlite" "$source_library/blobs"
