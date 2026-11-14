//! The log window (`DiagnosticsPanel.tsx`, `DiagnosticsPanel.module.css`):
//! a panel along the bottom of the main window, over everything but the
//! menus, dialogs and notifications, that View › Log Window (⌘L) opens and
//! closes. It lists the kept log lines with their level and source filters
//! and a text filter, follows new lines while scrolled to the end, copies
//! the visible lines, saves a support report where the user chooses, clears
//! the lines, and shows the application's workers, read every 750 ms. Its
//! top edge resizes it. Closed, it keeps nothing: it opens with every
//! filter on and at its default height, as production's unmounted panel.
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

/// `.panel`: `height: min(44vh, 620px)`, `min-height: 260px`; a drag keeps
/// it 80 px short of the window's top.
const DEFAULT_SHARE: f32 = 0.44;
const DEFAULT_MAX: f32 = 620.;
const MIN_HEIGHT: f32 = 260.;
const TOP_GAP: f32 = 80.;
/// The arrow keys on the resize edge move it by this much.
const KEY_STEP: f32 = 24.;
const HEADER: f32 = 40.;
const CONTROL: f32 = 26.;
const ROW: f32 = 19.;
const WORKERS_WIDTH: f32 = 290.;
/// Within this much of the end, the list follows new lines.
const FOLLOW_SLACK: f32 = 32.;
/// How often the workers are read again.
const WORKERS_EVERY: Duration = Duration::from_millis(750);
/// How often the panel looks for new lines.
const LINES_EVERY: Duration = Duration::from_millis(100);
/// `.workerDot[data-state='working']`.
const WORKING: u32 = 0x47c976;
/// `var(--color-warning, #e8a838)`: production defines no `--color-warning`.
const WARNING: u32 = 0xe8a838;

/// The panel asks to be closed (its close button, Escape).
pub struct Closed;
impl EventEmitter<Closed> for Panel {}

/// One drawn line: an entry's first line carries its columns; a message
/// over several lines continues on lines of its own, as `white-space: pre`
/// lays it out.
#[derive(Clone)]
struct Row {
    text: SharedString,
    level: Level,
}

/// Whether `entry` passes the filters: a level shown, the source chosen
/// (all when `None`), and the text filter found in its target and message,
/// ignoring case.
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

/// The panel's height: the dragged one within its limits, else the default
/// share of the window.
pub fn height(dragged: Option<f32>, window_height: f32) -> f32 {
    match dragged {
        Some(height) => clamp_height(height, window_height),
        None => (window_height * DEFAULT_SHARE)
            .min(DEFAULT_MAX)
            .max(MIN_HEIGHT),
    }
}

/// `resizeTo`.
pub fn clamp_height(height: f32, window_height: f32) -> f32 {
    height.min(window_height - TOP_GAP).max(MIN_HEIGHT)
}

/// `CSS text-transform: capitalize` on a filter's label.
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
    /// The height dragged to, if it was.
    dragged: Option<f32>,
    /// The press on the top edge: where it was and the height then.
    resize: Option<(f32, f32)>,
    edge_hovered: bool,
    edge: FocusHandle,
    focus: FocusHandle,
    list: UniformListScrollHandle,
    /// Whether the list was at its end when the lines last changed.
    follow: bool,
    rows: Rc<Vec<Row>>,
    /// The longest row, which sets the list's width.
    widest: Option<usize>,
    /// The visible entries, which Copy copies.
    visible: Vec<Entry>,
    revision: u64,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Panel {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        // i18n: a placeholder; the field shows it in the language
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
                // A failed read is left to the log lines, as in production.
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

    /// The lines again, from the store and the filters. The list follows
    /// them when it was at its end.
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

    /// Copy visible logs: every field of each, tab-delimited.
    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = self
            .visible
            .iter()
            .map(report::entry_line)
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Save support report: every kept line (not only the visible ones) and
    /// the workers, scrubbed, written only where the user chooses.
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
                // An unhandled failure in production reaches the log as a
                // renderer error; here it is logged as one.
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

    fn icon_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        size: f32,
        tip: SharedString,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let group: SharedString = format!("diagnostics-{id}").into();
        let button = div()
            .id(id)
            .group(group.clone())
            .size(px(CONTROL))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_MD))
            .hover(|style| style.bg(theme.surface_hover()))
            .child(
                ui::icon(glyph, size, theme.text_tertiary())
                    .group_hover(group, move |style| style.text_color(theme.text())),
            )
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)));
        tooltip::attach(
            button,
            SharedString::from(format!("{id}-tip")),
            tooltip::tip(tip),
            CONTROL,
            window,
            cx,
        )
    }
}

/// The line of text at `size` under Mantine's body `line-height`, which
/// the panel's workers inherit.
fn body_line(size: f32) -> Pixels {
    px(size * theme::LINE_HEIGHT_BODY)
}

/// What the report gives for the user agent: the app, its version and the
/// system it runs on.
fn runtime() -> String {
    format!(
        "Picto GPUI/{} ({}; {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Picto's version, `app.getVersion()`: the repository's `package.json`.
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

        // The top edge: a 9 px band over the panel's border, with a short
        // pill in its middle.
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
            // i18n: production's value, shown as it is
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
                window,
                cx,
            ))
            .child(self.icon_button(
                "diagnostics-save",
                "diagnostics-download",
                14.,
                t("Save support report").into(),
                |this, cx| this.save_report(cx),
                window,
                cx,
            ))
            .child(self.icon_button(
                "diagnostics-clear",
                "diagnostics-trash",
                14.,
                t("Clear logs").into(),
                |_, _| super::clear(),
                window,
                cx,
            ))
            .child(self.icon_button(
                "diagnostics-close",
                "action-x",
                15.,
                t("Close diagnostics").into(),
                |this, cx| this.close(cx),
                window,
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
            // The scrollbar's gutter stays reserved (`scrollbar-gutter:
            // stable`).
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
            // `scrollbar-gutter: stable` keeps the scrollbar's width.
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
        // `min(44vh, 620px)`, at least 260.
        assert_eq!(height(None, 865.), 865. * 0.44);
        assert_eq!(height(None, 2000.), 620.);
        assert_eq!(height(None, 500.), 260.);
        // Dragged: at least 260, and 80 short of the window's top.
        assert_eq!(height(Some(900.), 865.), 785.);
        assert_eq!(height(Some(100.), 865.), 260.);
        assert_eq!(capitalized("renderer"), "Renderer");
    }
}
