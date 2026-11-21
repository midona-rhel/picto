//! The rules of the review, without the drawing: how the predictions for one
//! item become rows, which rows are checked, what Apply writes, which models a
//! review starts with, and how failures are worded.
use crate::i18n::t;
use picto_core::ai_runtime::{AiModelStatus, AiTagPrediction, RootPrediction};
use picto_library::{RootId, RootTagAssignment};
use std::collections::HashMap;

/// The smallest value of the review's confidence cutoff, in percent.
/// `MAX_CONFIDENCE` is the largest value, and `DEFAULT_CONFIDENCE` is the
/// value that the cutoff has before the thresholds in Settings have been read.
pub const MIN_CONFIDENCE: u32 = 5;
pub const MAX_CONFIDENCE: u32 = 95;
pub const DEFAULT_CONFIDENCE: u32 = 35;
/// The number of items whose details are read at a time, and the largest
/// number of thumbnails that the review shows around the reviewed item.
pub const WINDOW: usize = 20;

/// One suggested tag of an item. It merges the predictions of that tag by
/// every model that was run.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewTag {
    /// The tag as Apply writes it: `namespace:subtag`, or the subtag alone
    /// when the namespace is empty, `general` or `default`.
    pub key: String,
    pub namespace: String,
    pub subtag: String,
    /// The highest confidence that any model gave the tag.
    pub confidence: f32,
    /// Each model that predicted the tag, with the confidence that this model
    /// gave it.
    pub models: Vec<(String, f32)>,
}

impl ReviewTag {
    /// Returns whether the namespace is written before the tag. It is not
    /// written when it is empty, `general` or `default`.
    pub fn shows_namespace(&self) -> bool {
        shows_namespace(&self.namespace)
    }
}

fn shows_namespace(namespace: &str) -> bool {
    !namespace.is_empty() && namespace != "general" && namespace != "default"
}

/// Returns the text under which a prediction is written as a tag:
/// `namespace:tag`, or the tag alone when the namespace is empty, `general` or
/// `default`.
pub fn tag_key(namespace: &str, tag: &str) -> String {
    if shows_namespace(namespace) {
        format!("{namespace}:{tag}")
    } else {
        tag.to_owned()
    }
}

/// Returns the rows for one item: its predictions by the chosen models, one
/// row for each tag, with the highest confidence first. Tags of equal
/// confidence keep the order in which their predictions arrived.
pub fn prediction_tags(prediction: Option<&RootPrediction>, models: &[String]) -> Vec<ReviewTag> {
    let Some(prediction) = prediction else {
        return Vec::new();
    };
    let mut tags: Vec<ReviewTag> = Vec::new();
    let mut index = HashMap::new();
    for tag in &prediction.predictions {
        if !models.contains(&tag.model) {
            continue;
        }
        let key = tag_key(&tag.namespace, &tag.tag);
        let at = *index.entry(key.clone()).or_insert_with(|| {
            tags.push(ReviewTag {
                key,
                namespace: tag.namespace.clone(),
                subtag: tag.tag.clone(),
                confidence: 0.,
                models: Vec::new(),
            });
            tags.len() - 1
        });
        let current = &mut tags[at];
        current.confidence = current.confidence.max(tag.confidence);
        match current
            .models
            .iter_mut()
            .find(|(slug, _)| *slug == tag.model)
        {
            Some((_, confidence)) => *confidence = confidence.max(tag.confidence),
            None => current.models.push((tag.model.clone(), tag.confidence)),
        }
    }
    tags.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    tags
}

