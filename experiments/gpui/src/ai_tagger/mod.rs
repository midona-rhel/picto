//! The AI tagger (`aiTaggerApi.ts`, `AiTaggerPanel.tsx`): the core's local
//! models, read and managed through its `ai.*` commands, and the review
//! panel that suggests tags for items and writes those accepted.
//!
//! Model downloads go to the network. With `PICTO_GPUI_HOLD_DOWNLOADS` set
//! (debug builds), [`send`] writes each model command down and answers
//! downloads itself instead of sending them, as `PICTO_GPUI_HOLD_RUNS` does
//! for subscription runs.
use crate::session::Session;
use futures::channel::oneshot;
use picto_core::ai_runtime::AiRuntimeStatus;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;

mod panel;
pub mod review;
mod view;

pub use panel::AiTagger;

/// `MODEL_SHORT_LABELS`: the names the Settings panel gives the models.
pub fn short_label(slug: &str) -> Option<&'static str> {
    // i18n: model names, as production writes them
    match slug {
        "wd14-swinv2-v3" => Some("WD14 SWN"),
        "z3d-e621-convnext" => Some("Z3D"),
        "wd14-eva02-large-v3" => Some("WD14 EVA"),
        "oppai-oracle-v1-1" => Some("OppaiOracle"),
        _ => None,
    }
}

/// `fmtSize` in the AI Tagging panel: decimal gigabytes with trailing zeros
/// dropped, else whole megabytes.
pub fn model_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        let text = format!("{:.2}", bytes as f64 / 1e9);
        let text = text.trim_end_matches('0').trim_end_matches('.');
        format!("{text} GB")
    } else {
        format!("{} MB", (bytes as f64 / 1e6).round())
    }
}

/// Reads the runtime's models and thresholds (`ai.status`), with the
/// progress a held download reports.
pub fn status(
    session: &Session,
) -> impl std::future::Future<Output = Result<AiRuntimeStatus, String>> + use<> {
    let answer = session.dispatch("ai.status", json!({}));
    async move {
        let value = answer.await.map_err(|_| "cancelled".to_owned())??;
        let mut status: AiRuntimeStatus =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        held_progress(&mut status);
        Ok(status)
    }
}

/// Sends a model command (`ai.models.download`, `.cancel`, `.delete`,
/// `.optimize`) for `slug`, and hands back its answer. Held, a download is
/// answered here and never reaches the core.
pub fn send(
    session: &Session,
    command: &'static str,
    slug: &str,
) -> oneshot::Receiver<Result<Value, String>> {
    let args = json!({ "slug": slug });
    let Some(held) = crate::seams::hold_downloads() else {
        return session.dispatch(command, args);
    };
    eprintln!("picto: AI command {command} {args}");
    match command {
        "ai.models.download" => {
            let (answer, receiver) = oneshot::channel();
            match held {
                crate::seams::HeldDownload::Fail => {
                    let _ = answer.send(Err(format!(
                        "{command} for {slug} was held (PICTO_GPUI_HOLD_DOWNLOADS=fail)"
                    )));
                }
                crate::seams::HeldDownload::Progress(_) => {
                    HELD.with(|held| held.borrow_mut().insert(slug.to_owned(), answer));
                }
            }
            receiver
        }
        "ai.models.cancel" if HELD.with(|held| held.borrow().contains_key(slug)) => {
            // As the core ends a cancelled download: its command fails.
            if let Some(download) = HELD.with(|held| held.borrow_mut().remove(slug)) {
                let _ = download.send(Err("Download cancelled".into()));
            }
            let (answer, receiver) = oneshot::channel();
            let _ = answer.send(Ok(json!({})));
            receiver
        }
        _ => session.dispatch(command, args),
    }
}

thread_local! {
    /// The downloads held, by model, each with the answer it is waiting on.
    static HELD: RefCell<HashMap<String, oneshot::Sender<Result<Value, String>>>> =
        RefCell::default();
}

/// A held download reports the share `PICTO_GPUI_HOLD_DOWNLOADS` names, as
/// the core reports a download under way.
fn held_progress(status: &mut AiRuntimeStatus) {
    let Some(crate::seams::HeldDownload::Progress(share)) = crate::seams::hold_downloads() else {
        return;
    };
    HELD.with(|held| {
        let held = held.borrow();
        for model in &mut status.models {
            if held.contains_key(&model.slug) {
                let total = model.size_bytes;
                model.download_total_bytes = Some(total);
                model.downloaded_bytes = Some((total as f64 * share) as u64);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{model_size, short_label};

    #[test]
    fn sizes_are_written_as_production_writes_them() {
        assert_eq!(model_size(1_260_000_000), "1.26 GB");
        assert_eq!(model_size(2_000_000_000), "2 GB");
        assert_eq!(model_size(2_600_000_000), "2.6 GB");
        assert_eq!(model_size(993_246_982), "993 MB");
        assert_eq!(model_size(0), "0 MB");
    }

    #[test]
    fn known_models_have_their_short_names() {
        assert_eq!(short_label("wd14-swinv2-v3"), Some("WD14 SWN"));
        assert_eq!(short_label("z3d-e621-convnext"), Some("Z3D"));
        assert_eq!(short_label("something-else"), None);
    }
}
