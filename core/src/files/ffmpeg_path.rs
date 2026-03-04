//! Resolves the path to bundled `ffmpeg` and `ffprobe` CLI binaries.
//!
//! Resolution order:
//! 1. `PICTO_FFMPEG_DIR` environment variable (set by Electron for packaged app)
//! 2. `vendor/ffmpeg/` relative to the workspace root (local development)
//! 3. System PATH fallback

use std::path::PathBuf;
use std::sync::OnceLock;

static FFMPEG: OnceLock<Result<PathBuf, String>> = OnceLock::new();
static FFPROBE: OnceLock<Result<PathBuf, String>> = OnceLock::new();

#[cfg(target_os = "windows")]
const FFMPEG_BIN: &str = "ffmpeg.exe";
#[cfg(not(target_os = "windows"))]
const FFMPEG_BIN: &str = "ffmpeg";

#[cfg(target_os = "windows")]
const FFPROBE_BIN: &str = "ffprobe.exe";
#[cfg(not(target_os = "windows"))]
const FFPROBE_BIN: &str = "ffprobe";

/// Resolve the path to the `ffmpeg` binary.
pub fn ffmpeg_path() -> Result<&'static PathBuf, String> {
    FFMPEG
        .get_or_init(|| resolve(FFMPEG_BIN))
        .as_ref()
        .map_err(|e| e.clone())
}

/// Resolve the path to the `ffprobe` binary.
pub fn ffprobe_path() -> Result<&'static PathBuf, String> {
    FFPROBE
        .get_or_init(|| resolve(FFPROBE_BIN))
        .as_ref()
        .map_err(|e| e.clone())
}

fn resolve(bin_name: &str) -> Result<PathBuf, String> {
    // 1. PICTO_FFMPEG_DIR env var (packaged Electron app)
    if let Ok(dir) = std::env::var("PICTO_FFMPEG_DIR") {
        let p = PathBuf::from(&dir).join(bin_name);
        if p.is_file() {
            tracing::info!("Using bundled {bin_name}: {}", p.display());
            return Ok(p);
        }
    }

    // 2. vendor/ffmpeg/ relative to the workspace root.
    //    Walk up from the current exe or cwd looking for vendor/ffmpeg/.
    for base in candidate_roots() {
        let p = base.join("vendor").join("ffmpeg").join(bin_name);
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
        "Could not find `{bin_name}`. Install FFmpeg or run `bash scripts/download-ffmpeg.sh`."
    ))
}

/// Candidate root directories (workspace root) to search for vendor/ffmpeg/.
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
