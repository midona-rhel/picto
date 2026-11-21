#!/bin/bash
# Builds the release program and makes the package of Picto for the system
# this script runs on. The packages have the file names that every release
# of Picto has had:
#
#   macOS (arm64):  Picto-<version>-arm64.dmg, Picto-<version>-arm64-mac.zip
#                   and latest-mac.yml
#   Windows (x64):  Picto-Setup-<version>.exe and latest.yml
#   Linux (x64):    Picto-<version>.AppImage and latest-linux.yml
#
# The `.yml` files are the update feeds that the app's updater reads. The
# updater of Picto 0.6.19 and earlier, which electron-updater provided, reads
# them too. Everything is written to `target/package/`.
# The version is the one in the repository's `package.json`. Arguments given
# to this script are passed on to `cargo build`; `./package.sh --offline`
# builds without the network.
#
# What the script needs:
#
# - `ffmpeg` and `ffprobe` in `vendor/ffmpeg/` at the repository's root.
#   `bash scripts/download-ffmpeg.sh`, run there, downloads them.
# - On Windows: Git Bash to run this script, and NSIS 3 (`makensis`).
# - On Linux: `appimagetool`, either on the PATH or named by the environment
#   variable `APPIMAGETOOL`. `APPIMAGE_RUNTIME` may name the runtime file
#   that appimagetool puts into the AppImage; without it appimagetool
#   downloads one.
# - On macOS with Xcode 26 or later: Node.js and the repository's packages
#   (`npm ci`), for the script that draws the layer of the app's icon.
# - On macOS, to sign: the certificate in `MAC_CSC_LINK` and its password in
#   `MAC_CSC_KEY_PASSWORD`, and Node.js. Without them the app is not signed
#   with the project's certificate, and the script says so.
#   `PICTO_REQUIRE_MAC_SIGNING=true` makes the script fail instead.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repository="$(cd "$here/.." && pwd)"
cd "$here"

say() {
	printf 'package.sh: %s\n' "$1"
}

fail() {
	printf 'package.sh: %s\n' "$1" >&2
	exit 1
}

case "$(uname -s)" in
Darwin) system=mac ;;
Linux) system=linux ;;
MINGW* | MSYS* | CYGWIN*) system=windows ;;
*) fail "there is no package for the system $(uname -s)" ;;
esac

version="$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$repository/package.json" | head -n 1)"
[[ -n "$version" ]] || fail "package.json names no version"
out="$here/target/package"
mkdir -p "$out"

if [[ "$system" == windows ]]; then
	program="target/release/picto-gpui-spike.exe"
	ffmpeg=("$repository/vendor/ffmpeg/ffmpeg.exe" "$repository/vendor/ffmpeg/ffprobe.exe")
else
	program="target/release/picto-gpui-spike"
	ffmpeg=("$repository/vendor/ffmpeg/ffmpeg" "$repository/vendor/ffmpeg/ffprobe")
fi
for file in "${ffmpeg[@]}"; do
	[[ -f "$file" ]] || fail "$file is missing. Run \`bash scripts/download-ffmpeg.sh\` in the repository's root first."
done

