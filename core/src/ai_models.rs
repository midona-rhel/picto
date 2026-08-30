//! Replacement AI model inventory and filesystem lifecycle.

use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicU64, Arc};

use tokio_util::sync::CancellationToken;

use crate::ai_tagger::models::{self, ModelInfo};
pub(crate) struct AiModelDownload {
    pub(crate) cancel: CancellationToken,
    pub(crate) downloaded_bytes: Arc<AtomicU64>,
    pub(crate) total_bytes: u64,
}

pub(crate) trait AiModelHost {
    fn library_root(&self) -> &Path;
    fn ai_sessions(&self) -> &crate::ai_tagger::inference::SharedTaggerSessions;
    fn ai_model_downloads(
        &self,
    ) -> &tokio::sync::Mutex<std::collections::HashMap<String, AiModelDownload>>;
    fn ai_model_lifecycle(&self) -> &tokio::sync::Mutex<()>;
}

impl AiModelHost for crate::library_application::LibraryApplication {
    fn library_root(&self) -> &Path {
        self.root()
    }

    fn ai_sessions(&self) -> &crate::ai_tagger::inference::SharedTaggerSessions {
        self.ai_sessions()
    }

    fn ai_model_downloads(
        &self,
    ) -> &tokio::sync::Mutex<std::collections::HashMap<String, AiModelDownload>> {
        self.ai_model_downloads()
    }

    fn ai_model_lifecycle(&self) -> &tokio::sync::Mutex<()> {
        self.ai_model_lifecycle()
    }
}

pub(crate) async fn download(
    application: &(impl AiModelHost + ?Sized),
    slug: &str,
) -> Result<(), String> {
    let model = require_model(slug)?;
    let token = CancellationToken::new();
    let downloaded_bytes = Arc::new(AtomicU64::new(0));
    let total_bytes = model.size_bytes
        + model
            .label_categories
            .as_ref()
            .map_or(0, |artifact| artifact.size);
    {
        let mut downloads = application.ai_model_downloads().lock().await;
        if downloads.contains_key(&model.slug) {
            return Err(format!("Model '{}' is already downloading", model.slug));
        }
        downloads.insert(
            model.slug.clone(),
            AiModelDownload {
                cancel: token.clone(),
                downloaded_bytes: Arc::clone(&downloaded_bytes),
                total_bytes,
            },
        );
    }
    let result = crate::ai_tagger::download::download_model_quiet(
        &model.slug,
        &models_root(application),
        &token,
        downloaded_bytes,
        application.ai_model_lifecycle(),
    )
    .await;
    application
        .ai_model_downloads()
        .lock()
        .await
        .remove(&model.slug);
    if result.is_ok() {
        application.ai_sessions().lock().await.remove(&model.slug);
    }
    result
}

pub(crate) async fn cancel_download(
    application: &(impl AiModelHost + ?Sized),
    slug: &str,
) -> Result<(), String> {
    let model = require_model(slug)?;
    if let Some(token) = application
        .ai_model_downloads()
        .lock()
        .await
        .get(&model.slug)
    {
        token.cancel.cancel();
    }
    Ok(())
}

pub(crate) async fn delete(
    application: &(impl AiModelHost + ?Sized),
    slug: &str,
) -> Result<(), String> {
    let model = require_model(slug)?;
    if application
        .ai_model_downloads()
        .lock()
        .await
        .contains_key(&model.slug)
    {
        return Err(format!(
            "Model '{}' cannot be deleted while it is downloading",
            model.slug
        ));
    }
    let _lifecycle = application.ai_model_lifecycle().lock().await;
    application.ai_sessions().lock().await.remove(&model.slug);
    let directory = models::model_dir(&models_root(application), &model);
    if directory.exists() {
        std::fs::remove_dir_all(&directory)
            .map_err(|error| format!("Failed to delete model directory: {error}"))?;
    }
    Ok(())
}

