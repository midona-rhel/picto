use std::path::{Path, PathBuf};

use tracing::warn;

pub(super) struct RunTempDir {
    path: Option<PathBuf>,
}

impl RunTempDir {
    pub(super) fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub(super) fn into_path(mut self) -> PathBuf {
        self.path.take().expect("run temp directory is owned")
    }
}

impl Drop for RunTempDir {
    fn drop(&mut self) {
        let Some(path) = self.path.take() else {
            return;
        };
        if let Err(error) = std::fs::remove_dir_all(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(path = %path.display(), error = %error, "Failed to clean up abandoned gallery-dl run");
            }
        }
    }
}

pub async fn cleanup_temp_dir(temp_dir: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(temp_dir).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            warn!(path = %temp_dir.display(), error = %error, "Failed to clean up gallery-dl run");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_owned_run_directory_removes_it() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("run");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("download.bin"), b"media").unwrap();

        drop(RunTempDir::new(path.clone()));

        assert!(!path.exists());
    }

    #[test]
    fn transferring_run_directory_keeps_it_for_terminal_cleanup() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("run");
        std::fs::create_dir_all(&path).unwrap();

        let transferred = RunTempDir::new(path.clone()).into_path();

        assert_eq!(transferred, path);
        assert!(path.exists());
    }
}
