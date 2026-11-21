#!/bin/bash
# Makes a synthetic library of generated pictures and opens the GPUI app on
# it, for checking scrolling, resizing and loading at scale.
#
#   million.sh [items]        (default 1000000)
#
# The library is made by the repository's `million_grid_demo` example in a new
# folder under /tmp: 4,096 generated JPEGs shared by hard links among the
# items, and five smart folders. No existing library is read or touched.
# Set PICTO_GPUI_MILLION_ONLY=1 to make it, print its path and stop.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
items="${1:-1000000}"
scratch="$(mktemp -d /tmp/picto-gpui-synthetic.XXXXXX)"
library="$scratch/Synthetic.library"

(cd "$root" && PICTO_DEMO_ITEMS="$items" cargo run --release -p picto_core --example million_grid_demo -- "$library")
# The app opens only folders marked as disposable. This one was just made
# from nothing, so it names no library it was copied from.
printf 'synthetic:million_grid_demo\n' > "$library/.gpui-disposable"

if [[ -n "${PICTO_GPUI_MILLION_ONLY:-}" ]]; then
	printf '%s\n' "$library"
	exit 0
fi
(cd "$here" && cargo build --locked)
PICTO_GPUI_FPS=1 "$here/target/debug/picto-gpui-spike" "$library"
