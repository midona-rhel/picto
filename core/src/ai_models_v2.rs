//! Replacement AI model inventory and filesystem lifecycle.

use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use crate::ai_tagger::models::{self, ModelInfo};
use crate::app::Application;

pub async fn download(application: &Application, slug: &str) -> Result<(), String> {
    let model = require_model(slug)?;
    let token = CancellationToken::new();
    {
        let mut downloads = application.ai_model_downloads().lock().await;
        if downloads.contains_key(&model.slug) {
            return Err(format!("Model '{}' is already downloading", model.slug));
        }
        downloads.insert(model.slug.clone(), token.clone());
    }
    let result = crate::ai_tagger::download::download_model_quiet(
        &model.slug,
        &models_root(application),
        &token,
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

pub async fn cancel_download(application: &Application, slug: &str) -> Result<(), String> {
    let model = require_model(slug)?;
    if let Some(token) = application
        .ai_model_downloads()
        .lock()
        .await
        .get(&model.slug)
    {
        token.cancel();
    }
    Ok(())
}

pub async fn delete(application: &Application, slug: &str) -> Result<(), String> {
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

pub fn models_root(application: &Application) -> PathBuf {
    application
        .store()
        .library_root()
        .parent()
        .unwrap_or_else(|| application.store().library_root())
        .join("models")
}

fn require_model(slug: &str) -> Result<ModelInfo, String> {
    models::find_model(slug).ok_or_else(|| format!("Unknown model: {slug}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Store;

    #[tokio::test]
    async fn unknown_models_are_rejected_without_touching_the_filesystem() {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));

        assert!(cancel_download(&application, "unknown").await.is_err());
        assert!(delete(&application, "unknown").await.is_err());
    }
}
