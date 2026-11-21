//! The log window, which the Electron app has in `DiagnosticsPanel.tsx` and
//! `DiagnosticsPanel.module.css`. It is a panel along the bottom of the main
//! window, drawn over everything except the menus, the dialogs and the
//! notifications. View › Log Window (⌘L) opens and closes it.
//!
//! The panel lists the kept log lines. Its header has a button for each
//! level and for each source, which filter the lines, and a text filter.
//! While the list is scrolled to its end, it follows new lines. Three
//! buttons copy the visible lines, save a support report to a file the user
//! chooses, and clear the lines. To the right of the lines the panel shows
//! the application's workers, which it reads every 750 ms. Dragging the
//! panel's top edge changes its height.
//!
//! A panel that is closed keeps nothing. The next one opens with every
//! filter on and at its default height, as in the Electron app, which
//! unmounts its panel when it closes.
use super::report::{self, Worker};
use super::{Entry, Level, Source};
use crate::i18n::t;
use crate::session::Session;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::text_input::{Kind, TextInput};
use crate::ui::{self, tooltip};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::rc::Rc;
use std::time::Duration;

/// The share of the window's height that the panel has by default. This
/// constant and the next three hold the limits of the panel's height. By
/// default the panel is 44% of the window's height, at most 620 px and at
/// least 260 px, as `.panel` is in the Electron app's CSS
/// (`height: min(44vh, 620px)`, `min-height: 260px`). A resized panel is at
/// least 260 px high, and its top edge stays at least 80 px below the top of
/// the window.
const DEFAULT_SHARE: f32 = 0.44;
const DEFAULT_MAX: f32 = 620.;
const MIN_HEIGHT: f32 = 260.;
const TOP_GAP: f32 = 80.;
/// The number of pixels by which the up and down arrow keys move the panel's
/// top edge while the edge has the keyboard focus.
const KEY_STEP: f32 = 24.;
const HEADER: f32 = 40.;
const CONTROL: f32 = 26.;
const ROW: f32 = 19.;
const WORKERS_WIDTH: f32 = 290.;
/// The list follows new lines while it is scrolled to within this many
/// pixels of its end.
const FOLLOW_SLACK: f32 = 32.;
/// The time between two reads of the workers.
const WORKERS_EVERY: Duration = Duration::from_millis(750);
/// The time between two checks for new log lines.
const LINES_EVERY: Duration = Duration::from_millis(100);
/// The colour of the dot beside a worker that is working, as
/// `.workerDot[data-state='working']` has it in the Electron app's CSS.
const WORKING: u32 = 0x47c976;
/// The colour of a warning line and of the dot beside a worker that needs
/// attention. The Electron app's CSS gives `var(--color-warning, #e8a838)`
/// and defines no `--color-warning`, so the colour is always `#e8a838`.
const WARNING: u32 = 0xe8a838;

/// The event that the panel emits when its close button is clicked. The
/// shell closes the panel when it receives the event.
pub struct Closed;
impl EventEmitter<Closed> for Panel {}

/// One line of the list. The first row of an entry holds the entry's columns
/// and the first line of its message. Each further line of the message is a
/// row of its own, which is how `white-space: pre` lays the message out in
/// the Electron app.
#[derive(Clone)]
struct Row {
    text: SharedString,
    level: Level,
}

/// Returns whether `entry` passes the filters. Its level must be switched on
/// in `levels`. Its source must be `source`; every source passes when
/// `source` is `None`. Its target and message, joined by a space, must
/// contain `query`, compared without regard to case; an empty `query` passes
/// every entry.
pub fn passes(entry: &Entry, levels: &[bool; 5], source: Option<Source>, query: &str) -> bool {
    let level = Level::ALL.iter().position(|level| *level == entry.level);
    let query = query.trim().to_lowercase();
    level.is_some_and(|index| levels[index])
        && source.is_none_or(|source| entry.source == source)
        && (query.is_empty()
            || format!("{} {}", entry.target, entry.message)
                .to_lowercase()
                .contains(&query))
}