pub(crate) async fn optimize(
    application: &(impl AiModelHost + ?Sized),
    slug: &str,
) -> Result<(), String> {
    let model = require_model(slug)?;
    let models_root = models_root(application);
    if !models::is_model_downloaded(&models_root, slug) {
        return Err(format!(
            "Model '{}' must be downloaded before optimization",
            model.label
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let directory = models::model_dir(&models_root, &model);
        let compiled = directory.join("model.mlmodelc");
        if models::is_model_optimized(&models_root, slug) {
            return Ok(());
        }
        let artifact = model
            .coreml
            .as_ref()
            .ok_or_else(|| format!("Model '{}' has no published Mac optimization", model.label))?;
        let cancel = CancellationToken::new();
        let downloaded_bytes = Arc::new(AtomicU64::new(0));
        {
            let mut downloads = application.ai_model_downloads().lock().await;
            if downloads.contains_key(slug) {
                return Err(format!(
                    "Model '{}' already has an active operation",
                    model.label
                ));
            }
            downloads.insert(
                slug.to_string(),
                AiModelDownload {
                    cancel: cancel.clone(),
                    downloaded_bytes: Arc::clone(&downloaded_bytes),
                    total_bytes: artifact.size,
                },
            );
        }
        let result = async {
            let staging = tempfile::Builder::new()
                .prefix(&format!(".{}.coreml-", model.slug))
                .tempdir_in(&models_root)
                .map_err(|error| format!("Failed to create Core ML staging directory: {error}"))?;
            crate::ai_tagger::download::download_coreml_package(
                artifact,
                staging.path(),
                &cancel,
                &downloaded_bytes,
            )
            .await?;
            if cancel.is_cancelled() {
                return Err("Model optimization cancelled".into());
            }
            let package = staging.path().join("model.mlpackage");
            let temporary =
                tokio::task::spawn_blocking(move || coreml_native::compile_model(&package))
                    .await
                    .map_err(|error| format!("Core ML optimization task failed: {error}"))?
                    .map_err(|error| format!("Failed to optimize model for this Mac: {error}"))?;
            let _lifecycle = application.ai_model_lifecycle().lock().await;
            application.ai_sessions().lock().await.remove(slug);
            if compiled.is_dir() {
                std::fs::remove_dir_all(&compiled).map_err(|error| {
                    format!(
                        "Failed to replace stale optimization {}: {error}",
                        compiled.display()
                    )
                })?;
            }
            std::fs::rename(&temporary, &compiled).map_err(|error| {
                format!(
                    "Failed to activate optimized model {}: {error}",
                    compiled.display()
                )
            })?;
            models::mark_coreml_artifact_current(&directory, &model)?;
            Ok(())
        }
        .await;
        application.ai_model_downloads().lock().await.remove(slug);
        result
    }
    #[cfg(not(target_os = "macos"))]
    Err("Model optimization is only required on macOS".into())
}

pub(crate) fn models_root(application: &(impl AiModelHost + ?Sized)) -> PathBuf {
    if let Some(root) = crate::state::application_data_root() {
        return root.join("models");
    }
    legacy_models_root(application)
}

fn legacy_models_root(application: &(impl AiModelHost + ?Sized)) -> PathBuf {
    application
        .library_root()
        .parent()
        .unwrap_or_else(|| application.library_root())
        .join("models")
}

pub(crate) fn migrate_legacy_storage(
    application: &(impl AiModelHost + ?Sized),
) -> Result<(), String> {
    let source = legacy_models_root(application);
    let target = models_root(application);
    if source == target || target.exists() || !source.exists() {
        return Ok(());
    }
    let parent = target
        .parent()
        .ok_or_else(|| "AI model storage has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to prepare application model storage: {error}"))?;
    std::fs::rename(&source, &target).map_err(|error| {
        format!("Failed to move legacy AI models into application storage: {error}")
    })
}

pub(crate) fn storage_bytes(application: &(impl AiModelHost + ?Sized)) -> Result<u64, String> {
    directory_bytes(&models_root(application))
}

fn directory_bytes(root: &Path) -> Result<u64, String> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("Failed to inspect AI model storage: {error}")),
    };
    let mut total = 0_u64;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Failed to inspect AI model storage: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Failed to inspect AI model storage: {error}"))?;
        let bytes = if file_type.is_dir() {
            directory_bytes(&entry.path())?
        } else {
            entry
                .metadata()
                .map_err(|error| format!("Failed to inspect AI model storage: {error}"))?
                .len()
        };
        total = total.saturating_add(bytes);
    }
    Ok(total)
}

fn require_model(slug: &str) -> Result<ModelInfo, String> {
    models::find_model(slug).ok_or_else(|| format!("Unknown model: {slug}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn canonical_application_owns_model_lifecycle_state() {
        let directory = tempfile::tempdir().unwrap();
        let application =
            crate::library_application::LibraryApplication::create(directory.path()).unwrap();

        assert!(cancel_download(&application, "unknown").await.is_err());
        assert!(delete(&application, "unknown").await.is_err());
        assert!(optimize(&application, "unknown").await.is_err());
        assert!(!models_root(&application).as_os_str().is_empty());
    }

    #[test]
    fn model_storage_counts_nested_artifacts() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("model.mlmodelc/weights")).unwrap();
        std::fs::write(directory.path().join("model.onnx"), [0_u8; 7]).unwrap();
        std::fs::write(
            directory.path().join("model.mlmodelc/weights/weight.bin"),
            [0_u8; 11],
        )
        .unwrap();

        assert_eq!(directory_bytes(directory.path()).unwrap(), 18);
    }
}
