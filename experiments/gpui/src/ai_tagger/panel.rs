//! The review panel (`AiTaggerPanel.tsx` in its `OverlayShell`): the items
//! to tag, the models to run, each item's suggested tags with their
//! confidences, and Apply, which writes the checked tags as one undoable
//! edit.
use super::review::{self, Overrides, WINDOW};
use crate::grid::thumbnails::Thumbnails;
use crate::motion;
use crate::session::{Change, Report, Reread, Session, edit_then_in};
use crate::ui::{
    self,
    overlay_shell::{self, Geometry, Nav},
    text_input::{Kind, TextInput},
};
use gpui::*;
use picto_core::ai_runtime::{AiModelStatus, LibraryManualPredictionResponse, RootPrediction};
use picto_library::selection::SelectionTarget;
use picto_library::{LabColor, MediaId, RootDetails, RootId};
use serde_json::json;
use std::collections::HashMap;

/// 780 × 540 with the rail, 590 × 540 without.
pub(super) const WIDE: f32 = 780.;
pub(super) const NARROW: f32 = 590.;
pub(super) const HEIGHT: f32 = 540.;

/// Which of an item's tags the list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ViewMode {
    Suggested,
    Below,
}

/// An item as the review shows it (`ReviewRoot`): its image members, and
/// the one its preview shows.
#[derive(Clone, Debug)]
pub(super) struct ReviewRoot {
    pub label: String,
    pub images: Vec<MediaId>,
    pub preview_hash: String,
    pub preview_size: Option<(u32, u32)>,
    pub preview_color: Option<LabColor>,
}

/// `reviewRoot`: an item with at least one image, previewed by its cover
/// if that is an image, else by its first image.
fn review_root(details: &RootDetails) -> Option<ReviewRoot> {
    let images = details
        .media
        .iter()
        .filter(|media| media.facts.mime.starts_with("image/"))
        .collect::<Vec<_>>();
    let preview = images
        .iter()
        .find(|media| media.media_id == details.root.cover_media_id)
        .or(images.first())?;
    let label = [&details.root.name, &preview.media_name]
        .into_iter()
        .find(|name| !name.is_empty())
        .cloned()
        // i18n: production writes it in English
        .unwrap_or_else(|| format!("Item {}", details.root.root_id.0));
    Some(ReviewRoot {
        label,
        images: images.iter().map(|media| media.media_id).collect(),
        preview_hash: preview.facts.content_hash.clone(),
        preview_size: preview.facts.width.zip(preview.facts.height),
        preview_color: preview.facts.palette.first().cloned(),
    })
}

/// How far a run has gone: models × items done, and the item analysed now.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Progress {
    pub done: usize,
    pub total: usize,
    pub current: Option<RootId>,
}

pub struct AiTagger {
    pub(super) session: Entity<Session>,
    pub(super) thumbnails: Entity<Thumbnails>,
    pub(super) search: Entity<TextInput>,
    pub(super) focus: FocusHandle,
    opening: Option<ui::popover::Opening>,
    pub(super) geometry: Geometry,
    pub(super) pinned: bool,
    pub(super) sidebar: bool,
    /// The panel's width (200ms) and the rail's fade.
    pub(super) width_fade: motion::Fade,
    pub(super) rail_shown: motion::Fade,
    /// What the review is of; `None` follows the grid's selection, as the
    /// panel opened by the grid, its menu or the inspector does.
    explicit: Option<SelectionTarget>,
    target: Option<SelectionTarget>,
    /// Bumped by every opening, run and close, so an answer for an earlier
    /// one is dropped.
    generation: u64,
    pub(super) models: Vec<AiModelStatus>,
    /// The models chosen, in the order they run.
    pub(super) run_models: Vec<String>,
    pub(super) predictions: Vec<RootPrediction>,
    /// The items with images, in the destination's order.
    pub(super) items: Vec<RootId>,
    pub(super) resolved: bool,
    /// Items read so far, and those the thumbnail rail shows.
    roots: HashMap<RootId, ReviewRoot>,
    pub(super) rail: Vec<RootId>,
    pub(super) active: Option<RootId>,
    pub(super) progress: Progress,
    pub(super) backend: Option<String>,
    pub(super) running: bool,
    pub(super) applying: bool,
    pub(super) error: Option<String>,
    /// The cutoff in whole percent.
    pub(super) confidence: u32,
    pub(super) view: ViewMode,
    pub(super) overrides: Overrides,
    pub(super) query: String,
    pub(super) list_scroll: ScrollHandle,
    pub(super) rail_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl AiTagger {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| TextInput::new(Kind::Menu, cx).placeholder("Filter suggestions..."));
        let subscriptions = vec![
            cx.observe(&search, |this, search, cx| {
                let query = search.read(cx).value();
                if query != this.query {
                    this.query = query;
                    this.list_scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }
            }),
            // Opened on the selection, the review follows it.
            cx.subscribe(&session, |this, _, change: &Change, cx| {
                if *change == Change::Selection {
                    this.selection_changed(cx);
                }
            }),
        ];
        Self {
            session,
            thumbnails,
            search,
            focus: cx.focus_handle(),
            opening: None,
            geometry: Geometry {
                right: None,
                left: 0.,
                top: 0.,
            },
            pinned: false,
            sidebar: true,
            width_fade: motion::Fade::at(true),
            rail_shown: motion::Fade::at(true),
            explicit: None,
            target: None,
            generation: 0,
            models: Vec::new(),
            run_models: Vec::new(),
            predictions: Vec::new(),
            items: Vec::new(),
            resolved: false,
            roots: HashMap::new(),
            rail: Vec::new(),
            active: None,
            progress: Progress::default(),
            backend: None,
            running: false,
            applying: false,
            error: None,
            confidence: review::DEFAULT_CONFIDENCE,
            view: ViewMode::Suggested,
            overrides: Overrides::default(),
            query: String::new(),
            list_scroll: ScrollHandle::new(),
            rail_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.opening.is_some()
    }

