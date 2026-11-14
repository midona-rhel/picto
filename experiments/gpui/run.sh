#!/bin/bash
# Makes a complete, private copy of a library and opens the GPUI app on it.
#
# The copy is the app's own `--make-copy` (`application::make_copy`): the
# database through SQLite's online backup, which includes committed WAL data
# and never touches the live files the Electron app may have open; everything
# else through the system's file copy, which on APFS is a `clonefile` that is
# instant and takes no space until either side changes a file; a library that
# holds a symbolic link is refused; the copy is marked, detached and checked.
# The source library is only read.
set -euo pipefail
cd "$(dirname "$0")"

fail() {
	printf 'run.sh: %s\n' "$1" >&2
	exit 1
}

source_library="${1:?Usage: run.sh /path/to/Name.library}"
[[ -d "$source_library" ]] || fail "$source_library is not a folder"
source_library="$(cd "$source_library" && pwd -P)"
[[ -f "$source_library/library.sqlite" ]] || fail "$source_library has no library.sqlite"

# The copy goes to the system's temporary folder. Cloning needs it on the same
# APFS volume as the library; elsewhere the file copy would silently copy tens
# of gigabytes in full, so refuse instead.
temporary="$(cd "${TMPDIR:-/tmp}" && pwd -P)"
filesystem_of() {
	local device
	device="$(df -P "$1" | awk 'NR == 2 { print $1 }')"
	mount | awk -v device="$device" '$1 == device { print $0 }' | sed -E 's/.*\(([a-z0-9]+),.*/\1/'
}
if [[ "$(stat -f %d "$source_library")" != "$(stat -f %d "$temporary")" ]]; then
	fail "$source_library and $temporary are on different volumes, so the library cannot be cloned. Nothing was copied."
fi
if [[ "$(filesystem_of "$temporary")" != "apfs" ]]; then
	fail "the volume holding $source_library is not APFS, so the library cannot be cloned. Nothing was copied."
fi

cargo build --locked >&2
copy="$(target/debug/picto-gpui-spike --make-copy "$source_library" | tail -n 1)"
[[ -n "$copy" ]] || fail "no copy was made"

# `PICTO_GPUI_COPY_ONLY=1` stops here, for the tests that need a real copy.
if [[ -n "${PICTO_GPUI_COPY_ONLY:-}" ]]; then
	printf '%s\n' "$copy"
	exit 0
fi

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
printf 'Copy: %s\nApp: %s\n' "$copy" "$app"
printf '%s\n' "$copy" > target/last-snapshot
open -n "$app" --args "$copy"
