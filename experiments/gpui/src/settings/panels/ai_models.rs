//! The AI Models section (`AiTaggingPanel.tsx`): the models production
//! registers, the confidence thresholds and automatic tagging.
use crate::i18n::t;
use crate::settings::model::Threshold;
use crate::settings::pieces::{
    block, card, control, edit, hint, label, number, row, separator, title, value, value_label,
};
use crate::settings::window::SettingsWindow;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look};
use gpui::*;

/// `fmtSize` in the AI Models panel: decimal gigabytes with trailing zeros
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

/// The AI models production registers (`known_models`), shown before any
/// is downloaded.
struct ModelInfo {
    slug: &'static str,
    short: &'static str,
    label: &'static str,
    dataset: &'static str,
    size: u64,
    ms: f32,
}

const MODELS: [ModelInfo; 4] = [
    ModelInfo {
        slug: "wd14-swinv2-v3",
        short: "WD14 SWN",
        label: "WD14 SwinV2 v3",
        dataset: "Danbooru · general, character, rating",
        size: 467_000_000,
        ms: 17.74,
    },
    ModelInfo {
        slug: "z3d-e621-convnext",
        short: "Z3D",
        label: "E621 ConvNext (Z3D)",
        dataset: "e621 · general, creator, series, character, species",
        size: 390_000_000,
        ms: 14.71,
    },
    ModelInfo {
        slug: "wd14-eva02-large-v3",
        short: "WD14 EVA",
        label: "WD14 EVA02-Large v3",
        dataset: "Danbooru · general, character, rating · Highest accuracy",
        size: 1_260_000_000,
        ms: 48.38,
    },
    ModelInfo {
        slug: "oppai-oracle-v1-1",
        short: "OppaiOracle",
        label: "OppaiOracle V1.1",
        dataset: "Danbooru · wide-vocabulary general tags",
        size: 993_246_982,
        ms: 86.45,
    },
];

// i18n: translated where drawn
const THRESHOLDS: [(Threshold, &str, &str); 6] = [
    (Threshold::General, "General", "general"),
    (Threshold::Character, "Character", "character"),
    (Threshold::Species, "Species", "species"),
    (Threshold::Copyright, "Series", "series"),
    (Threshold::Artist, "Creator", "creator"),
    (Threshold::Rating, "Rating", "rating"),
];

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
    pub(super) fn ai_models(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = model.read(cx).draft.app.clone();
        // TODO: read which models are downloaded and optimized from the
        // runtime's status and download, optimize and delete through it.
        // Until then every model shows as not downloaded; only the storage
        // they take is read.
        let mut models = card(theme).child(title(theme, t("Models")));
        for (index, info) in MODELS.iter().enumerate() {
            if index > 0 {
                models = models.child(separator(theme));
            }
            let metadata = format!(
                "{} · {} · ≈{} ms/image",
                info.dataset,
                model_size(info.size),
                info.ms.round() as i64
            );
            let name = ui::tooltip::attach(
                div()
                    .id(("model-name", index))
                    .flex_basis(px(92.))
                    .flex_shrink(1.)
                    .min_w_0()
                    .text(theme::TEXT_MD)
                    .text_color(theme.text())
                    .truncate()
                    .child(info.short),
                ("model-name-tip", index),
                ui::tip(info.label),
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
            models = models.child(
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
                    .child(
                        div()
                            .w(px(286.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                ui::action_button(("model-download", index), Look::Standard)
                                    .label(t("Download"))
                                    .width(92.)
                                    .padding(0.),
                            ),
                    ),
            );
        }
        let models = models.child(separator(theme)).child(row(
            theme,
            t("Model storage"),
            value(theme, self.ai_storage.map_or("—".into(), model_size)),
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
        let downloaded = false;
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
                            .disabled(!auto && !downloaded)
                            .on_toggle(edit(model, |draft| {
                                draft.app.ai_tagger_auto_on_import =
                                    !draft.app.ai_tagger_auto_on_import
                            })),
                    ),
                ),
        );
        // Each downloaded model's own switch shows while auto-tagging is on.
        for info in MODELS.iter().filter(|_| auto && downloaded) {
            behavior = behavior.child(separator(theme)).child(
                div()
                    .flex()
                    .items_center()
                    .min_h(px(26.))
                    .child(copy(theme, info.short, info.dataset))
                    .child(control().child(ui::toggle(info.slug, app.runs_model(info.slug)))),
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
        vec![
            block()
                .child(models)
                .child(hint(
                    theme,
                    t("Selected models run locally one after another. Warm single-image reference on an Apple M5 Pro; actual speed varies by device and batch size. Picto never uploads media for AI tagging."),
                ))
                .into_any_element(),
            block().child(confidence).into_any_element(),
            block().child(behavior).into_any_element(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::model_size;

    #[test]
    fn sizes_are_written_as_production_writes_them() {
        assert_eq!(model_size(1_260_000_000), "1.26 GB");
        assert_eq!(model_size(2_000_000_000), "2 GB");
        assert_eq!(model_size(993_246_982), "993 MB");
        assert_eq!(model_size(0), "0 MB");
    }
}