/// Merges newly arrived predictions into `previous`. The answers arrive for
/// one model and one image at a time, so the predictions for the same item are
/// joined into one entry. For each model and tag the prediction with the
/// highest confidence is kept, and the first error of an item is kept.
pub fn merge(previous: &mut Vec<RootPrediction>, incoming: Vec<RootPrediction>) {
    for next in incoming {
        let at = previous
            .iter()
            .position(|entry| entry.root_id == next.root_id);
        let current = at.map(|at| &previous[at]);
        let mut merged: Vec<AiTagPrediction> = Vec::new();
        let mut keys: HashMap<(String, String, String), usize> = HashMap::new();
        let existing = current
            .map(|entry| entry.predictions.as_slice())
            .unwrap_or(&[]);
        for tag in existing.iter().chain(&next.predictions) {
            let key = (tag.model.clone(), tag.namespace.clone(), tag.tag.clone());
            match keys.get(&key) {
                Some(&index) if tag.confidence > merged[index].confidence => {
                    merged[index] = tag.clone()
                }
                Some(_) => {}
                None => {
                    keys.insert(key, merged.len());
                    merged.push(tag.clone());
                }
            }
        }
        let error = current.and_then(|entry| entry.error.clone()).or(next.error);
        let entry = RootPrediction {
            root_id: next.root_id,
            predictions: merged,
            error,
        };
        match at {
            Some(at) => previous[at] = entry,
            None => previous.push(entry),
        }
    }
}

/// The check marks that the user changed with a click, by item and tag. A tag
/// without an entry here is checked when its confidence is at or above the
/// cutoff.
#[derive(Clone, Debug, Default)]
pub struct Overrides(HashMap<(RootId, String), bool>);

impl Overrides {
    pub fn is_checked(&self, item: RootId, tag: &ReviewTag, cutoff: f32) -> bool {
        self.0
            .get(&(item, tag.key.clone()))
            .copied()
            .unwrap_or(tag.confidence >= cutoff)
    }

