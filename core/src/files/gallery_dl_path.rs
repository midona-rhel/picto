//! Resolves the path to the bundled `gallery-dl` CLI binary.
//!
//! Resolution order:
//! 1. `PICTO_GALLERY_DL_DIR` environment variable (set by Electron for packaged app)
//! 2. `vendor/gallery-dl/` relative to the workspace root (local development)
//! 3. System PATH fallback

use std::path::PathBuf;
use std::sync::OnceLock;

static GALLERY_DL: OnceLock<Result<PathBuf, String>> = OnceLock::new();

#[cfg(target_os = "windows")]
const GALLERY_DL_BIN: &str = "gallery-dl.exe";
#[cfg(not(target_os = "windows"))]
const GALLERY_DL_BIN: &str = "gallery-dl";

/// Resolve the path to the `gallery-dl` binary.
pub fn gallery_dl_path() -> Result<&'static PathBuf, String> {
    GALLERY_DL
        .get_or_init(|| resolve(GALLERY_DL_BIN))
        .as_ref()
        .map_err(|e| e.clone())
}

fn resolve(bin_name: &str) -> Result<PathBuf, String> {
    // 1. PICTO_GALLERY_DL_DIR env var (packaged Electron app)
    if let Ok(dir) = std::env::var("PICTO_GALLERY_DL_DIR") {
        let p = PathBuf::from(&dir).join(bin_name);
        if p.is_file() {
            tracing::info!("Using bundled {bin_name}: {}", p.display());
            return Ok(p);
        }
    }

    // 2. vendor/gallery-dl/ relative to the workspace root.
    for base in candidate_roots() {
        let p = base.join("vendor").join("gallery-dl").join(bin_name);
        if p.is_file() {
            tracing::info!("Using vendor {bin_name}: {}", p.display());
            return Ok(p);
        }
    }

    // 3. System PATH fallback
    if let Ok(p) = which(bin_name) {
        tracing::info!("Using system {bin_name}: {}", p.display());
        return Ok(p);
    }

    Err(format!(
        "Could not find `{bin_name}`. Run `bash scripts/download-gallery-dl.sh` or install gallery-dl."
    ))
}

/// Candidate root directories (workspace root) to search for vendor/gallery-dl/.
fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    // Current working directory (often the workspace root during dev)
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }

    // Walk up from the executable
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            roots.push(d.clone());
            dir = d.parent().map(|p| p.to_path_buf());
            // Don't walk above 5 levels
            if roots.len() > 8 {
                break;
            }
        }
    }

    roots
}

/// Simple which(1) implementation — search PATH for a binary.
fn which(bin_name: &str) -> Result<PathBuf, ()> {
    let path_var = std::env::var("PATH").map_err(|_| ())?;
    #[cfg(target_os = "windows")]
    let sep = ';';
    #[cfg(not(target_os = "windows"))]
    let sep = ':';

    for dir in path_var.split(sep) {
        let candidate = PathBuf::from(dir).join(bin_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(())
}