/// Returns the panel's height in a window that is `window_height` high. When
/// the user has resized the panel, this is the height `dragged`, kept within
/// the limits of [`clamp_height`]. Otherwise it is the default height: 44%
/// of the window's height, at most 620 px and at least 260 px.
pub fn height(dragged: Option<f32>, window_height: f32) -> f32 {
    match dragged {
        Some(height) => clamp_height(height, window_height),
        None => (window_height * DEFAULT_SHARE)
            .min(DEFAULT_MAX)
            .max(MIN_HEIGHT),
    }
}

/// Returns `height` limited to at most 80 px less than the window's height
/// and to at least 260 px, as the Electron app's `resizeTo` limits it.
pub fn clamp_height(height: f32, window_height: f32) -> f32 {
    height.min(window_height - TOP_GAP).max(MIN_HEIGHT)
}

/// Returns `word` with its first letter in upper case. The Electron app's
/// CSS does the same to the label of a filter button with
/// `text-transform: capitalize`.
fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

pub struct Panel {
    session: Entity<Session>,
    levels: [bool; 5],
    source: Option<Source>,
    search: Entity<TextInput>,
    workers: Vec<Worker>,
    /// The height that the user gave the panel by dragging its top edge or
    /// with the arrow keys, or `None` while the panel has its default height.
    dragged: Option<f32>,
    /// Set while the top edge is being dragged: the pointer's vertical
    /// position when the mouse button was pressed, and the panel's height at
    /// that moment.
    resize: Option<(f32, f32)>,
    edge_hovered: bool,
    edge: FocusHandle,
    focus: FocusHandle,
    list: UniformListScrollHandle,
    /// Whether the list was at its end when the lines last changed.
    follow: bool,
    rows: Rc<Vec<Row>>,
    /// The index of the row with the most characters. The list takes its
    /// width from that row.
    widest: Option<usize>,
    /// The entries that pass the filters. "Copy visible logs" copies these.
    visible: Vec<Entry>,
    revision: u64,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Panel {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        // i18n: the text field translates its placeholder when it draws it
        let search = cx.new(|cx| TextInput::new(Kind::Compact, cx).placeholder("Filter logs"));
        let subscriptions = vec![cx.observe(&search, |this, _, cx| {
            this.refilter(cx);
            cx.notify();
        })];
        let lines = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(LINES_EVERY).await;
                let Ok(()) = this.update(cx, |this, cx| {
                    if super::revision() != this.revision {
                        this.refilter(cx);
                        cx.notify();
                    }
                }) else {
                    return;
                };
            }
        });
        let workers = cx.spawn(async move |this, cx| {
            loop {
                let Ok(answer) = this.update(cx, |this, cx| {
                    this.session
                        .read(cx)
                        .dispatch("diagnostics.snapshot", serde_json::json!({}))
                }) else {
                    return;
                };
                // When a read fails, the workers shown stay as they are and
                // no error is shown. The Electron app's panel does the same.
                if let Ok(Ok(snapshot)) = answer.await {
                    let workers = Worker::list(&snapshot);
                    if this
                        .update(cx, |this, cx| {
                            if this.workers != workers {
                                this.workers = workers;
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                cx.background_executor().timer(WORKERS_EVERY).await;
            }
        });
        let mut panel = Self {
            session,
            levels: [true; 5],
            source: None,
            search,
            workers: Vec::new(),
            dragged: None,
            resize: None,
            edge_hovered: false,
            edge: cx.focus_handle(),
            focus: cx.focus_handle(),
            list: UniformListScrollHandle::new(),
            follow: true,
            rows: Rc::new(Vec::new()),
            widest: None,
            visible: Vec::new(),
            revision: 0,
            _subscriptions: subscriptions,
            _tasks: vec![lines, workers],
        };
        panel.refilter(cx);
        panel.list.scroll_to_bottom();
        panel
    }

    /// Builds the rows again from the kept log lines and the filters. When
    /// the list was scrolled to its end before, it is scrolled to the new
    /// end.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        self.revision = super::revision();
        let query = self.search.read(cx).value();
        let base = self.list.0.borrow().base_handle.clone();
        let before = self.rows.len();
        if before > 0 {
            let gap = f32::from(base.max_offset().y + base.offset().y);
            self.follow = gap < FOLLOW_SLACK;
        }
        self.visible = super::snapshot()
            .into_iter()
            .filter(|entry| passes(entry, &self.levels, self.source, &query))
            .collect();
        let mut rows = Vec::new();
        for entry in &self.visible {
            let line = report::display_line(entry, &report::local_time(&entry.timestamp));
            for text in line.split('\n') {
                rows.push(Row {
                    text: text.to_owned().into(),
                    level: entry.level,
                });
            }
        }
        self.widest = rows
            .iter()
            .enumerate()
            .max_by_key(|(_, row)| row.text.chars().count())
            .map(|(index, _)| index);
        let grew = rows.len() != before;
        self.rows = Rc::new(rows);
        if grew && self.follow {
            self.list.scroll_to_bottom();
        }
    }

    fn toggle_level(&mut self, index: usize, cx: &mut Context<Self>) {
        self.levels[index] = !self.levels[index];
        self.refilter(cx);
        cx.notify();
    }

    fn choose_source(&mut self, source: Option<Source>, cx: &mut Context<Self>) {
        self.source = source;
        self.refilter(cx);
        cx.notify();
    }

    /// Does "Copy visible logs": puts every entry that passes the filters on
    /// the clipboard, one entry on each line, with the entry's fields
    /// separated by tabs.
    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = self
            .visible
            .iter()
            .map(report::entry_line)
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Does "Save support report". The report holds every kept log line, not
    /// only the visible ones, and the workers, with personal data removed
    /// (`report::scrub`). It is written to the file that the user chooses in
    /// the save dialog, and nowhere when the user cancels the dialog.
    fn save_report(&mut self, cx: &mut Context<Self>) {
        let created = super::now();
        let entries = super::snapshot();
        let language = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned());
        let text = report::build(
            &entries,
            &self.workers,
            &runtime(),
            &language,
            &version(),
            &report::host(),
            &created,
        );
        let chosen = crate::prompt::save(
            &crate::prompt::home(),
            &report::file_name(&created),
            // i18n: a file extension
            "txt",
            cx,
        );
        cx.background_executor()
            .spawn(async move {
                let Ok(Some(path)) = chosen.await else {
                    return;
                };
                // The Electron app does not handle a failure to write the
                // report, and its log then shows the failure as an error of
                // the renderer. This adds the same line to the log.
                if let Err(error) = report::write(&path, &text) {
                    super::add(super::Line {
                        level: Level::Error,
                        source: Source::Renderer,
                        // i18n: a log target
                        target: "renderer.promise".to_owned(),
                        message: error.to_string(),
                        timestamp: super::now(),
                        duration_ms: None,
                    });
                }
            })
            .detach();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(Closed);
    }

    fn filter_button(
        &self,
        id: impl Into<ElementId>,
        label: String,
        active: bool,
        theme: theme::Theme,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label.clone())
            .aria_toggled(ui::a11y::toggled(active))
            .h(px(CONTROL))
            .px(px(7.))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .text(theme::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .line_height(px(1. * theme::TEXT_XS))
            .when(active, |button| {
                button.text_color(theme.text()).bg(theme.surface_active())
            })
            .when(!active, |button| {
                button
                    .text_color(theme.text_tertiary())
                    .hover(|style| style.text_color(theme.text()).bg(theme.surface_hover()))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
    }

    /// Returns one of the header's icon buttons, styled as `.iconButton` is
    /// in the Electron app's CSS. The button is 26 px square and its glyph
    /// has the tertiary text colour. While the pointer is over the button,
    /// the glyph has the primary text colour and the button has the hover
    /// background.
    fn icon_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        size: f32,
        tip: SharedString,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> ui::icon_button::IconButton {
        ui::icon_button(id, glyph)
            .look(ui::icon_button::Look::Close)
            .size(CONTROL)
            .radius(theme::RADIUS_MD)
            .glyph(size)
            .tip(tooltip::tip(tip))
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
    }
}

/// Returns the line height of text of the size `size` under Mantine's body
/// `line-height`, which the text in the list of workers inherits in the
/// Electron app.
fn body_line(size: f32) -> Pixels {
    px(size * theme::LINE_HEIGHT_BODY)
}

/// Returns the text that the report gives as the user agent: the app's name,
/// the app's version (the version in the repository's `package.json`) and
/// the system the app runs on.
fn runtime() -> String {
    format!(
        "{}/{} ({}; {})",
        crate::desktop::APP_NAME,
        crate::updates::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Returns Picto's version, which is the version in the repository's
/// `package.json`. The Electron app's `app.getVersion()` returns the same.
fn version() -> String {
    serde_json::from_str::<serde_json::Value>(include_str!("../../../../package.json"))
        .ok()
        .and_then(|package| package.get("version")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let window_height = f32::from(window.viewport_size().height);
        let height = height(self.dragged, window_height);

        // The top edge is a band 9 px high that lies over the panel's top
        // border. A bar 38 px wide and 2 px high is drawn in its middle.
        let edge_lit = self.edge_hovered || self.resize.is_some() || self.edge.is_focused(window);
        let edge = div()
            .id("diagnostics-resize")
            .absolute()
            .top(px(-4.))
            .left_0()
            .right_0()
            .h(px(9.))
            .cursor(CursorStyle::ResizeUpDown)
            .track_focus(&self.edge)
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.edge_hovered = *hovered;
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.resize = Some((f32::from(event.position.y), height));
                    window.focus(&this.edge, cx);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let step = match event.keystroke.key.as_str() {
                    "up" => KEY_STEP,
                    "down" => -KEY_STEP,
                    _ => return,
                };
                let window_height = f32::from(window.viewport_size().height);
                this.dragged = Some(clamp_height(height + step, window_height));
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(relative(0.5))
                    .ml(px(-19.))
                    .w(px(38.))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(theme.text_tertiary())
                    .opacity(if edge_lit { 0.7 } else { 0.35 }),
            );
        // While the edge is dragged, the pointer is followed wherever it
        // goes in the window.
        let dragging = self.resize.map(|_| {
            let view = cx.entity().downgrade();
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moved = view.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let window_height = f32::from(window.viewport_size().height);
                        let _ = moved.update(cx, |this, cx| {
                            if event.pressed_button != Some(MouseButton::Left) {
                                this.resize = None;
                            } else if let Some((start_y, start_height)) = this.resize {
                                this.dragged = Some(clamp_height(
                                    start_height + start_y - f32::from(event.position.y),
                                    window_height,
                                ));
                            }
                            cx.notify();
                        });
                    });
                    let released = view.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = released.update(cx, |this, cx| {
                                this.resize = None;
                                cx.notify();
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_0()
        });

        let mut levels = div().flex().gap(px(2.));
        for (index, level) in Level::ALL.into_iter().enumerate() {
            levels = levels.child(self.filter_button(
                ("diagnostics-level", index),
                level.name().to_owned(),
                self.levels[index],
                theme,
                move |this, cx| this.toggle_level(index, cx),
                cx,
            ));
        }
        let mut sources = div().flex().gap(px(2.));
        let choices = std::iter::once(None).chain(Source::ALL.into_iter().map(Some));
        for (index, source) in choices.enumerate() {
            // i18n: the Electron app shows this value without translating it
            let name = source.map_or("all", Source::name);
            sources = sources.child(self.filter_button(
                ("diagnostics-source", index),
                capitalized(name),
                self.source == source,
                theme,
                move |this, cx| this.choose_source(source, cx),
                cx,
            ));
        }
        let search_focused = self.search.read(cx).focus_handle(cx).is_focused(window);
        let search = div()
            .w(px(170.))
            .h(px(CONTROL))
            .ml_auto()
            .px(px(8.))
            .flex_none()
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(if search_focused {
                theme.border()
            } else {
                theme.border_secondary()
            })
            .bg(rgba(theme::BLACK_20))
            .child(self.search.clone());
        let header = div()
            .h(px(HEADER))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .pl(px(12.))
            .pr(px(10.))
            .border_b_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .mr(px(6.))
                    .text(theme::TEXT_MD)
                    .font_weight(FontWeight::BOLD)
                    .child(t("Logs")),
            )
            .child(levels)
            .child(sources)
            .child(search)
            .child(self.icon_button(
                "diagnostics-copy",
                "action-copy",
                14.,
                t("Copy visible logs").into(),
                |this, cx| this.copy(cx),
                cx,
            ))
            .child(self.icon_button(
                "diagnostics-save",
                "diagnostics-download",
                14.,
                t("Save support report").into(),
                |this, cx| this.save_report(cx),
                cx,
            ))
            .child(self.icon_button(
                "diagnostics-clear",
                "diagnostics-trash",
                14.,
                t("Clear logs").into(),
                |_, _| super::clear(),
                cx,
            ))
            .child(self.icon_button(
                "diagnostics-close",
                "action-x",
                15.,
                t("Close diagnostics").into(),
                |this, cx| this.close(cx),
                cx,
            ));

        let rows = self.rows.clone();
        let (error, warning, text, hover) = (
            theme.negative(),
            rgb(WARNING),
            theme.text(),
            theme.surface_hover(),
        );
        let list = uniform_list("diagnostics-lines", rows.len(), move |range, _, _| {
            range
                .map(|index| {
                    let row = &rows[index];
                    div()
                        .id(index)
                        .h(px(ROW))
                        .min_w_full()
                        .px(px(12.))
                        .whitespace_nowrap()
                        .text_color(match row.level {
                            Level::Error => error.into(),
                            Level::Warn => warning,
                            _ => text,
                        })
                        .hover(|style| style.bg(hover))
                        .child(row.text.clone())
                })
                .collect()
        })
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .with_width_from_item(self.widest)
        .track_scroll(&self.list)
        .size_full()
        .pt(px(4.))
        .pb(px(8.));
        let scroll = self.list.0.borrow().base_handle.clone();
        let lines = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .font_family(theme::FONT_MONO)
            .text(theme::TEXT_CAPTION)
            .line_height(px(ROW))
            // Room for the scrollbar is kept on the right whether or not the
            // scrollbar is shown, as `scrollbar-gutter: stable` keeps it.
            .pr(px(ui::scrollbar::GUTTER))
            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
            .child(list)
            .child(ui::scrollbar(
                "diagnostics-lines-scrollbar",
                &scroll,
                window,
                cx,
            ));

        let mut workers = div()
            .id("diagnostics-workers")
            .w(px(WORKERS_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(theme.border_secondary())
            .p(px(10.))
            // The right padding includes room for the scrollbar whether or
            // not the scrollbar is shown, as `scrollbar-gutter: stable` does.
            .pr(px(10. + ui::scrollbar::GUTTER))
            .child(
                div()
                    .px(px(4.))
                    .pb(px(8.))
                    .text_color(theme.text_secondary())
                    .text(theme::TEXT_CAPTION)
                    .line_height(body_line(theme::TEXT_CAPTION))
                    .font_weight(FontWeight::BOLD)
                    .child(t("Workers").to_uppercase()),
            );
        for (index, worker) in self.workers.iter().enumerate() {
            let (dot, strength) = match worker.state.as_str() {
                "working" => (rgb(WORKING), 1.),
                "attention" => (rgb(WARNING), 1.),
                _ => (theme.text_tertiary(), 0.55),
            };
            let count = if worker.active > 0 {
                Some(crate::tf!("{value0} running", worker.active))
            } else if worker.queued > 0 {
                Some(crate::tf!("{value0} queued", worker.queued))
            } else {
                None
            };
            workers = workers.child(
                div()
                    .id(("diagnostics-worker", index))
                    .min_h(px(48.))
                    .flex()
                    .items_start()
                    .gap(px(9.))
                    .p(px(8.))
                    .rounded(px(theme::RADIUS_MD))
                    .hover(|style| style.bg(theme.surface_hover()))
                    .child(
                        div()
                            .size(px(7.))
                            .mt(px(5.))
                            .flex_none()
                            .rounded_full()
                            .bg(dot)
                            .opacity(strength),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text(theme::TEXT_SM)
                                    .line_height(body_line(theme::TEXT_SM))
                                    .text_color(theme.text())
                                    .child(worker.label.clone()),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text(theme::TEXT_CAPTION)
                                    .line_height(body_line(theme::TEXT_CAPTION))
                                    .text_color(theme.text_tertiary())
                                    .truncate()
                                    .child(worker.detail.clone()),
                            ),
                    )
                    .children(count.map(|count| {
                        div()
                            .flex_none()
                            .font_family(theme::FONT_MONO)
                            .text(theme::TEXT_XS)
                            .line_height(px(16.))
                            .text_color(theme.text_tertiary())
                            .child(count)
                    })),
            );
        }

        let mut background = theme.background();
        background.a = 0.97;
        let panel = ui::GlassPanel::new(
            div()
                .id("diagnostics")
                .track_focus(&self.focus)
                // i18n: a key context
                .key_context("Diagnostics")
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(height))
                .flex()
                .flex_col()
                .occlude()
                .text_color(theme.text())
                .bg(background)
                .border_t_1()
                .border_color(theme.border()),
            0.,
        )
        .backdrop_blur(BackdropFilter {
            brightness: 0.8,
            ..BackdropFilter::blur(px(16.))
        })
        .drop_shadows(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.3),
            offset: point(px(0.), px(-12.)),
            blur_radius: px(32.),
            spread_radius: px(0.),
            inset: false,
        }])
        .child(edge)
        .child(header)
        .child(div().flex_1().min_h_0().flex().child(lines).child(workers))
        .children(dragging);
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Level, Source, capitalized, height, passes};

    fn entry(level: Level, source: Source, target: &str, message: &str) -> Entry {
        Entry {
            id: 1,
            level,
            source,
            target: target.into(),
            message: message.into(),
            timestamp: "2026-08-26T12:00:00.000Z".into(),
            duration_ms: None,
            native_duration_ms: None,
        }
    }

    #[test]
    fn filters_by_level_source_and_text() {
        let all = [true; 5];
        let warn = entry(
            Level::Warn,
            Source::Core,
            "picto_core::media",
            "Thumbnail Failed",
        );
        assert!(passes(&warn, &all, None, ""));
        assert!(passes(&warn, &all, Some(Source::Core), " thumbnail "));
        assert!(passes(&warn, &all, None, "PICTO_CORE"));
        assert!(!passes(&warn, &all, Some(Source::Ipc), ""));
        assert!(!passes(&warn, &all, None, "missing"));
        let mut no_warnings = all;
        no_warnings[1] = false;
        assert!(!passes(&warn, &no_warnings, None, ""));
    }

    #[test]
    fn height_follows_the_window_within_its_limits() {
        // A panel that was not resized is 44% of the window's height, at
        // most 620 and at least 260.
        assert_eq!(height(None, 865.), 865. * 0.44);
        assert_eq!(height(None, 2000.), 620.);
        assert_eq!(height(None, 500.), 260.);
        // A resized panel is at least 260 high and at most 80 less than the
        // window's height.
        assert_eq!(height(Some(900.), 865.), 785.);
        assert_eq!(height(Some(100.), 865.), 260.);
        assert_eq!(capitalized("renderer"), "Renderer");
    }
}
