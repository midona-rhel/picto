//! The AI Tagging section (`AiTaggingPanel.tsx`): the runtime's models with
//! their state and the commands that download, optimize and delete them,
//! the confidence thresholds, and automatic tagging. Model commands act at
//! once, as production's do; the thresholds and switches are part of the
//! draft that Save & Close or Apply writes.
use crate::ai_tagger::{self, model_size, short_label};
use crate::i18n::{en, t};
use crate::settings::model::{AppSettings, Threshold};
use crate::settings::pieces::{
    block, card, control, edit, hint, label, number, row, separator, title, value, value_label,
};
use crate::settings::window::SettingsWindow;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look};
use gpui::*;
use picto_core::ai_runtime::{AiModelStatus, AiRuntimeStatus};
use std::collections::BTreeSet;
use std::time::Duration;

/// While a model downloads or is optimized, its progress is read this
/// often.
const POLL: Duration = Duration::from_millis(200);

// i18n: translated where drawn
const THRESHOLDS: [(Threshold, &str, &str); 6] = [
    (Threshold::General, "General", "general"),
    (Threshold::Character, "Character", "character"),
    (Threshold::Species, "Species", "species"),
    (Threshold::Copyright, "Series", "series"),
    (Threshold::Artist, "Creator", "creator"),
    (Threshold::Rating, "Rating", "rating"),
];

/// The section's own state: the runtime's status and the operations under
/// way.
#[derive(Default)]
pub struct AiState {
    status: Option<AiRuntimeStatus>,
    error: Option<String>,
    downloading: BTreeSet<String>,
    optimizing: BTreeSet<String>,
    poll: Option<Task<()>>,
}

/// How far a download or optimization has gone, in percent.
fn operation_percent(model: &AiModelStatus) -> f64 {
    let total = model.download_total_bytes.unwrap_or(model.size_bytes);
    if total == 0 {
        return 0.;
    }
    (model.downloaded_bytes.unwrap_or(0) as f64 / total as f64 * 100.).clamp(0., 100.)
}

/// Auto-tag new imports switched on: the models already chosen for it stay;
/// with none chosen among those downloaded, the recommended one (else the
/// first downloaded) is chosen too.
fn enable_auto_tag(settings: &mut AppSettings, models: &[AiModelStatus]) {
    settings.ai_tagger_auto_on_import = true;
    let downloaded = models.iter().filter(|model| model.downloaded);
    if downloaded
        .clone()
        .any(|model| settings.runs_model(&model.slug))
    {
        return;
    }
    let mut candidates = downloaded;
    let fallback = candidates
        .clone()
        .find(|model| model.recommended)
        .or_else(|| candidates.next());
    if let Some(fallback) = fallback {
        settings.set_runs_model(&fallback.slug, true);
    }
}

fn copy(
    theme: theme::Theme,
    name: impl Into<SharedString>,
    description: impl Into<SharedString>,
) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .text(theme::TEXT_MD)
                // Mantine's body `line-height: 1.55`.
                .line_height(px(theme::TEXT_MD * 1.55))
                .text_color(theme.text())
                .child(name.into()),
        )
        .child(
            div()
                .mt(px(2.))
                .text(theme::TEXT_SM)
                .line_height(px(theme::TEXT_SM * 1.4))
                .text_color(theme.text_tertiary())
                .child(description.into()),
        )
}

