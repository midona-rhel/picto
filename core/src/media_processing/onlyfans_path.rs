//! Resolves Picto's bundled OF-Scraper bridge.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ONLYFANS: OnceLock<Result<PathBuf, String>> = OnceLock::new();

#[cfg(target_os = "windows")]
const BRIDGE_BIN: &str = "picto-onlyfans-bridge.exe";
#[cfg(not(target_os = "windows"))]
const BRIDGE_BIN: &str = "picto-onlyfans-bridge";

pub fn onlyfans_path() -> Result<&'static PathBuf, String> {
    ONLYFANS.get_or_init(resolve).as_ref().map_err(Clone::clone)
}

fn resolve() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("PICTO_ONLYFANS_DIR").map(PathBuf::from) {
        let path = dir.join(BRIDGE_BIN);
        return path.is_file().then_some(path).ok_or_else(|| {
            format!(
                "Packaged Picto OnlyFans bridge is missing: {}",
                dir.join(BRIDGE_BIN).display()
            )
        });
    }
    for root in candidate_roots() {
        let path = root.join("vendor").join("onlyfans").join(BRIDGE_BIN);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "Could not find `{BRIDGE_BIN}`. Build Picto's bundled OnlyFans bridge."
    ))
}

fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = std::env::current_dir().into_iter().collect::<Vec<_>>();
    if let Ok(exe) = std::env::current_exe() {
        let mut current = exe.parent().map(Path::to_path_buf);
        while let Some(path) = current {
            roots.push(path.clone());
            current = path.parent().map(Path::to_path_buf);
            if roots.len() > 8 {
                break;
            }
        }
    }
    roots
}
