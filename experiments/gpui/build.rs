//! Embeds every `assets/*.svg` as an icon named after its file, so adding an
//! icon is adding a file.
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
}