impl SettingsWindow {
    /// Reads the runtime's status again.
    pub(in crate::settings) fn refresh_ai(&mut self, cx: &mut Context<Self>) {
        let session = self.model.read(cx).session.clone();
        let status = ai_tagger::status(session.read(cx));
        cx.spawn(async move |this, cx| {
            let status = status.await;
            let _ = this.update(cx, |this, cx| {
                match status {
                    Ok(status) => this.ai.status = Some(status),
                    Err(error) => this.ai.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// While an operation goes on, its progress is read every 200 ms.
    fn poll_ai(&mut self, cx: &mut Context<Self>) {
        if self.ai.poll.is_some() {
            return;
        }
        self.ai.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(going) = this.update(cx, |this, cx| {
                    let going = !this.ai.downloading.is_empty() || !this.ai.optimizing.is_empty();
                    if going {
                        this.refresh_ai(cx);
                    } else {
                        this.ai.poll = None;
                    }
                    going
                }) else {
                    return;
                };
                if !going {
                    return;
                }
            }
        }));
    }

    /// Sends a model command; `then` has its answer.
    fn model_command(
        &mut self,
        command: &'static str,
        slug: &str,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<serde_json::Value, String>, &mut Context<Self>) + 'static,
    ) {
        let session = self.model.read(cx).session.clone();
        let answer = ai_tagger::send(session.read(cx), command, slug);
        cx.spawn(async move |this, cx| {
            let result = answer.await.unwrap_or_else(|_| Err("cancelled".into()));
            let _ = this.update(cx, |this, cx| {
                then(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// `startDownload`: the command answers once the model is downloaded,
    /// fails, or is cancelled; a cancel is not an error.
    fn download_model(&mut self, slug: String, cx: &mut Context<Self>) {
        self.ai.error = None;
        self.ai.downloading.insert(slug.clone());
        self.poll_ai(cx);
        self.model_command(
            "ai.models.download",
            &slug.clone(),
            cx,
            move |this, result, cx| {
                match result {
                    Ok(value) => {
                        if let Ok(status) = serde_json::from_value(value) {
                            this.ai.status = Some(status);
                        }
                    }
                    Err(error) => {
                        if !error.to_lowercase().contains("cancelled") {
                            this.ai.error = Some(error);
                        }
                    }
                }
                this.ai.downloading.remove(&slug);
                this.refresh_ai(cx);
            },
        );
        cx.notify();
    }

    fn optimize_model(&mut self, slug: String, cx: &mut Context<Self>) {
        self.ai.error = None;
        self.ai.optimizing.insert(slug.clone());
        self.poll_ai(cx);
        self.model_command(
            "ai.models.optimize",
            &slug.clone(),
            cx,
            move |this, result, cx| {
                match result {
                    Ok(value) => {
                        if let Ok(status) = serde_json::from_value(value) {
                            this.ai.status = Some(status);
                        }
                    }
                    Err(error) => {
                        this.ai.error = Some(error);
                        this.refresh_ai(cx);
                    }
                }
                this.ai.optimizing.remove(&slug);
            },
        );
        cx.notify();
    }

    fn cancel_model(&mut self, slug: &str, cx: &mut Context<Self>) {
        self.model_command("ai.models.cancel", slug, cx, |this, result, _| {
            if let Err(error) = result {
                this.ai.error = Some(error);
            }
        });
    }

    fn delete_model(&mut self, slug: &str, cx: &mut Context<Self>) {
        self.model_command(
            "ai.models.delete",
            slug,
            cx,
            |this, result, cx| match result {
                Ok(_) => this.refresh_ai(cx),
                Err(error) => this.ai.error = Some(error),
            },
        );
    }

    /// `.error`: a failed model command, in the danger colour.
    fn ai_error(&self, cx: &App) -> Option<AnyElement> {
        let theme = theme::theme(cx);
        let error = self.ai.error.clone()?;
        Some(
            div()
                .mb(px(12.))
                .text(theme::TEXT_SM)
                .text_color(theme.negative())
                .child(error)
                .into_any_element(),
        )
    }

    /// One model's row: its name and facts, and its state with the
    /// commands that change it, over the 2px progress track.
    fn model_row(
        &self,
        index: usize,
        model: &AiModelStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let downloading = self.ai.downloading.contains(&model.slug);
        let optimizing = self.ai.optimizing.contains(&model.slug);
        let active = downloading || optimizing;
        let percent = operation_percent(model);
        let metadata = format!(
            "{} · {} · ≈{} ms/image",
            model.dataset,
            model_size(model.size_bytes),
            model.reference_inference_ms.round() as i64
        );
        let short = short_label(&model.slug).map_or(model.label.clone(), str::to_owned);
        let name = ui::tooltip::attach(
            div()
                .id(("model-name", index))
                .flex_basis(px(92.))
                .flex_shrink(1.)
                .min_w_0()
                .text(theme::TEXT_MD)
                .text_color(theme.text())
                .truncate()
                .child(short),
            ("model-name-tip", index),
            ui::tip(model.label.clone()),
            theme::TEXT_MD + 2.,
            window,
            cx,
        );
        let meta = ui::tooltip::attach(
            div()
                .id(("model-meta", index))
                .flex_1()
                .min_w_0()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .truncate()
                .child(metadata.clone()),
            ("model-meta-tip", index),
            ui::tip(metadata),
            theme::TEXT_SM + 2.,
            window,
            cx,
        );
        let button = |id: &'static str, text: &'static str| {
            ui::action_button((id, index), Look::Standard)
                .label(text)
                .width(92.)
                .padding(0.)
        };
        let slug = model.slug.clone();
        let mut state = div()
            .w(px(286.))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(10.));
        if active {
            let operation = if optimizing {
                en("Optimizing")
            } else {
                en("Downloading")
            };
            state =
                state.child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            div()
                                .min_w_0()
                                .min_h(px(18.))
                                .mr_auto()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text(theme::TEXT_SM)
                                .line_height(px(18.))
                                .text_color(theme.text_secondary())
                                .child(div().truncate().child(operation))
                                .child(
                                    div()
                                        .w(px(40.))
                                        .flex_none()
                                        .flex()
                                        .justify_end()
                                        .font_family(theme::FONT_MONO)
                                        .text_color(theme.text())
                                        .child(format!("{}%", percent.round() as i64)),
                                ),
                        )
                        .child(button("model-cancel", t("Cancel")).on_click(
                            cx.listener(move |this, _, _, cx| this.cancel_model(&slug, cx)),
                        )),
                );
        } else if model.downloaded {
            state = state.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_secondary())
                    .child(ui::icon("ai-check", 13., theme.primary()))
                    .child(if model.optimized {
                        t("Optimized")
                    } else {
                        t("Downloaded")
                    }),
            );
            if model.optimization_supported && !model.optimized {
                let slug = slug.clone();
                state =
                    state.child(
                        button("model-optimize", t("Optimize"))
                            .tip(ui::tip(t("Optimize for this Mac")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.optimize_model(slug.clone(), cx)
                            })),
                    );
            }
            state = state.child(
                button("model-delete", t("Delete"))
                    .on_click(cx.listener(move |this, _, _, cx| this.delete_model(&slug, cx))),
            );
        } else {
            state = state.child(button("model-download", t("Download")).on_click(
                cx.listener(move |this, _, _, cx| this.download_model(slug.clone(), cx)),
            ));
        }
        // `.modelProgressTrack`: 2px under the row, drawn only while an
        // operation runs.
        let mut track = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(-2.))
            .h(px(2.))
            .overflow_hidden();
        if active {
            track = track.bg(theme.border_secondary()).child(
                div()
                    .h_full()
                    .w(relative((percent / 100.) as f32))
                    .bg(theme.primary()),
            );
        }
        div()
            .relative()
            .h(px(32.))
            .pb(px(2.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .child(name)
                    .child(meta),
            )
            .child(state)
            .child(track)
    }

    pub(super) fn ai_models(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = model.read(cx).draft.app.clone();
        // Nothing until the runtime has answered, as production's panel.
        let Some(status) = self.ai.status.clone() else {
            return self.ai_error(cx).into_iter().collect();
        };

        let mut models = card(theme).child(title(theme, t("Models")));
        for (index, info) in status.models.iter().enumerate() {
            if index > 0 {
                models = models.child(separator(theme));
            }
            models = models.child(self.model_row(index, info, window, cx));
        }
        let models = models.child(separator(theme)).child(row(
            theme,
            t("Model storage"),
            value(theme, model_size(status.storage_bytes)),
        ));

        let mut confidence = card(theme).child(title(theme, t("Confidence")));
        for (index, (key, name, namespace)) in THRESHOLDS.iter().enumerate() {
            if index > 0 {
                confidence = confidence.child(separator(theme));
            }
            let percent = (app.threshold(*key) * 100.).round();
            let key = *key;
            confidence = confidence.child(
                div()
                    .flex()
                    .items_center()
                    .min_h(px(26.))
                    .child(
                        label(theme)
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(
                                div()
                                    .size(px(8.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(rgb(crate::tags::group_preset_color(namespace)))
                                    .opacity(0.75),
                            )
                            .child(t(name)),
                    )
                    .child(
                        control()
                            .child(
                                ui::slider(("threshold", index), percent, 5., 95., 1.).on_change(
                                    number(model, move |draft, value: f64| {
                                        draft.app.set_threshold(key, value / 100.)
                                    }),
                                ),
                            )
                            .child(
                                value_label(theme, format!("{}%", percent as i64))
                                    .w(px(36.))
                                    .flex()
                                    .justify_end(),
                            ),
                    ),
            );
        }
        let confidence = confidence.child(hint(
            theme,
            t("Tags below these confidence levels are hidden by default. You can adjust the cutoff for an individual run."),
        ));

        let auto = app.ai_tagger_auto_on_import;
        let downloaded = status
            .models
            .iter()
            .filter(|model| model.downloaded)
            .cloned()
            .collect::<Vec<_>>();
        let all_models = status.models.clone();
        let mut behavior = card(theme).child(title(theme, t("Behavior"))).child(
            div()
                .flex()
                .items_center()
                .min_h(px(26.))
                .child(copy(
                    theme,
                    t("Auto-tag new imports"),
                    t("Run selected models after import and apply tags above the confidence levels."),
                ))
                .child(
                    control().child(
                        ui::toggle("auto-tag", auto)
                            .disabled(!auto && downloaded.is_empty())
                            .on_toggle(edit(model, move |draft| {
                                if draft.app.ai_tagger_auto_on_import {
                                    draft.app.ai_tagger_auto_on_import = false;
                                } else {
                                    enable_auto_tag(&mut draft.app, &all_models);
                                }
                            })),
                    ),
                ),
        );
        // Each downloaded model's own switch shows while auto-tagging is on.
        for info in downloaded
            .iter()
            .filter(|info| auto && AppSettings::has_model_setting(&info.slug))
        {
            let slug = info.slug.clone();
            let short = short_label(&info.slug).map_or(info.label.clone(), str::to_owned);
            behavior = behavior.child(separator(theme)).child(
                div()
                    .flex()
                    .items_center()
                    .min_h(px(26.))
                    .child(copy(theme, short, info.dataset.clone()))
                    .child(
                        control().child(
                            ui::toggle(
                                SharedString::from(info.slug.clone()),
                                app.runs_model(&info.slug),
                            )
                            .on_toggle(edit(model, move |draft| {
                                let on = draft.app.runs_model(&slug);
                                draft.app.set_runs_model(&slug, !on);
                            })),
                        ),
                    ),
            );
        }
        let behavior = behavior.child(separator(theme)).child(
            div()
                .flex()
                .items_center()
                .min_h(px(26.))
                .child(copy(
                    theme,
                    t("Write rating tags"),
                    t("Store general, sensitive, questionable, or explicit as a rating tag."),
                ))
                .child(control().child(
                    ui::toggle("write-rating", app.ai_tagger_write_rating).on_toggle(edit(
                        model,
                        |draft| {
                            draft.app.ai_tagger_write_rating = !draft.app.ai_tagger_write_rating
                        },
                    )),
                )),
        );
        self.ai_error(cx)
            .into_iter()
            .chain([
                block()
                    .child(models)
                    .child(hint(
                        theme,
                        t("Selected models run locally one after another. Warm single-image reference on an Apple M5 Pro; actual speed varies by device and batch size. Picto never uploads media for AI tagging."),
                    ))
                    .into_any_element(),
                block().child(confidence).into_any_element(),
                block().child(behavior).into_any_element(),
            ])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{enable_auto_tag, operation_percent};
    use crate::settings::model::AppSettings;
    use picto_core::ai_runtime::AiModelStatus;

    fn model(slug: &str, downloaded: bool, recommended: bool) -> AiModelStatus {
        AiModelStatus {
            slug: slug.into(),
            label: slug.into(),
            enabled: false,
            downloaded,
            session_loaded: false,
            recommended,
            heavy: !recommended,
            optimization_supported: false,
            optimized: false,
            downloaded_bytes: None,
            download_total_bytes: None,
            size_bytes: 400,
            dataset: String::new(),
            reference_inference_ms: 0.,
        }
    }

    #[test]
    fn progress_is_the_share_of_the_download_total() {
        let mut downloading = model("wd14-swinv2-v3", false, true);
        assert_eq!(operation_percent(&downloading), 0.);
        downloading.downloaded_bytes = Some(100);
        assert_eq!(operation_percent(&downloading), 25.);
        downloading.download_total_bytes = Some(200);
        assert_eq!(operation_percent(&downloading), 50.);
        downloading.downloaded_bytes = Some(500);
        assert_eq!(operation_percent(&downloading), 100.);
    }

    #[test]
    fn turning_auto_tagging_on_chooses_a_downloaded_model_if_none_is() {
        let models = [
            model("wd14-eva02-large-v3", true, false),
            model("wd14-swinv2-v3", true, true),
            model("z3d-e621-convnext", false, true),
        ];
        let mut settings = AppSettings::default();
        enable_auto_tag(&mut settings, &models);
        assert!(settings.ai_tagger_auto_on_import);
        assert!(settings.runs_model("wd14-swinv2-v3"));
        assert!(!settings.runs_model("wd14-eva02-large-v3"));
        // One already chosen stays the only one.
        let mut settings = AppSettings::default();
        settings.set_runs_model("wd14-eva02-large-v3", true);
        enable_auto_tag(&mut settings, &models);
        assert!(!settings.runs_model("wd14-swinv2-v3"));
        // No recommended model downloaded: the first downloaded.
        let mut settings = AppSettings::default();
        enable_auto_tag(&mut settings, &models[..1]);
        assert!(settings.runs_model("wd14-eva02-large-v3"));
    }
}
