#!/bin/bash
# Starts the Electron app as an isolated reference to compare the GPUI app
# against: a development build from this checkout, in its automation mode,
# on its own disposable copy of a library, with its own application data and
# a DevTools port that `reference.mjs` plays scripts into.
#
#   reference.sh /path/to/Name.library [n]
#
# `n` (default 0) numbers the instance: its DevTools port is 9222 + n, so
# several can run at once. The source library is only read (run.sh copies
# it); the copy is detached from cloud sync, watched folders and queued
# subscription runs before Electron opens it. A development build has no
# updater. Needs `node_modules` at the repository root and in
# `native/picto-node`, and the native addon built
# (`node scripts/build-native-dev.mjs`). Stop it with Ctrl-C.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
library="${1:?Usage: reference.sh /path/to/Name.library [n]}"
instance="${2:-0}"
port=$((9222 + instance))
renderer_port=8181

fail() {
	printf 'reference.sh: %s\n' "$1" >&2
	exit 1
}
[[ -d "$root/node_modules/electron" ]] || fail "no node_modules at $root (install, or link an installed checkout's)"
[[ -f "$root/native/picto-node/index.node" ]] || fail "the native addon is not built: node scripts/build-native-dev.mjs"
[[ -x "$here/target/debug/picto-gpui-spike" ]] || fail "build the GPUI app first (cargo build); it detaches the copy"

copy="$(PICTO_GPUI_COPY_ONLY=1 "$here/run.sh" "$library")"
"$here/target/debug/picto-gpui-spike" "$copy" --detach
data="$(dirname "$copy")/electron-data"
printf 'Copy: %s\nApplication data: %s\nDevTools port: %s\n' "$copy" "$data" "$port"

# One renderer server serves every instance.
if ! curl -s -o /dev/null "http://127.0.0.1:$renderer_port/"; then
	(cd "$root" && node node_modules/vite/bin/vite.js --host 127.0.0.1 --port "$renderer_port" --strictPort >/dev/null 2>&1 &)
	for _ in $(seq 1 60); do
		curl -s -o /dev/null "http://127.0.0.1:$renderer_port/" && break
		sleep 0.5
	done
fi

cd "$root"
# The app's own DevTools switch is fixed at 9222; turn it off and pass ours.
PICTO_E2E=1 PICTO_REMOTE_DEBUGGING=0 PICTO_SMOKE_APP_DATA="$data" PICTO_LIBRARY_ROOT="$copy" \
	VITE_DEV_SERVER_URL="http://127.0.0.1:$renderer_port" \
	exec node node_modules/electron/cli.js . "--remote-debugging-port=$port"