    pub(super) fn width(&self) -> f32 {
        NARROW + (WIDE - NARROW) * self.width_fade.value_with(motion::EASE_DEFAULT)
    }

    /// Opens beside the inspector at `anchor` (or centred) on `target`, or
    /// on the grid's selection, which it then follows.
    pub fn open(
        &mut self,
        anchor: Option<Point<f32>>,
        target: Option<SelectionTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.opening.is_none() {
            self.opening = Some(ui::popover::Opening::new(window, cx));
        }
        let view = window.viewport_size();
        let width = if self.sidebar { WIDE } else { NARROW };
        self.geometry =
            overlay_shell::place(overlay_shell::Opens::beside(anchor), view, width, HEIGHT);
        self.explicit = target;
        self.target = self.current_target(cx);
        let handle = self.search.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        self.start(cx);
    }

    fn current_target(&self, cx: &App) -> Option<SelectionTarget> {
        self.explicit
            .clone()
            .or_else(|| self.session.read(cx).selection_target())
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(opening) = self.opening.take() else {
            return;
        };
        self.end(cx);
        opening.finish(&self.focus, window, cx);
        cx.notify();
    }

    /// Stops what the review was doing and lets the models go
    /// (`aiTaggerUnload`), as production does whenever the panel closes or
    /// its items change.
    fn end(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.running = false;
        let _ = self
            .session
            .read(cx)
            .dispatch("ai.review.unload", json!({}));
    }