    pub fn set(&mut self, item: RootId, key: &str, checked: bool) {
        self.0.insert((item, key.to_owned()), checked);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// Returns what Apply writes: for each item, its checked tags. An item without
/// a checked tag is left out.
pub fn assignments(
    predictions: &[RootPrediction],
    models: &[String],
    overrides: &Overrides,
    cutoff: f32,
) -> Vec<RootTagAssignment> {
    predictions
        .iter()
        .filter_map(|prediction| {
            let tags = prediction_tags(Some(prediction), models)
                .into_iter()
                .filter(|tag| overrides.is_checked(prediction.root_id, tag, cutoff))
                .map(|tag| tag.key)
                .collect::<Vec<_>>();
            (!tags.is_empty()).then_some(RootTagAssignment {
                root_id: prediction.root_id,
                tags,
            })
        })
        .collect()
}

/// Returns the cutoff that a review starts at: the General confidence from
/// Settings, in whole percent and limited to the slider's range.
pub fn initial_confidence(general: f32) -> u32 {
    ((general * 100.).round() as i64).clamp(MIN_CONFIDENCE as i64, MAX_CONFIDENCE as i64) as u32
}

/// Returns the models that a review starts with. These are the models that
/// were chosen in the last review (`remembered`, the setting
/// `ai_tagger_manual_model_slugs`). If no review has chosen models yet, they
/// are the models that are set to tag imports (`configured`). Of either list,
/// only the downloaded models are taken. If no review has chosen models yet
/// and none of the models that tag imports is downloaded, the result is the
/// recommended downloaded model, or else the first downloaded model.
pub fn initial_models(
    models: &[AiModelStatus],
    configured: &[String],
    remembered: Option<&[String]>,
) -> Vec<String> {
    let downloaded = |slug: &String| models.iter().any(|m| m.downloaded && m.slug == *slug);
    let previous = remembered
        .unwrap_or(configured)
        .iter()
        .filter(|slug| downloaded(slug))
        .cloned()
        .collect::<Vec<_>>();
    if remembered.is_none() && previous.is_empty() {
        let fallback = models
            .iter()
            .find(|m| m.downloaded && m.recommended)
            .or_else(|| models.iter().find(|m| m.downloaded));
        if let Some(fallback) = fallback {
            return vec![fallback.slug.clone()];
        }
    }
    previous
}

/// Adds the model `slug` to the chosen models, or removes it if it is already
/// chosen. An added model is put at the end, so it runs after the models that
/// were chosen before it.
pub fn toggle_model(models: &mut Vec<String>, slug: &str) {
    match models.iter().position(|chosen| chosen == slug) {
        Some(at) => {
            models.remove(at);
        }
        None => models.push(slug.to_owned()),
    }
}

/// Returns how many tags the model `slug` predicted, counted over all items. A
/// tag that the model predicted for two items counts twice.
pub fn model_count(predictions: &[RootPrediction], slug: &str) -> usize {
    let mut keys = std::collections::HashSet::new();
    for prediction in predictions {
        for tag in &prediction.predictions {
            if tag.model == slug {
                keys.insert((prediction.root_id, &tag.namespace, &tag.tag));
            }
        }
    }
    keys.len()
}

/// Returns the error message of a finished run, or `None` if nothing failed.
/// If items could not be read, the message gives their number. Otherwise, if
/// analyses of an image by a model failed, the message gives their number,
/// `total`, and the text of the first failure.
pub fn failure(
    unprepared: usize,
    failed: usize,
    total: usize,
    first: Option<&str>,
) -> Option<String> {
    if unprepared > 0 {
        Some(crate::tf!(
            "{value0} selected items could not be prepared for AI review.",
            unprepared
        ))
    } else if failed > 0 {
        Some(crate::tf!(
            "{value0} of {value1} model/media analyses failed. {value2}",
            failed,
            total,
            first.unwrap_or("")
        ))
    } else {
        None
    }
}

/// Returns the label of the Apply button: the number of checked tags, followed
/// by the English noun "tag" or "tags".
pub fn apply_label(checked: usize) -> String {
    crate::tf!(
        "Apply {value0} {value1}",
        checked,
        if checked == 1 { "tag" } else { "tags" }
    )
}

/// The message that the list shows when it has no tag to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Empty {
    /// The run failed. The list shows the run's error message. When the list
    /// has tags, the error message is shown above them instead.
    Error,
    Preparing,
    NoItems,
    AnalyzingThis,
    WaitingForThis,
    NoModels,
    NothingBelow,
    ChooseModels,
    NoSuggestions,
}

/// The facts that decide which message an empty list shows.
pub struct EmptyState {
    pub error: bool,
    pub running: bool,
    pub resolved: bool,
    pub items: usize,
    pub this_running: bool,
    pub models: usize,
    pub below: bool,
    pub predictions: usize,
}

impl Empty {
    /// Returns the message that an empty list shows for `state`. The
    /// conditions are tested in the order written here, and the first one that
    /// holds decides.
    pub fn of(state: &EmptyState) -> Self {
        if state.error && !state.running {
            Empty::Error
        } else if !state.resolved {
            Empty::Preparing
        } else if state.items == 0 {
            Empty::NoItems
        } else if state.this_running {
            Empty::AnalyzingThis
        } else if state.running {
            Empty::WaitingForThis
        } else if state.models == 0 {
            Empty::NoModels
        } else if state.below {
            Empty::NothingBelow
        } else if state.predictions == 0 {
            Empty::ChooseModels
        } else {
            Empty::NoSuggestions
        }
    }

    /// Returns the message's text. `Error` has no text here, because the list
    /// shows the run's error message for it.
    pub fn text(self) -> &'static str {
        match self {
            Empty::Error => "",
            Empty::Preparing => t("Preparing preview…"),
            Empty::NoItems => t("Select specific library items to auto tag"),
            Empty::AnalyzingThis => t("Analyzing this item…"),
            Empty::WaitingForThis => t("Waiting for this item…"),
            Empty::NoModels => t("Select at least one downloaded model"),
            Empty::NothingBelow => t("Nothing below the cutoff"),
            Empty::ChooseModels => t("Choose models, then press Run"),
            Empty::NoSuggestions => t("No suggestions"),
        }
    }
}