# The pinned toolchain, when it is unpacked beside this script (README,
# "Building").
for bin in .toolchain/*/bin; do
	[[ -x "$bin/cargo" ]] && PATH="$here/$bin:$PATH"
done
say "building the release program (version $version)"
cargo build --locked --release "$@"
[[ -f "$program" ]] || fail "the build did not make $program"

# Prints the SHA-512 of a file in base64, as the update feeds give it.
sha512() {
	openssl dgst -sha512 -binary "$1" | openssl base64 -A
}

# Prints the size of a file in bytes.
size() {
	wc -c <"$1" | tr -d '[:space:]'
}

# Writes the update feed `$1` for the package files that follow. The format
# is the one electron-builder writes: the version, each file with its
# SHA-512 and its size, and once more the name and SHA-512 of the first
# file, which older updaters read. The app's updater (`src/updates/feed.rs`)
# and the updater of Picto 0.6.19 and earlier take the file to download from
# this feed and check the download against the SHA-512 and the size.
write_feed() {
	local feed="$1" file
	shift
	{
		printf 'version: %s\n' "$version"
		printf 'files:\n'
		for file in "$@"; do
			printf '  - url: %s\n' "$(basename "$file")"
			printf '    sha512: %s\n' "$(sha512 "$file")"
			printf '    size: %s\n' "$(size "$file")"
		done
		printf 'path: %s\n' "$(basename "$1")"
		printf 'sha512: %s\n' "$(sha512 "$1")"
		printf "releaseDate: '%s'\n" "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)"
	} >"$feed"
}

# Puts the app's icon into the bundle's Resources folder `$1`. With Xcode 26
# or later, `actool` compiles the Icon Composer file `build/Picto.icon` into
# `Assets.car` and `Icon.icns`. Without it, `Icon.icns` is made from
# `build/icon-flat.png`, and the bundle has no `Assets.car`.
mac_icon() {
	local resources="$1" work actool_version pixels
	work="$(mktemp -d)"
	actool_version="$(xcrun actool --version 2>/dev/null |
		sed -n '/short-bundle-version/{n;s/.*<string>\([0-9]*\).*/\1/p;}' || true)"
	if [[ -n "$actool_version" && "$actool_version" -ge 26 ]]; then
		# The picture of the icon's one layer is not kept in the
		# repository. `scripts/generate-app-icons.mjs` draws it from
		# `build/icons/picto-macos-book.svg`.
		node "$repository/scripts/generate-app-icons.mjs" ||
			fail "the icon's layer could not be drawn. Run \`npm ci\` in the repository's root first."
		cp -R "$repository/build/Picto.icon" "$work/Icon.icon"
		mkdir "$work/out"
		xcrun actool "$work/Icon.icon" --compile "$work/out" \
			--output-format human-readable-text --notices --warnings \
			--output-partial-info-plist "$work/out/assetcatalog_generated_info.plist" \
			--app-icon Icon --include-all-app-icons --accent-color AccentColor \
			--enable-on-demand-resources NO --development-region en \
			--target-device mac --minimum-deployment-target 26.0 --platform macosx
		cp "$work/out/Assets.car" "$work/out/Icon.icns" "$resources/"
		say "the icon was compiled from build/Picto.icon with actool $actool_version"
	else
		[[ "${PICTO_REQUIRE_MAC_SIGNING:-}" != true ]] ||
			fail "a release package needs Xcode 26 or later and actool, for the Icon Composer icon"
		mkdir "$work/Icon.iconset"
		for pixels in 16 32 128 256; do
			sips -z "$pixels" "$pixels" "$repository/build/icon-flat.png" \
				--out "$work/Icon.iconset/icon_${pixels}x${pixels}.png" >/dev/null
			sips -z "$((pixels * 2))" "$((pixels * 2))" "$repository/build/icon-flat.png" \
				--out "$work/Icon.iconset/icon_${pixels}x${pixels}@2x.png" >/dev/null
		done
		cp "$repository/build/icon-flat.png" "$work/Icon.iconset/icon_512x512.png"
		iconutil -c icns "$work/Icon.iconset" -o "$resources/Icon.icns"
		say "actool 26 or later was not found, so the icon was made from build/icon-flat.png and the bundle has no Icon Composer catalog"
	fi
	rm -rf "$work"
}