    /// The grid's selection changed: a review that follows it starts again
    /// on the new items.
    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        if !self.is_open() || self.explicit.is_some() {
            return;
        }
        let target = self.current_target(cx);
        if target != self.target {
            self.target = target;
            self.end(cx);
            self.start(cx);
        }
    }

    /// Starts a review afresh: the models and thresholds, the items with
    /// images, and the models last chosen.
    fn start(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.running = false;
        self.query.clear();
        self.search.update(cx, |search, cx| search.clear(cx));
        self.predictions.clear();
        self.rail.clear();
        self.active = None;
        self.overrides.clear();
        self.view = ViewMode::Suggested;
        self.error = None;
        self.backend = None;
        self.progress = Progress::default();
        self.items.clear();
        self.resolved = false;
        self.roots.clear();
        self.confidence = review::DEFAULT_CONFIDENCE;
        self.list_scroll.set_offset(point(px(0.), px(0.)));

        let status = super::status(self.session.read(cx));
        let target = self.target.clone();
        let items = self.session.read(cx).run(move |library| match target {
            Some(target) => library
                .ordered_image_selection(&target)
                .map_err(|error| error.to_string()),
            None => Ok(Vec::new()),
        });
        let remembered = crate::settings::current(cx)
            .ai_tagger_manual_model_slugs
            .clone();
        cx.spawn(async move |this, cx| {
            let status = status.await;
            let items = items.await.unwrap_or_else(|_| Err("cancelled".into()));
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match (status, items) {
                    (Ok(status), Ok(items)) => {
                        this.items = items;
                        this.resolved = true;
                        this.confidence = review::initial_confidence(status.thresholds.general);
                        this.backend = status.cached_backend.clone();
                        this.active = this.items.first().copied();
                        this.run_models = review::initial_models(
                            &status.models,
                            &status.configured_model_slugs,
                            remembered.as_deref(),
                        );
                        this.models = status.models;
                        this.load_rail(cx);
                    }
                    (Err(error), _) | (_, Err(error)) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ── Items ───────────────────────────────────────────────────

    /// Reads the items `ids` not read yet; those without an image are left
    /// out of the answer.
    fn read_roots(
        &self,
        ids: Vec<RootId>,
        cx: &App,
    ) -> futures::channel::oneshot::Receiver<Vec<(RootId, Option<ReviewRoot>)>> {
        let known = ids
            .iter()
            .map(|id| (*id, self.roots.get(id).cloned()))
            .collect::<HashMap<_, _>>();
        self.session.read(cx).run(move |library| {
            ids.into_iter()
                .map(|id| match known.get(&id) {
                    Some(Some(root)) => (id, Some(root.clone())),
                    _ => (id, library.details(id).ok().as_ref().and_then(review_root)),
                })
                .collect()
        })
    }

    /// Keeps what was read, at most a few windows of it.
    fn keep_roots(&mut self, roots: &[(RootId, Option<ReviewRoot>)]) {
        if self.roots.len() > 4 * WINDOW {
            let rail = self.rail.clone();
            self.roots.retain(|id, _| rail.contains(id));
        }
        for (id, root) in roots {
            if let Some(root) = root {
                self.roots.insert(*id, root.clone());
            }
        }
    }

    /// The thumbnails around the item reviewed, read if they are not yet.
    fn load_rail(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.active else {
            self.rail.clear();
            return;
        };
        let window = review::window_around(&self.items, active).to_vec();
        let answer = self.read_roots(window, cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let Ok(roots) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation && !this.running {
                    return;
                }
                this.keep_roots(&roots);
                this.rail = roots
                    .iter()
                    .filter(|(_, root)| root.is_some())
                    .map(|(id, _)| *id)
                    .collect();
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn root(&self, id: RootId) -> Option<&ReviewRoot> {
        self.roots.get(&id)
    }

    /// Shows the item `delta` places on (`view.prevImage`, `view.nextImage`
    /// and the arrows beside the counter).
    pub(super) fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        let current = self
            .active
            .and_then(|id| self.items.iter().position(|item| *item == id))
            .unwrap_or(0);
        let next = current
            .saturating_add_signed(delta)
            .min(self.items.len() - 1);
        self.show(self.items[next], cx);
    }

    pub(super) fn show(&mut self, id: RootId, cx: &mut Context<Self>) {
        if self.active != Some(id) {
            self.active = Some(id);
            self.list_scroll.set_offset(point(px(0.), px(0.)));
            self.load_rail(cx);
        }
        cx.notify();
    }

    pub(super) fn active_index(&self) -> usize {
        self.active
            .and_then(|id| self.items.iter().position(|item| *item == id))
            .unwrap_or(0)
    }

    // ── Models and running ──────────────────────────────────────

    /// A model chosen or left out, remembered for the next review
    /// (`aiTaggerManualModelSlugs`).
    pub(super) fn toggle_model(&mut self, slug: &str, cx: &mut Context<Self>) {
        review::toggle_model(&mut self.run_models, slug);
        let chosen = self.run_models.clone();
        crate::settings::model(cx).update(cx, |model, cx| {
            model.apply_now(
                move |settings| settings.ai_tagger_manual_model_slugs = Some(chosen.clone()),
                cx,
            )
        });
        cx.notify();
    }

    /// `runPredict`: each chosen model in turn over every item, one image
    /// at a time, so the list fills in as answers arrive.
    pub(super) fn run(&mut self, cx: &mut Context<Self>) {
        if self.running || self.run_models.is_empty() || self.items.is_empty() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.running = true;
        self.error = None;
        self.predictions.clear();
        let slugs = self.run_models.clone();
        let items = self.items.clone();
        let total = slugs.len() * items.len();
        self.progress = Progress {
            done: 0,
            total,
            current: items.first().copied(),
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let current = |this: &AiTagger| this.generation == generation;
            let mut targets: HashMap<RootId, Vec<MediaId>> = HashMap::new();
            let (mut unprepared, mut failed) = (0, 0);
            let mut first_failure: Option<String> = None;
            let mut done = 0;
            let mut stopped: Option<String> = None;
            'models: for (index, slug) in slugs.iter().enumerate() {
                for window in items.chunks(WINDOW) {
                    if index == 0 {
                        let Ok(answer) =
                            this.update(cx, |this, cx| this.read_roots(window.to_vec(), cx))
                        else {
                            return;
                        };
                        let Ok(roots) = answer.await else {
                            return;
                        };
                        let Ok(still) = this.update(cx, |this, _| {
                            this.keep_roots(&roots);
                            current(this)
                        }) else {
                            return;
                        };
                        if !still {
                            return;
                        }
                        for (id, root) in roots {
                            match root {
                                Some(root) => targets.insert(id, root.images),
                                None => {
                                    unprepared += 1;
                                    targets.insert(id, Vec::new())
                                }
                            };
                        }
                    }
                    for root in window {
                        let Ok(still) = this.update(cx, |this, cx| {
                            this.progress = Progress {
                                done,
                                total,
                                current: Some(*root),
                            };
                            cx.notify();
                            current(this)
                        }) else {
                            return;
                        };
                        if !still {
                            return;
                        }
                        for media in targets.get(root).cloned().unwrap_or_default() {
                            let request = json!({
                                "targets": [{ "rootId": root.0, "mediaItemId": media.0 }],
                                "modelSlugs": [slug],
                            });
                            let Ok(answer) = this.update(cx, |this, cx| {
                                this.session.read(cx).dispatch("ai.review.predict", request)
                            }) else {
                                return;
                            };
                            let output = match answer.await {
                                Ok(Ok(value)) => {
                                    serde_json::from_value::<LibraryManualPredictionResponse>(value)
                                        .map_err(|error| error.to_string())
                                }
                                Ok(Err(error)) => Err(error),
                                Err(_) => return,
                            };
                            let output = match output {
                                Ok(output) => output,
                                Err(error) => {
                                    stopped = Some(error);
                                    break 'models;
                                }
                            };
                            if let Some(error) =
                                output.predictions.iter().find_map(|p| p.error.clone())
                            {
                                failed += 1;
                                first_failure.get_or_insert(error);
                            }
                            let Ok(still) = this.update(cx, |this, cx| {
                                if current(this) {
                                    review::merge(&mut this.predictions, output.predictions);
                                    cx.notify();
                                }
                                current(this)
                            }) else {
                                return;
                            };
                            if !still {
                                return;
                            }
                        }
                        done += 1;
                        let _ = this.update(cx, |this, cx| {
                            this.active.get_or_insert(*root);
                            this.progress = Progress {
                                done,
                                total,
                                current: None,
                            };
                            cx.notify();
                        });
                    }
                }
            }
            let error = stopped
                .or_else(|| review::failure(unprepared, failed, total, first_failure.as_deref()));
            // The backend the models ran on, for the footer.
            let Ok(status) = this.update(cx, |this, cx| super::status(this.session.read(cx)))
            else {
                return;
            };
            let backend = status.await.ok().map(|status| status.cached_backend);
            let _ = this.update(cx, |this, cx| {
                if !current(this) {
                    return;
                }
                this.error = error;
                this.backend = Some(
                    backend
                        .flatten()
                        .or(this.backend.take())
                        // i18n: production's default, not translated
                        .unwrap_or_else(|| "CPU".into()),
                );
                this.running = false;
                this.progress.current = None;
                cx.notify();
            });
        })
        .detach();
    }

    // ── Choosing and applying ───────────────────────────────────

    pub(super) fn cutoff(&self) -> f32 {
        self.confidence as f32 / 100.
    }

    pub(super) fn set_confidence(&mut self, value: f64, cx: &mut Context<Self>) {
        let value = (value.round() as u32).clamp(review::MIN_CONFIDENCE, review::MAX_CONFIDENCE);
        if value != self.confidence {
            self.confidence = value;
            cx.notify();
        }
    }

    pub(super) fn toggle_tag(
        &mut self,
        item: RootId,
        tag: &review::ReviewTag,
        cx: &mut Context<Self>,
    ) {
        let checked = self.overrides.is_checked(item, tag, self.cutoff());
        self.overrides.set(item, &tag.key, !checked);
        cx.notify();
    }

    /// Writes the checked tags of every item as one undoable edit, then
    /// closes unless pinned (`applyChecked`).
    pub(super) fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let assignments = review::assignments(
            &self.predictions,
            &self.run_models,
            &self.overrides,
            self.cutoff(),
        );
        if assignments.is_empty() || self.applying {
            return;
        }
        self.applying = true;
        self.error = None;
        cx.notify();
        edit_then_in(
            &self.session,
            Report {
                announce: Some("items.apply_ai_tags"),
                ..Report::default()
            },
            Reread::SELECTION,
            move |library| library.add_tag_assignments(&assignments),
            window,
            cx,
            move |this, result, window, cx| {
                this.applying = false;
                match result {
                    Ok(_) => {
                        this.overrides.clear();
                        if !this.pinned {
                            this.close(window, cx);
                        }
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            },
        );
    }

    /// Answers the arrows, Enter and Tab from anywhere in the panel: the
    /// side arrows step through the items while the filter is empty.
    pub(super) fn navigate(&mut self, key: Nav, cx: &mut Context<Self>) -> bool {
        match key {
            Nav::Left | Nav::Right if self.query.is_empty() => {
                self.move_active(if key == Nav::Left { -1 } else { 1 }, cx);
                true
            }
            _ => false,
        }
    }
}