/// Returns the items whose thumbnails the review shows: at most [`WINDOW`]
/// items around the reviewed item. The reviewed item is in the middle, except
/// near the start or the end of `items`.
pub fn window_around(items: &[RootId], active: RootId) -> &[RootId] {
    let index = items.iter().position(|id| *id == active).unwrap_or(0);
    let last_start = items.len().saturating_sub(WINDOW);
    let start = index.saturating_sub(WINDOW / 2).min(last_start);
    &items[start..(start + WINDOW).min(items.len())]
}

/// Returns the name of the inference backend that the footer shows. It joins
/// the backends that the loaded sessions of the `chosen` models run on, each
/// named once, in the order of `models` ("Core ML GPU/ANE", or "Core ML
/// GPU/ANE + CPU" when they differ). Returns `None` while none of the chosen
/// models is loaded.
pub fn backend(models: &[AiModelStatus], chosen: &[String]) -> Option<String> {
    let mut backends: Vec<&str> = Vec::new();
    for model in models.iter().filter(|model| chosen.contains(&model.slug)) {
        if let Some(backend) = model.backend.as_deref()
            && !backends.contains(&backend)
        {
            backends.push(backend);
        }
    }
    (!backends.is_empty()).then(|| backends.join(" + "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(namespace: &str, name: &str, confidence: f32, model: &str) -> AiTagPrediction {
        AiTagPrediction {
            tag: name.into(),
            namespace: namespace.into(),
            confidence,
            model: model.into(),
        }
    }

    fn root(id: u32, predictions: Vec<AiTagPrediction>) -> RootPrediction {
        RootPrediction {
            root_id: RootId(id),
            predictions,
            error: None,
        }
    }

    #[test]
    fn the_footer_names_the_backends_of_the_chosen_models_only() {
        let loaded = |slug: &str, backend: Option<&str>| AiModelStatus {
            session_loaded: backend.is_some(),
            backend: backend.map(String::from),
            ..model(slug, true, true)
        };
        let models = [
            loaded("convnext", Some("CPU")),
            loaded("swin", Some("CPU")),
            loaded("eva", Some("Core ML GPU/ANE")),
            loaded("oracle", None),
        ];
        let chosen = |slugs: &[&str]| slugs.iter().map(|slug| (*slug).to_owned()).collect::<Vec<_>>();
        // A model that is loaded on the CPU but is not chosen does not add its
        // backend to the name.
        assert_eq!(
            backend(&models, &chosen(&["eva"])).as_deref(),
            Some("Core ML GPU/ANE")
        );
        assert_eq!(backend(&models, &chosen(&["convnext", "swin"])).as_deref(), Some("CPU"));
        assert_eq!(
            backend(&models, &chosen(&["swin", "eva"])).as_deref(),
            Some("CPU + Core ML GPU/ANE")
        );
        // A chosen model that is not loaded yet has no backend to name.
        assert_eq!(backend(&models, &chosen(&["oracle"])), None);
    }

    fn model(slug: &str, downloaded: bool, recommended: bool) -> AiModelStatus {
        AiModelStatus {
            slug: slug.into(),
            label: slug.into(),
            enabled: false,
            downloaded,
            session_loaded: false,
            backend: None,
            recommended,
            heavy: !recommended,
            optimization_supported: false,
            optimized: false,
            downloaded_bytes: None,
            download_total_bytes: None,
            size_bytes: 0,
            dataset: String::new(),
            reference_inference_ms: 0.,
        }
    }

    fn slugs(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn tags_merge_across_models_most_confident_first() {
        let prediction = root(
            1,
            vec![
                tag("general", "1girl", 0.4, "a"),
                tag("character", "hatsune_miku", 0.9, "a"),
                tag("general", "1girl", 0.7, "b"),
                tag("rating", "general", 0.8, "b"),
                tag("general", "solo", 0.6, "c"),
            ],
        );
        let tags = prediction_tags(Some(&prediction), &slugs(&["a", "b"]));
        let keys = tags.iter().map(|tag| tag.key.as_str()).collect::<Vec<_>>();
        // Model c is not among the chosen models, so its prediction is left
        // out. A general tag is written without its namespace.
        assert_eq!(keys, ["character:hatsune_miku", "rating:general", "1girl"]);
        assert_eq!(tags[2].confidence, 0.7);
        assert_eq!(
            tags[2].models,
            vec![("a".to_owned(), 0.4), ("b".to_owned(), 0.7)]
        );
        assert!(!tags[2].shows_namespace() && tags[0].shows_namespace());
        assert_eq!(tag_key("default", "x"), "x");
        assert_eq!(tag_key("", "x"), "x");
        assert!(prediction_tags(None, &slugs(&["a"])).is_empty());
    }

    #[test]
    fn answers_join_per_item_keeping_the_highest_confidence_and_first_error() {
        let mut all = vec![root(1, vec![tag("general", "cat", 0.5, "a")])];
        let mut failed = root(2, vec![]);
        failed.error = Some("first".into());
        merge(
            &mut all,
            vec![
                root(
                    1,
                    vec![
                        tag("general", "cat", 0.8, "a"),
                        tag("general", "cat", 0.3, "b"),
                    ],
                ),
                failed,
            ],
        );
        let mut later = root(2, vec![tag("general", "dog", 0.6, "a")]);
        later.error = Some("second".into());
        merge(&mut all, vec![later]);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].predictions.len(), 2);
        assert_eq!(all[0].predictions[0].confidence, 0.8);
        assert_eq!(all[1].error.as_deref(), Some("first"));
        assert_eq!(all[1].predictions.len(), 1);
    }

    #[test]
    fn checks_follow_the_cutoff_until_clicked_and_apply_writes_only_checked_tags() {
        let predictions = vec![
            root(
                1,
                vec![
                    tag("general", "cat", 0.9, "a"),
                    tag("general", "dog", 0.2, "a"),
                    tag("artist", "someone", 0.36, "a"),
                ],
            ),
            root(2, vec![tag("general", "tree", 0.1, "a")]),
        ];
        let models = slugs(&["a"]);
        let mut overrides = Overrides::default();
        let written = assignments(&predictions, &models, &overrides, 0.35);
        assert_eq!(written.len(), 1, "an item with nothing checked is left out");
        assert_eq!(written[0].root_id, RootId(1));
        assert_eq!(written[0].tags, ["cat", "artist:someone"]);
        // A click checks a tag below the cutoff and unchecks one above it.
        overrides.set(RootId(1), "dog", true);
        overrides.set(RootId(1), "cat", false);
        overrides.set(RootId(2), "tree", true);
        let written = assignments(&predictions, &models, &overrides, 0.35);
        assert_eq!(written[0].tags, ["artist:someone", "dog"]);
        assert_eq!(written[1].tags, ["tree"]);
        // A higher cutoff changes only the tags that the user did not click.
        let written = assignments(&predictions, &models, &overrides, 0.5);
        assert_eq!(written[0].tags, ["dog"]);
        // With no model chosen, nothing is written.
        assert!(assignments(&predictions, &[], &overrides, 0.35).is_empty());
    }

    #[test]
    fn the_cutoff_starts_at_the_general_threshold_within_the_range() {
        assert_eq!(initial_confidence(0.35), 35);
        assert_eq!(initial_confidence(0.678), 68);
        assert_eq!(initial_confidence(0.01), MIN_CONFIDENCE);
        assert_eq!(initial_confidence(1.), MAX_CONFIDENCE);
    }

    #[test]
    fn a_review_starts_with_the_models_last_chosen_that_are_downloaded() {
        let models = [
            model("heavy", true, false),
            model("light", true, true),
            model("missing", false, true),
        ];
        // The models of the last review are taken in their order, without
        // those that are not downloaded.
        assert_eq!(
            initial_models(&models, &[], Some(&slugs(&["missing", "light", "heavy"]))),
            ["light", "heavy"]
        );
        // If the last review ended with no model chosen, the next one starts
        // with none.
        assert!(initial_models(&models, &slugs(&["heavy"]), Some(&[])).is_empty());
        // If no review has chosen models yet, the models that tag imports are
        // taken.
        assert_eq!(initial_models(&models, &slugs(&["heavy"]), None), ["heavy"]);
        // If there are neither, the recommended downloaded model is taken, or
        // else the first downloaded model.
        assert_eq!(initial_models(&models, &[], None), ["light"]);
        let unrecommended = [model("x", false, true), model("y", true, false)];
        assert_eq!(initial_models(&unrecommended, &[], None), ["y"]);
        assert!(initial_models(&[model("x", false, true)], &[], None).is_empty());
        let mut chosen = slugs(&["a"]);
        toggle_model(&mut chosen, "b");
        toggle_model(&mut chosen, "a");
        assert_eq!(chosen, ["b"]);
    }

    #[test]
    fn failures_are_worded_as_the_earlier_version_worded_them() {
        assert_eq!(failure(0, 0, 4, None), None);
        assert_eq!(
            failure(2, 1, 4, Some("x")).as_deref(),
            Some("2 selected items could not be prepared for AI review.")
        );
        assert_eq!(
            failure(0, 1, 6, Some("Failed to decode image")).as_deref(),
            Some("1 of 6 model/media analyses failed. Failed to decode image")
        );
        assert_eq!(apply_label(1), "Apply 1 tag");
        assert_eq!(apply_label(12), "Apply 12 tags");
    }

    #[test]
    fn models_count_distinct_tags_per_item() {
        let predictions = vec![
            root(
                1,
                vec![
                    tag("general", "cat", 0.9, "a"),
                    tag("general", "cat", 0.5, "b"),
                ],
            ),
            root(
                2,
                vec![
                    tag("general", "cat", 0.9, "a"),
                    tag("general", "dog", 0.2, "a"),
                ],
            ),
        ];
        assert_eq!(model_count(&predictions, "a"), 3);
        assert_eq!(model_count(&predictions, "b"), 1);
        assert_eq!(model_count(&predictions, "c"), 0);
    }

    #[test]
    fn an_empty_list_says_why_in_the_earlier_versions_order() {
        let state = |change: &dyn Fn(&mut EmptyState)| {
            let mut state = EmptyState {
                error: false,
                running: false,
                resolved: true,
                items: 3,
                this_running: false,
                models: 1,
                below: false,
                predictions: 0,
            };
            change(&mut state);
            Empty::of(&state)
        };
        assert_eq!(state(&|_| {}), Empty::ChooseModels);
        assert_eq!(state(&|s| s.error = true), Empty::Error);
        assert_eq!(
            state(&|s| {
                s.error = true;
                s.running = true;
                s.this_running = true
            }),
            Empty::AnalyzingThis
        );
        assert_eq!(state(&|s| s.resolved = false), Empty::Preparing);
        assert_eq!(state(&|s| s.items = 0), Empty::NoItems);
        assert_eq!(state(&|s| s.running = true), Empty::WaitingForThis);
        assert_eq!(state(&|s| s.models = 0), Empty::NoModels);
        assert_eq!(state(&|s| s.below = true), Empty::NothingBelow);
        assert_eq!(state(&|s| s.predictions = 2), Empty::NoSuggestions);
    }

    #[test]
    fn the_rail_shows_twenty_items_around_the_one_reviewed() {
        let items = (0..50).map(RootId).collect::<Vec<_>>();
        assert_eq!(window_around(&items, RootId(0))[0], RootId(0));
        assert_eq!(window_around(&items, RootId(25))[0], RootId(15));
        assert_eq!(window_around(&items, RootId(49))[0], RootId(30));
        assert_eq!(window_around(&items, RootId(49)).len(), WINDOW);
        let few = (0..3).map(RootId).collect::<Vec<_>>();
        assert_eq!(window_around(&few, RootId(2)).len(), 3);
    }
}
