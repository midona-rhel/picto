//! Embeds every `assets/*.svg` as an icon named after its file, so adding an
//! icon is adding a file; and takes the app's version from the repository's
//! `package.json` (production's, the one source of it) with that version's
//! bundled release notes (`docs/<version>-release-notes.md`).
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
    release(&env::var("OUT_DIR").unwrap());
}

/// `PICTO_VERSION` and `release-notes.md` (empty when this version has
/// none, which the update check reports as production does).
fn release(out: &str) {
    let repository = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
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
}