package_mac() {
	[[ "$(uname -m)" == arm64 ]] || fail "the macOS package is built on Apple silicon only"
	local stage="$out/mac-arm64"
	local app="$stage/Picto.app"
	local zip="$out/Picto-$version-arm64-mac.zip"
	local dmg="$out/Picto-$version-arm64.dmg"
	rm -rf "$stage" "$zip" "$dmg"
	mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

	# The program, and `ffmpeg` and `ffprobe` beside it, where the app looks
	# for them (`folder_with_bundled_ffmpeg` in `src/main.rs`). The release
	# program loads only libraries of macOS, so no library is copied.
	cp "$program" "$app/Contents/MacOS/Picto"
	cp "${ffmpeg[@]}" "$app/Contents/MacOS/"
	cp packaging/macos/Info.plist "$app/Contents/Info.plist"
	plutil -replace CFBundleShortVersionString -string "$version" "$app/Contents/Info.plist"
	plutil -replace CFBundleVersion -string "$version" "$app/Contents/Info.plist"
	plutil -lint "$app/Contents/Info.plist" >/dev/null
	printf 'APPL????' >"$app/Contents/PkgInfo"
	# The icons of the two file types that Info.plist names.
	cp "$repository/build/library.icns" "$repository/build/picto-pack.icns" "$app/Contents/Resources/"
	mac_icon "$app/Contents/Resources"

	if [[ -n "${MAC_CSC_LINK:-}" ]]; then
		node "$repository/scripts/sign-mac-app.mjs" "$app"
	elif [[ "${PICTO_REQUIRE_MAC_SIGNING:-}" == true ]]; then
		fail "PICTO_REQUIRE_MAC_SIGNING is true, and MAC_CSC_LINK and MAC_CSC_KEY_PASSWORD are not set"
	else
		say "MAC_CSC_LINK is not set. Picto.app is NOT signed with the project's certificate \"Picto Code Signing\". It is a package for trying out, not for a release."
	fi

	# The archive keeps the bundle's folder, its extended attributes and its
	# signature, as `ditto` writes them.
	ditto -c -k --sequesterRsrc --keepParent "$app" "$zip"
	# The disk image shows the app beside a link to the Applications folder,
	# so that the user installs by dragging one onto the other.
	local image="$stage/disk-image"
	mkdir "$image"
	cp -R "$app" "$image/"
	ln -s /Applications "$image/Applications"
	hdiutil create -quiet -volname "Picto $version" -srcfolder "$image" -format UDZO -ov "$dmg"
	# The bundle is now inside the archive and the disk image. The copy that
	# was put together here is removed, so that macOS does not find a second
	# Picto.app on this machine and offer it for Picto's files.
	rm -rf "$stage"
	write_feed "$out/latest-mac.yml" "$zip" "$dmg"
	packages=("$dmg" "$zip" "$out/latest-mac.yml")
}

package_windows() {
	local stage="$out/win-x64/Picto"
	local installer="$out/Picto-Setup-$version.exe"
	local uninstall_list="$out/win-x64/uninstall-files.nsh"
	local makensis library numbers name
	rm -rf "$out/win-x64" "$installer"
	mkdir -p "$stage/resources"

	# The program, the libraries that the build put beside it
	# (`DirectML.dll`, which the application core's ONNX Runtime uses and
	# which the program loads when it starts), `ffmpeg` and `ffprobe`, and
	# the icon of pack files.
	cp "$program" "$stage/Picto.exe"
	for library in target/release/*.dll; do
		[[ -e "$library" ]] || continue
		cp "$library" "$stage/"
		say "library shipped beside the program: $(basename "$library")"
	done
	cp "${ffmpeg[@]}" "$stage/"
	cp "$repository/build/picto-pack.ico" "$stage/resources/picto-pack.ico"

	# The uninstaller deletes the installed files one by one, by name. This
	# writes the instructions for it: a `Delete` for every file put together
	# above, then an `RMDir` for every folder, the deepest first.
	{
		(cd "$stage" && find . -type f | sed 's|^\./||' | sort) | while read -r name; do
			printf 'Delete "$INSTDIR\\%s"\n' "${name//\//\\}"
		done
		(cd "$stage" && find . -mindepth 1 -type d | sed 's|^\./||' | sort -r) | while read -r name; do
			printf 'RMDir "$INSTDIR\\%s"\n' "${name//\//\\}"
		done
	} >"$uninstall_list"

	makensis="$(command -v makensis || true)"
	[[ -n "$makensis" ]] || makensis="/c/Program Files (x86)/NSIS/makensis.exe"
	[[ -x "$makensis" ]] || fail "makensis was not found. Install NSIS 3."
	# A version such as `0.6.19-alpha` gives the four numbers `0.6.19.0`.
	numbers="$(printf '%s' "$version" | sed -E 's/^([0-9]+)\.([0-9]+)\.([0-9]+).*/\1.\2.\3.0/')"
	# Git Bash would rewrite arguments that look like paths; these two
	# variables switch that off for this one command.
	MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' "$makensis" -V2 -INPUTCHARSET UTF8 \
		"-DVERSION=$version" \
		"-DVERSION_NUMBERS=$numbers" \
		"-DSOURCE=$(cygpath -w "$stage")" \
		"-DOUTPUT=$(cygpath -w "$installer")" \
		"-DICON=$(cygpath -w "$repository/build/icon.ico")" \
		"-DESTIMATED_SIZE_KB=$(du -sk "$stage" | cut -f1)" \
		"-DUNINSTALL_LIST=$(cygpath -w "$uninstall_list")" \
		"$(cygpath -w "$here/packaging/windows/installer.nsi")"
	write_feed "$out/latest.yml" "$installer"
	packages=("$installer" "$out/latest.yml")
}

