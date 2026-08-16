//! Resolves the executable used by the gallery-dl bridge runner.
//!
//! Packaged builds must contain the self-contained Picto bridge sidecar. Local
//! debug builds use the source bridge and vendored wheel so bridge edits take
//! effect immediately instead of silently running a stale compiled sidecar.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static GALLERY_DL: OnceLock<Result<PathBuf, String>> = OnceLock::new();

#[cfg(target_os = "windows")]
const BRIDGE_BIN: &str = "picto-gallery-dl-bridge.exe";
#[cfg(not(target_os = "windows"))]
const BRIDGE_BIN: &str = "picto-gallery-dl-bridge";

#[cfg(target_os = "windows")]
const DEV_FALLBACK_BIN: &str = "gallery-dl.exe";
#[cfg(not(target_os = "windows"))]
const DEV_FALLBACK_BIN: &str = "gallery-dl";

/// Resolve the Picto gallery-dl bridge runtime.
pub fn gallery_dl_path() -> Result<&'static PathBuf, String> {
    GALLERY_DL
        .get_or_init(resolve)
        .as_ref()
        .map_err(Clone::clone)
}

fn resolve() -> Result<PathBuf, String> {
    let packaged_dir = std::env::var_os("PICTO_GALLERY_DL_DIR").map(PathBuf::from);
    resolve_from(
        packaged_dir.as_deref(),
        &candidate_roots(),
        cfg!(debug_assertions),
    )
}

fn resolve_from(
    packaged_dir: Option<&Path>,
    roots: &[PathBuf],
    allow_dev_fallback: bool,
) -> Result<PathBuf, String> {
    if let Some(dir) = packaged_dir {
        let sidecar = dir.join(BRIDGE_BIN);
        if sidecar.is_file() {
            tracing::info!(path = %sidecar.display(), "Using packaged Picto gallery-dl bridge");
            return Ok(sidecar);
        }

        return Err(format!(
            "Packaged Picto gallery-dl bridge is missing: {}",
            sidecar.display()
        ));
    }

    if allow_dev_fallback {
        for root in roots {
            let fallback = root
                .join("vendor")
                .join("gallery-dl")
                .join(DEV_FALLBACK_BIN);
            if has_python_fallback_runtime(&fallback) {
                tracing::warn!(
                    path = %fallback.display(),
                    "Using development Python gallery-dl bridge fallback"
                );
                return Ok(fallback);
            }
        }
    }

    for root in roots {
        let sidecar = root.join("vendor").join("gallery-dl").join(BRIDGE_BIN);
        if sidecar.is_file() {
            tracing::info!(path = %sidecar.display(), "Using vendor Picto gallery-dl bridge");
            return Ok(sidecar);
        }
    }

    Err(format!(
        "Could not find `{BRIDGE_BIN}`. Build or install the self-contained Picto gallery-dl bridge."
    ))
}

fn has_python_fallback_runtime(binary: &Path) -> bool {
    binary.is_file()
        && binary
            .parent()
            .map(|parent| parent.join("wheel").join("gallery_dl").is_dir())
            .unwrap_or(false)
}

/// Candidate root directories to search for the development vendor runtime.
fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        while let Some(current) = dir {
            roots.push(current.clone());
            dir = current.parent().map(Path::to_path_buf);
            if roots.len() > 8 {
                break;
            }
        }
    }

    roots
}

#[cfg(test)]
mod tests {
    use super::{has_python_fallback_runtime, resolve_from, BRIDGE_BIN, DEV_FALLBACK_BIN};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn test_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "picto-gallery-dl-path-test-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn packaged_runtime_name_is_the_picto_bridge() {
        assert!(BRIDGE_BIN.starts_with("picto-gallery-dl-bridge"));
        assert!(!BRIDGE_BIN.starts_with("gallery-dl"));
    }

    #[test]
    fn development_fallback_requires_the_vendored_wheel() {
        let root = test_root();
        let vendor = root.join("vendor").join("gallery-dl");
        fs::create_dir_all(vendor.join("wheel").join("gallery_dl")).unwrap();
        let fallback = vendor.join(DEV_FALLBACK_BIN);
        fs::write(&fallback, b"dev fallback").unwrap();

        assert!(has_python_fallback_runtime(&fallback));
        assert!(!has_python_fallback_runtime(Path::new(
            "/definitely/not/a/gallery-dl-runtime"
        )));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn packaged_directory_never_falls_back_to_development_runtime() {
        let root = test_root();
        let packaged = root.join("packaged");
        let vendor = root.join("vendor").join("gallery-dl");
        fs::create_dir_all(vendor.join("wheel").join("gallery_dl")).unwrap();
        let fallback = vendor.join(DEV_FALLBACK_BIN);
        fs::write(&fallback, b"dev fallback").unwrap();

        let error = resolve_from(Some(&packaged), std::slice::from_ref(&root), true).unwrap_err();
        assert!(error.contains("Packaged Picto gallery-dl bridge is missing"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn development_source_bridge_wins_over_stale_sidecar() {
        let root = test_root();
        let vendor = root.join("vendor").join("gallery-dl");
        fs::create_dir_all(vendor.join("wheel").join("gallery_dl")).unwrap();
        let fallback = vendor.join(DEV_FALLBACK_BIN);
        fs::write(&fallback, b"dev fallback").unwrap();
        let sidecar = vendor.join(BRIDGE_BIN);
        fs::write(&sidecar, b"sidecar").unwrap();

        let resolved = resolve_from(None, std::slice::from_ref(&root), true).unwrap();
        assert_eq!(resolved, fallback);

        fs::remove_dir_all(root).unwrap();
    }
}
