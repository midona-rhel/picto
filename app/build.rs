//! The build script does four things.
//!
//! - It embeds every `assets/*.svg` file as an icon named after the file, so
//!   that adding an icon is adding a file.
//! - It takes the app's version from the repository's `package.json`, which
//!   is the one place the version is written, and that version's release
//!   notes from `docs/<version>-release-notes.md`.
//! - On Linux it makes the program look for shared libraries in its own
//!   folder.
//! - On Windows it puts the app's icon and its version information into the
//!   program.
use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=assets");
    let assets = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    let mut names = fs::read_dir(&assets)
        .expect("read the assets directory")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "svg").then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect::<Vec<_>>();
    names.sort();
    let arms = names
        .iter()
        .map(|name| {
            let path = assets.join(format!("{name}.svg"));
            format!("        {name:?} => Some(include_bytes!({path:?}).as_slice()),\n")
        })
        .collect::<String>();
    let source = format!(
        "fn icon_bytes(path: &str) -> Option<&'static [u8]> {{\n    match path {{\n{arms}        _ => None,\n    }}\n}}\n"
    );
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("icons.rs"),
        source,
    )
    .expect("write the icon table");
    let version = release(&env::var("OUT_DIR").unwrap());
    let target = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "linux" {
        // The application core's ONNX Runtime needs `libwebgpu_dawn.so`,
        // which its build puts beside the program, and the packaged app
        // keeps it there. `$ORIGIN` is the folder that holds the program, so
        // the program finds the library without `LD_LIBRARY_PATH`.
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
    #[cfg(windows)]
    if target == "windows" {
        windows_resource(&version);
    }
    #[cfg(not(windows))]
    let _ = version;
}

/// Compiles a Windows resource into the program: the app's icon
/// (`build/icon.ico`) and its version information, which Explorer and the
/// Task Manager show. The icon has the resource id 1, because GPUI loads the
/// icon with that id for the app's windows. The program's manifest is not
/// part of this resource: GPUI's own build script already embeds one.
///
/// The `embed-resource` crate calls the resource compiler of the Windows
/// SDK. This function exists only when the build runs on Windows, because
/// the crate is a build dependency for Windows only.
#[cfg(windows)]
fn windows_resource(version: &str) {
    let repository = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let icon = repository.join("build/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    // The resource compiler reads a backslash in a quoted path as the start
    // of an escape, so the path is written with forward slashes.
    let icon = icon
        .canonicalize()
        .expect("find build/icon.ico")
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    // The file version is four numbers. A version such as `0.6.19-alpha`
    // gives `0,6,19,0`; the text values below keep the full version.
    let mut numbers = version
        .split(['.', '-', '+'])
        .map_while(|part| part.parse::<u16>().ok())
        .take(3)
        .collect::<Vec<_>>();
    numbers.resize(3, 0);
    let numbers = format!("{},{},{},0", numbers[0], numbers[1], numbers[2]);
    let script = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004L
FILETYPE 0x1L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Picto"
            VALUE "FileDescription", "Picto"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "Picto"
            VALUE "OriginalFilename", "Picto.exe"
            VALUE "ProductName", "Picto"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("picto.rc");
    fs::write(&path, script).expect("write the resource script");
    embed_resource::compile(&path, embed_resource::NONE)
        .manifest_optional()
        .expect("compile the Windows resource");
}

/// Sets `PICTO_VERSION` for the crate and writes `release-notes.md` to
/// `out`. The file is empty when this version has no release notes, and the
/// app then shows no release notes for this version. Returns the version.
fn release(out: &str) -> String {
    let repository = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let package = repository.join("package.json");
    println!("cargo:rerun-if-changed={}", package.display());
    let text = fs::read_to_string(&package).expect("read package.json");
    let version = text
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("\"version\"")?;
            let value = rest.trim_start().strip_prefix(':')?.trim();
            Some(value.trim_end_matches(',').trim_matches('"').to_owned())
        })
        .expect("package.json has a version");
    println!("cargo:rustc-env=PICTO_VERSION={version}");
    let notes = repository.join(format!("docs/{version}-release-notes.md"));
    println!("cargo:rerun-if-changed={}", notes.display());
    fs::write(
        Path::new(out).join("release-notes.md"),
        fs::read_to_string(&notes).unwrap_or_default(),
    )
    .expect("write the release notes");
    version
}
