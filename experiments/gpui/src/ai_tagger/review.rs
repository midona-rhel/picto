//! The review's rules (`AiTaggerPanel.tsx`), apart from drawing: how one
//! item's predictions become rows, which are checked, what is written, the
//! models a review starts with, and how failures are worded.
use crate::i18n::t;
use picto_core::ai_runtime::{AiModelStatus, AiTagPrediction, RootPrediction};
use picto_library::{RootId, RootTagAssignment};
use std::collections::HashMap;

/// `MIN_REVIEW_CONFIDENCE`, `MAX_REVIEW_CONFIDENCE`,
/// `DEFAULT_REVIEW_CONFIDENCE`, in percent.
pub const MIN_CONFIDENCE: u32 = 5;
pub const MAX_CONFIDENCE: u32 = 95;
pub const DEFAULT_CONFIDENCE: u32 = 35;
/// `REVIEW_WINDOW_SIZE`: how many items' details are read at once, and how
/// many thumbnails the rail shows around the one reviewed.
pub const WINDOW: usize = 20;

/// One suggested tag of an item: its predictions by every model run,
/// merged (`ReviewTag`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewTag {
    /// What is written: the tag, with its group unless that is general.
    pub key: String,
    pub namespace: String,
    pub subtag: String,
    /// The highest confidence any model gave it.
    pub confidence: f32,
    /// Each model that predicted it, with its own confidence.
    pub models: Vec<(String, f32)>,
}

impl ReviewTag {
    /// Whether its group is written before it (`general` and `default` are
    /// not).
    pub fn shows_namespace(&self) -> bool {
        shows_namespace(&self.namespace)
    }
}

fn shows_namespace(namespace: &str) -> bool {
    !namespace.is_empty() && namespace != "general" && namespace != "default"
}

/// The name a prediction is written under.
pub fn tag_key(namespace: &str, tag: &str) -> String {
    if shows_namespace(namespace) {
        format!("{namespace}:{tag}")
    } else {
        tag.to_owned()
    }
}

/// `predictionTags`: an item's predictions by the models chosen, one row
/// per tag, most confident first (ties keep the order they came in).
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

/// `mergePredictionResults`: answers arriving model by model and member by
/// member are joined per item, each model's tag kept at its highest
/// confidence, and an item's first error kept.
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

/// A tag's check for one item, changed by a click; otherwise it is checked
/// when its confidence reaches the cutoff.
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

/// What Apply writes: each item's checked tags, items with none left out.
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

/// The cutoff a review starts at: the General confidence in Settings, in
/// whole percent within the slider's range.
pub fn initial_confidence(general: f32) -> u32 {
    ((general * 100.).round() as i64).clamp(MIN_CONFIDENCE as i64, MAX_CONFIDENCE as i64) as u32
}

/// The models a review starts with: those last chosen for a review
/// (`aiTaggerManualModelSlugs`), else those set to tag imports, that are
/// downloaded; with neither ever chosen, the recommended downloaded model,
/// else the first downloaded.
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

/// A model chosen or left out of the review; a model chosen runs after
/// those chosen before it.
pub fn toggle_model(models: &mut Vec<String>, slug: &str) {
    match models.iter().position(|chosen| chosen == slug) {
        Some(at) => {
            models.remove(at);
        }
        None => models.push(slug.to_owned()),
    }
}

/// How many distinct tags each model predicted across the items.
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

/// What a finished run reports: items that could not be read come first;
/// otherwise the model and member analyses that failed, with the first
/// failure's own words.
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

/// Apply's label: the count and production's English noun.
pub fn apply_label(checked: usize) -> String {
    crate::tf!(
        "Apply {value0} {value1}",
        checked,
        if checked == 1 { "tag" } else { "tags" }
    )
}

/// What the list says when it shows no tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Empty {
    /// A failure, with no tags to show it above.
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

/// The facts the empty list depends on.
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
    /// Which message an empty list shows, in production's order.
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

    /// Its words; the error shows its own.
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

/// The review window around the item reviewed: up to [`WINDOW`] items,
/// that item in the middle where the ends allow.
pub fn window_around(items: &[RootId], active: RootId) -> &[RootId] {
    let index = items.iter().position(|id| *id == active).unwrap_or(0);
    let last_start = items.len().saturating_sub(WINDOW);
    let start = index.saturating_sub(WINDOW / 2).min(last_start);
    &items[start..(start + WINDOW).min(items.len())]
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
        // Model c was not run; general tags are written without their group.
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
        // A higher cutoff moves what is not overridden.
        let written = assignments(&predictions, &models, &overrides, 0.5);
        assert_eq!(written[0].tags, ["dog"]);
        // No model chosen: nothing.
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
        // Remembered from the last review, in that order, if downloaded.
        assert_eq!(
            initial_models(&models, &[], Some(&slugs(&["missing", "light", "heavy"]))),
            ["light", "heavy"]
        );
        // Chosen to be left empty last time: stays empty.
        assert!(initial_models(&models, &slugs(&["heavy"]), Some(&[])).is_empty());
        // Never chosen: those that tag imports.
        assert_eq!(initial_models(&models, &slugs(&["heavy"]), None), ["heavy"]);
        // Neither: the recommended downloaded model, else the first.
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
    fn failures_are_worded_as_production_words_them() {
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
    fn an_empty_list_says_why_in_production_order() {
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
