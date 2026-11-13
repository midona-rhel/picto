#!/bin/bash
# Makes a complete, private copy of a library and opens the GPUI app on it.
#
# The database is copied with SQLite's online backup, which includes committed
# WAL data and never touches the live files the Electron app may have open.
# Everything else in the library folder (blobs, the cover files, the other
# folders) is cloned with APFS `clonefile`, which is instant and takes no
# space until either side changes a file. The source library is only read.
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

scratch="$(mktemp -d /tmp/picto-gpui-library.XXXXXX)"
scratch="$(cd "$scratch" && pwd -P)"
copy="$scratch/$(basename "$source_library")"

# Cloning needs both folders on the same APFS volume. `cp -c` would silently
# fall back to a full copy of tens of gigabytes otherwise, so refuse instead.
filesystem_of() {
	local device
	device="$(df -P "$1" | awk 'NR == 2 { print $1 }')"
	mount | awk -v device="$device" '$1 == device { print $0 }' | sed -E 's/.*\(([a-z0-9]+),.*/\1/'
}
if [[ "$(stat -f %d "$source_library")" != "$(stat -f %d "$scratch")" ]]; then
	rmdir "$scratch"
	fail "$source_library and /tmp are on different volumes, so the library cannot be cloned. Nothing was copied."
fi
if [[ "$(filesystem_of "$scratch")" != "apfs" ]]; then
	rmdir "$scratch"
	fail "the volume holding $source_library is not APFS, so the library cannot be cloned. Nothing was copied."
fi

mkdir "$copy"
# The database first: files it names were added before it was read, so the
# clone taken afterwards holds every one of them.
sqlite3 -readonly "$source_library/library.sqlite" ".backup '$copy/library.sqlite'"
while IFS= read -r -d '' entry; do
	case "$(basename "$entry")" in
	library.sqlite | library.sqlite-wal | library.sqlite-shm | library.sqlite-journal) continue ;;
	esac
	cp -Rc "$entry" "$copy/"
done < <(find "$source_library" -mindepth 1 -maxdepth 1 -print0)

# The marker the app requires, naming the library this is a copy of, so the
# app can refuse a path inside it.
printf '%s\n' "$source_library" > "$copy/.gpui-disposable"

# `PICTO_GPUI_COPY_ONLY=1` stops here, for the tests that need a real copy.
if [[ -n "${PICTO_GPUI_COPY_ONLY:-}" ]]; then
	printf '%s\n' "$copy"
	exit 0
fi

cargo build --locked
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