package_linux() {
	[[ "$(uname -m)" == x86_64 ]] || fail "the Linux package is built on x86_64 only"
	local appdir="$out/linux-x64/Picto.AppDir"
	local image="$out/Picto-$version.AppImage"
	local tool="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
	local library arguments
	[[ -n "$tool" && -x "$tool" ]] || fail "appimagetool was not found. Put it on the PATH or name it in APPIMAGETOOL."
	rm -rf "$out/linux-x64" "$image"
	mkdir -p "$appdir/usr/bin" "$appdir/usr/share/icons/hicolor/512x512/apps"

	# The program, with the shared libraries that the build put beside it
	# (the application core's ONNX Runtime needs `libwebgpu_dawn.so`). The
	# program looks for libraries in its own folder (`build.rs`), so they
	# stay beside it. Libraries of the system (GTK, WebKitGTK and the others
	# the README lists) are not shipped.
	cp "$program" "$appdir/usr/bin/picto"
	if ldd "$here/$program" | grep 'not found'; then
		fail "the release program does not find the libraries listed above"
	fi
	while read -r library; do
		cp "$library" "$appdir/usr/bin/"
		say "library shipped beside the program: $(basename "$library")"
	done < <(ldd "$here/$program" | awk '$3 ~ /^\// { print $3 }' | grep -F "$here/target/release/" || true)
	cp "${ffmpeg[@]}" "$appdir/usr/bin/"

	cp packaging/linux/AppRun "$appdir/AppRun"
	chmod 755 "$appdir/AppRun" "$appdir/usr/bin/picto" "$appdir/usr/bin/ffmpeg" "$appdir/usr/bin/ffprobe"
	cp packaging/linux/com.picto.desktop.desktop "$appdir/"
	cp "$repository/build/icon-flat.png" "$appdir/picto.png"
	cp "$repository/build/icon-flat.png" "$appdir/usr/share/icons/hicolor/512x512/apps/picto.png"
	ln -s picto.png "$appdir/.DirIcon"

	arguments=(--no-appstream)
	[[ -z "${APPIMAGE_RUNTIME:-}" ]] || arguments+=(--runtime-file "$APPIMAGE_RUNTIME")
	# appimagetool is an AppImage itself. `APPIMAGE_EXTRACT_AND_RUN` makes it
	# unpack itself to run, so that it also works where FUSE is not set up.
	ARCH=x86_64 VERSION="$version" APPIMAGE_EXTRACT_AND_RUN=1 \
		"$tool" "${arguments[@]}" "$appdir" "$image"
	chmod 755 "$image"
	write_feed "$out/latest-linux.yml" "$image"
	packages=("$image" "$out/latest-linux.yml")
}

packages=()
"package_$system"
say "made in $out:"
for file in "${packages[@]}"; do
	printf '  %s  %s bytes\n' "$(basename "$file")" "$(size "$file")"
done
