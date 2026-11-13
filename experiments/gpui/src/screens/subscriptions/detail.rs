//! One subscription's workspace (`SubscriptionDetail.tsx`): a fixed rail
//! with its cover, name, status, actions and properties, beside a data plane
//! of tabbed tables: its sources, its runs and its problems.
use super::data::{self, Detail};
use super::format::{self, Track};
use super::{HISTORY, SOURCES, SubscriptionsScreen, TABS, home, measure};
use crate::theme::Text as _;
use crate::ui::status_badge::Tone;
use crate::ui::{self, action_button::Variant};
use crate::{motion, theme};
use gpui::*;
use picto_library::query::ItemScope;

/// `.subscriptionRail`
const RAIL: f32 = 280.;
/// The rail's content: its width less the left padding and the gutter.
const RAIL_CONTENT: f32 = RAIL - 8. - ui::scrollbar::GUTTER;
/// A table row's and the header's height.
const ROW: f32 = 32.;
const HEADER: f32 = 28.;
/// `.subscriptionTableViewport`'s top padding, which the sticky header
/// starts below and scrolls up into.
const VIEWPORT_TOP: f32 = 8.;
/// The data plane narrower than this drops the Source column.
const NARROW_PLANE: f32 = 440.;

/// The square add button after a list of chips (`.subscriptionDestinationAdd`).
fn add_square(id: &'static str, cx: &App) -> Stateful<Div> {
    let theme = theme::theme(cx);
    div()
        .id(id)
        .group(id)
        .size(px(theme::CONTROL_SM))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_MD))
        .bg(theme.surface_hover())
        .hover(move |style| style.bg(theme.surface_active()))
        .cursor_pointer()
        .child(
            ui::icon("plus", 14., theme.text_secondary())
                .group_hover(id, move |style| style.text_color(theme.text())),
        )
}

/// One row of a table, with its cells at the given widths.
fn table_row(cells: Vec<(f32, AnyElement)>) -> Div {
    let mut row = div()
        .w_full()
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(10.));
    for (width, cell) in cells {
        row = row.child(
            div()
                .w(px(width))
                .flex_none()
                .min_w_0()
                .overflow_hidden()
                .child(cell),
        );
    }
    row
}

/// A header cell's text (`.subscriptionTableHeader`).
fn heading(label: &'static str, right: bool) -> AnyElement {
    let cell = div()
        .w_full()
        .whitespace_nowrap()
        .child(label.to_uppercase());
    if right { cell.text_right() } else { cell }.into_any_element()
}

/// A number cell (`.qCellNum`): right-aligned tabular figures.
fn number(value: i64, cx: &App) -> AnyElement {
    div()
        .w_full()
        .text_right()
        .font_features(theme::tabular_figures())
        .text_color(theme::theme(cx).text_secondary())
        .child(format::count(value))
        .into_any_element()
}

/// A time cell (`.qCellTime`).
fn time(text: String, cx: &App) -> AnyElement {
    div()
        .w_full()
        .truncate()
        .text_color(theme::theme(cx).text_tertiary())
        .child(text)
        .into_any_element()
}

/// Text centred in the data plane (`.subscriptionWorkspaceEmpty`).
fn plane_message(text: &'static str, cx: &App) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .text(theme::TEXT_MD)
        .text_color(theme::theme(cx).text_tertiary())
        .child(text)
}

impl SubscriptionsScreen {
    pub(super) fn workspace(
        &mut self,
        subscription: &data::Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let rail = self.rail(subscription, window, cx);
        let plane = self.plane(subscription, window, cx);
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .gap(px(12.))
            .pr(px(8.))
            .overflow_hidden()
            .font_family(theme::FONT_UI)
            .child(rail)
            .child(plane)
    }

    // ── The rail ────────────────────────────────────────────────

    fn rail(
        &mut self,
        subscription: &data::Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let id = subscription.id;
        let (tone, label) = format::detail_status(subscription);
        // A warning explains itself: the open problem, else the failure.
        let problem = (tone == Tone::Attention).then(|| {
            self.detail
                .as_ref()
                .filter(|detail| detail.subscription == id)
                .and_then(|detail| detail.issues.items.first())
                .map(|issue| issue.message.clone())
                .or_else(|| {
                    format::latest_failure(subscription).and_then(|query| {
                        format::describe_failure(
                            query.last_failure_kind.as_deref(),
                            query.last_failure_message.as_deref(),
                        )
                    })
                })
                .unwrap_or_else(|| "Run interrupted".to_owned())
        });

        let cover_size = RAIL_CONTENT - 32.;
        let cover = home::cover(
            "subscription-rail-cover",
            subscription.cover.as_ref(),
            cover_size,
            theme::RADIUS_LG,
            30.,
            &self.covers,
            cx,
        )
        .bg(theme.rail())
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), rgba(0x0000001a).into()).spread_radius(px(1.)),
        ])
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme::RADIUS_LG))
                .border_1()
                .border_color(theme.border_secondary()),
        );
        let name = if self.renaming == Some(id) {
            div()
                .w_full()
                .h(px(24.))
                .mb(px(8.))
                .flex()
                .items_center()
                .child(self.rename_field("subscription-rail-rename"))
                .into_any_element()
        } else {
            ui::tooltip::attach(
                div()
                    .id("subscription-rail-name")
                    .max_w_full()
                    .mb(px(8.))
                    .text(theme::TEXT_XL)
                    .line_height(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .text_center()
                    .child(subscription.name.clone())
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.click_count() == 2 {
                            this.start_rename(id, theme::TEXT_XL, window, cx);
                        }
                    })),
                "subscription-rail-name-tip",
                ui::tip("Double-click to rename"),
                24.,
                window,
                cx,
            )
            .into_any_element()
        };
        let identity = div()
            .w_full()
            .min_w_0()
            .pt(px(36.))
            .px(px(16.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .items_center()
            .child(div().mb(px(16.)).child(cover))
            .child(name)
            .child(
                div()
                    .min_h(px(18.))
                    .max_w(px(RAIL_CONTENT - 32. - 24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        ui::status_badge("subscription-rail-status", tone, label)
                            .tip(problem.map(ui::tip)),
                    ),
            );

        let content = div()
            .w(px(RAIL_CONTENT))
            .flex()
            .flex_col()
            .text(theme::TEXT_SM)
            .child(identity)
            .child(self.rail_actions(subscription, cx))
            .child(Self::run_progress(subscription, cx))
            .child(Self::properties(subscription, cx))
            .child(self.destination(subscription, cx));
        div()
            .relative()
            .w(px(RAIL))
            .flex_none()
            .h_full()
            .child(
                div()
                    .id("subscription-rail")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.rail_scroll)
                    .child(div().pt(px(8.)).pb(px(8.)).pl(px(8.)).child(content)),
            )
            .child(ui::scrollbar(
                "subscription-rail-scrollbar",
                &self.rail_scroll,
                window,
                cx,
            ))
            .into_any_element()
    }

    /// Run or pause, hold or stop, and the overflow menu, in the columns
    /// `minmax(0, 1fr) auto 24px`.
    fn rail_actions(
        &mut self,
        subscription: &data::Subscription,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = subscription.id;
        let active = subscription.active_run_id.is_some();
        let running = format::running(subscription);
        let paused_run = subscription.run_status.as_deref() == Some("paused");
        let run_failed = active
            && !running
            && !matches!(
                subscription.run_status.as_deref(),
                Some("paused" | "inbox_full")
            );
        // TODO: start, pause, resume and retry runs through the subscription
        // runner.
        let primary = if active {
            let (variant, icon, label) = if running {
                (Variant::Secondary, "player-pause", "Pause")
            } else if run_failed {
                (Variant::Primary, "refresh", "Retry")
            } else {
                (
                    Variant::Primary,
                    "player-play",
                    if paused_run { "Resume" } else { "Retry" },
                )
            };
            ui::action_button("subscription-run", variant, label).icon(icon)
        } else {
            ui::action_button("subscription-run", Variant::Primary, "Run now")
                .icon("player-play")
                .disabled(subscription.paused || subscription.queries.is_empty())
        };
        // TODO: stop the run, or hold and release future runs.
        let (hold_icon, hold_label, hold_tip) = if active {
            (
                "player-stop",
                "Stop",
                "Stop this run and keep the posts already added",
            )
        } else if subscription.paused {
            ("player-play", "Release", "Allow future runs again")
        } else {
            ("hand-stop", "Hold", "Put future runs on hold")
        };
        let overflow = self.overflow.clone();
        let bounds = self.overflow.clone();
        div()
            .mx(px(16.))
            .mb(px(20.))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .child(primary.padding(10.).full_width()),
            )
            .child(
                ui::action_button("subscription-hold", Variant::Secondary, hold_label)
                    .icon(hold_icon)
                    .padding(10.)
                    .tip(ui::tip(hold_tip)),
            )
            .child(
                // The 30px button overhangs its 24px column, as it does in
                // the Electron rail.
                div()
                    .relative()
                    .w(px(24.))
                    .h(px(30.))
                    .flex_none()
                    .child(
                        canvas(move |bounds, _, _| overflow.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .top_0()
                            .left_0()
                            .size(px(30.)),
                    )
                    .child(
                        ui::small_button("subscription-more", "dots-vertical")
                            .glyph(16.)
                            .size(30., theme::RADIUS_MD)
                            .tip(ui::tip("More actions"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let at = bounds.get();
                                let position = point(at.left(), at.bottom() + px(4.));
                                this.open_menu(vec![id], position, window, cx);
                            })),
                    ),
            )
    }

    /// The current run's progress (`.subscriptionRunProgress`), kept in the
    /// layout and hidden while no run is active.
    fn run_progress(subscription: &data::Subscription, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let active = subscription.active_run_id.is_some();
        let settled = if active {
            subscription.progress.posts_added + subscription.progress.posts_skipped
        } else {
            0
        };
        let downloaded = if active {
            subscription.progress.downloaded
        } else {
            0
        };
        let target = format::run_target(subscription);
        let share = if target > 0 {
            (settled as f32 / target as f32).min(1.)
        } else {
            0.
        };
        div()
            .min_h(px(38.))
            .mt(px(-8.))
            .mx(px(16.))
            .mb(px(12.))
            .opacity(if active { 1. } else { 0. })
            .child(
                div()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .whitespace_nowrap()
                    .child(format!(
                        "{} / {} posts processed",
                        format::count(settled),
                        format::count(target)
                    ))
                    .child(format!("{} files downloaded", format::count(downloaded))),
            )
            .child(
                div()
                    .h(px(3.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(theme.surface_active())
                    .child(
                        div()
                            .h_full()
                            .w(relative(share))
                            .rounded_full()
                            .bg(theme.primary()),
                    ),
            )
    }

    /// The run's counts, the last check, and the schedule and run limit
    /// (`.subscriptionProperties`).
    fn properties(subscription: &data::Subscription, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let has_run = subscription.run_status.is_some();
        let progress = subscription.progress;
        let stored_posts: i64 = subscription.queries.iter().map(|query| query.posts).sum();
        let of_run = |value: i64| format::count(if has_run { value } else { 0 });
        let row = |label: &'static str, value: AnyElement| {
            div()
                .h(px(26.))
                .mb(px(2.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .text(theme::TEXT_SM)
                .line_height(px(26.))
                .text_color(theme.text_secondary())
                .child(div().whitespace_nowrap().child(label))
                .child(value)
        };
        let value = |text: String| {
            div()
                .min_w_0()
                .truncate()
                .text_right()
                .text_color(theme.text())
                .child(text)
                .into_any_element()
        };
        let schedule = match subscription.schedule.as_str() {
            "manual" => "Manual",
            "daily" => "Daily",
            "weekly" => "Weekly",
            "monthly" => "Monthly",
            other => other,
        }
        .to_owned();
        div()
            .mx(px(16.))
            .pt(px(16.))
            .border_t_1()
            .border_color(theme.border())
            .flex()
            .flex_col()
            .child(row(
                "Posts traversed",
                value(of_run(progress.posts_traversed)),
            ))
            .child(row(
                "Posts added",
                value(format::count(if has_run {
                    progress.posts_added
                } else {
                    stored_posts
                })),
            ))
            .child(row("Posts skipped", value(of_run(progress.posts_skipped))))
            .child(row("Files downloaded", value(of_run(progress.downloaded))))
            .child(row(
                "Files already in library",
                value(of_run(progress.files_already_in_library)),
            ))
            .child(row(
                "Last check",
                value(format::relative_time(
                    format::last_check(subscription),
                    chrono::Local::now(),
                )),
            ))
            // TODO: open the schedule options and save the choice.
            .child(row(
                "Schedule",
                ui::select("subscription-schedule", schedule, 100.).into_any_element(),
            ))
            // TODO: edit the limit and save it.
            .child(row(
                "Posts per run",
                ui::number_input(
                    "subscription-posts-per-run",
                    subscription.posts_per_run.to_string(),
                )
                .disabled(format::running(subscription))
                .into_any_element(),
            ))
    }

    /// Where new files go (`.subscriptionDestination`): the folders and tags
    /// every downloaded file receives.
    fn destination(&self, subscription: &data::Subscription, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let nav = &self.session.read(cx).nav;
        let folder_label = |folder: i64| {
            nav.iter()
                .find(|entry| match &entry.scope {
                    ItemScope::FolderTree { folder_id } | ItemScope::Folder { folder_id } => {
                        i64::from(folder_id.0) == folder
                    }
                    _ => false,
                })
                .map_or_else(|| format!("Folder {folder}"), |entry| entry.label.clone())
        };
        // TODO: remove a folder or tag, and add them through the pickers.
        let mut folders = div()
            .min_h(px(26.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.));
        for folder in &subscription.folder_ids {
            folders = folders.child(ui::chip(
                ("subscription-folder", *folder as usize),
                folder_label(*folder),
                crate::inspector::FOLDER_COLOR,
            ));
        }
        folders = if subscription.folder_ids.is_empty() {
            folders.child(div().w_full().text_color(theme.text_secondary()).child(
                ui::action_row("subscription-add-folders", "plus", "Add to folders").glyph(14.),
            ))
        } else {
            folders.child(add_square("subscription-add-folder", cx))
        };
        let mut tags = div()
            .min_h(px(26.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.));
        for (index, tag) in subscription.automatic_tags.iter().enumerate() {
            let (namespace, subtag) = tag.split_once(':').unwrap_or(("", tag));
            let (color, _) = crate::inspector::tag_style(namespace);
            tags = tags.child(ui::chip(
                ("subscription-tag", index),
                subtag.to_owned(),
                color,
            ));
        }
        tags = if subscription.automatic_tags.is_empty() {
            tags.child(
                div()
                    .w_full()
                    .text_color(theme.text_tertiary())
                    .child(ui::action_row("subscription-add-tags", "plus", "Add tags").glyph(14.)),
            )
        } else {
            tags.child(add_square("subscription-add-tag", cx))
        };
        div()
            .mx(px(16.))
            .mt(px(16.))
            .pt(px(16.))
            .border_t_1()
            .border_color(theme.border())
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text(theme::TEXT_SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("New files"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child("Automatically add to folders")
                    .child(folders),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text(theme::TEXT_MD)
                    .text_color(theme.text())
                    .child("Automatically add tags")
                    .child(tags),
            )
    }

    // ── The data plane ──────────────────────────────────────────

    fn plane(
        &mut self,
        subscription: &data::Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let detail = self
            .detail
            .as_ref()
            .filter(|detail| detail.subscription == subscription.id)
            .cloned();
        let body = match self.tab {
            SOURCES if subscription.queries.is_empty() => {
                plane_message("No sources yet.", cx).into_any_element()
            }
            SOURCES => self.sources(subscription, window, cx).into_any_element(),
            HISTORY => match &detail {
                None => plane_message("Loading history…", cx).into_any_element(),
                Some(detail) => self.history(detail, window, cx).into_any_element(),
            },
            _ => match &detail {
                None => plane_message("Checking for problems…", cx).into_any_element(),
                Some(detail) if detail.issues.total == 0 => {
                    plane_message("No problems found.", cx).into_any_element()
                }
                Some(detail) => self.problems(detail, window, cx).into_any_element(),
            },
        };
        let footer = (self.tab == SOURCES).then(|| Self::add_query_bar(cx));
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .my(px(8.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(theme::RADIUS_LG))
            .border_1()
            .border_color(theme.border())
            .bg(theme.rail())
            .shadow(vec![
                BoxShadow::new(px(0.), px(0.), rgba(0x00000040).into()).spread_radius(px(1.)),
                BoxShadow::new(px(0.), px(0.), rgba(0x00000026).into()).blur_radius(px(12.)),
            ])
            .child(measure(self.plane_width.clone(), window))
            .child(
                div()
                    .h(px(48.))
                    .flex_none()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        ui::tabs(
                            "subscription-tabs",
                            TABS.iter().map(|tab| SharedString::from(*tab)).collect(),
                            self.tab,
                        )
                        .on_select({
                            let view = cx.entity().downgrade();
                            move |tab, _, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.tab = tab;
                                    this.table_scroll.set_offset(point(px(0.), px(0.)));
                                    cx.notify();
                                });
                            }
                        }),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(body),
            )
            .child(
                div()
                    .h(px(52.))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(theme.border_secondary())
                    .children(footer),
            )
            .into_any_element()
    }

    /// The width a table row's cells share: the plane's inside less the
    /// viewport's padding and gutter and the row's own padding.
    fn row_width(&self) -> f32 {
        (self.plane_width.get() - 2. - 32. - ui::scrollbar::GUTTER - 24.).max(0.)
    }

    fn narrow(&self) -> bool {
        self.plane_width.get() - 2. <= NARROW_PLANE
    }

    /// A scrolling table (`.subscriptionTableViewport`) whose header stays at
    /// the top once the rows scroll under it.
    fn table(
        &self,
        id: &'static str,
        header: Div,
        rows: Vec<AnyElement>,
        after: Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let scrolled = -f32::from(self.table_scroll.offset().y);
        let header_top = (VIEWPORT_TOP - scrolled).max(0.);
        let header = header
            .h(px(HEADER))
            .bg(theme.footer_surface())
            .text(theme::TEXT_CAPTION)
            .line_height(px(theme::TEXT_CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text_tertiary());
        let mut column = div()
            .pt(px(VIEWPORT_TOP))
            .pb(px(16.))
            .pl(px(16.))
            .pr(px(16. + ui::scrollbar::GUTTER))
            .flex()
            .flex_col()
            // The header's place in the flow; it is drawn over the rows.
            .child(div().h(px(HEADER)).flex_none());
        for row in rows {
            column = column.child(row);
        }
        column = column.children(after);
        div()
            .absolute()
            .inset_0()
            .text(theme::TEXT_MD)
            .child(
                div()
                    .id(id)
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.table_scroll)
                    .child(column),
            )
            .child(
                div()
                    .absolute()
                    .top(px(header_top))
                    .left(px(16.))
                    .right(px(16. + ui::scrollbar::GUTTER))
                    .child(header),
            )
            .child(ui::scrollbar((id, 1usize), &self.table_scroll, window, cx))
            .into_any_element()
    }

    /// A body row: 32px or taller, its hover surface, and a rule above it.
    fn body_row(&self, id: ElementId, row: Div, cx: &App) -> Stateful<Div> {
        let theme = theme::theme(cx);
        row.id(id)
            .min_h(px(ROW))
            .border_t_1()
            .border_color(theme.border_secondary())
            .hover(move |style| style.bg(theme.surface_hover()))
    }

    fn sources(
        &self,
        subscription: &data::Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let narrow = self.narrow();
        let columns = if narrow {
            vec![
                Track::Flex(0., 1.),
                Track::Fixed(44.),
                Track::Fixed(44.),
                Track::Fixed(128.),
            ]
        } else {
            vec![
                Track::Flex(90., 0.28),
                Track::Flex(0., 1.),
                Track::Fixed(76.),
                Track::Fixed(76.),
                Track::Fixed(128.),
            ]
        };
        let widths = format::tracks(self.row_width(), 10., &columns);
        let cells = |mut values: Vec<AnyElement>| -> Vec<(f32, AnyElement)> {
            if narrow {
                values.remove(0);
            }
            widths.iter().copied().zip(values).collect()
        };
        let header = table_row(cells(vec![
            heading("Source", false),
            heading("Query", false),
            heading("Posts", true),
            heading("Media", true),
            div().into_any_element(),
        ]));
        let active = subscription.active_run_id.is_some();
        let running = format::running(subscription);
        let mut rows = Vec::new();
        for query in &subscription.queries {
            let query_id = query.id as usize;
            let fade =
                window.use_keyed_state(("subscription-query-hover", query_id), cx, |_, _| {
                    motion::Fade::default()
                });
            let hover = fade.read(cx).value_with(motion::EASE);
            if fade.read(cx).animating() {
                window.request_animation_frame();
            }
            let mut name = div()
                .id(("subscription-query-name", query_id))
                .w_full()
                .flex()
                .items_center()
                .gap(px(6.))
                .overflow_hidden()
                .child(div().min_w_0().truncate().child(query.text.clone()));
            if query.history_complete && query.initial_run_complete {
                name = name.child(ui::tooltip::attach(
                    div()
                        .id(("subscription-query-complete", query_id))
                        .flex_none()
                        .child(ui::icon("circle-check-filled", 13., rgb(theme::SUCCESS))),
                    ("subscription-query-complete-tip", query_id),
                    ui::tip("Checked all available posts"),
                    13.,
                    window,
                    cx,
                ));
            }
            if let Some(message) = query
                .last_failure_message
                .clone()
                .filter(|_| !(running && !query.paused))
            {
                name = name.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text(theme::TEXT_SM)
                        .text_color(rgb(theme::FAILURE_TEXT))
                        .child(message),
                );
            }
            // TODO: the source details window on a double-click.
            let name = ui::tooltip::attach(
                name,
                ("subscription-query-tip", query_id),
                ui::tip("Double-click for source details"),
                17.,
                window,
                cx,
            );
            // TODO: run, group, hold, edit and delete a source, through the
            // subscription runner and the catalog.
            let actions = div()
                .w_full()
                .flex()
                .justify_end()
                .gap(px(2.))
                .opacity(hover)
                .child(
                    ui::small_button(("query-run", query_id), "player-play")
                        .disabled(active)
                        .tip(ui::tip("Run only this source now")),
                )
                .child(
                    ui::small_button(("query-group", query_id), "copy-2")
                        .active(query.group_posts)
                        .disabled(active)
                        .tip(ui::tip(if query.group_posts {
                            "Group multi-media posts"
                        } else {
                            "Keep post media separate"
                        })),
                )
                .child(
                    ui::small_button(
                        ("query-hold", query_id),
                        if query.paused {
                            "player-play"
                        } else {
                            "hand-stop"
                        },
                    )
                    .disabled(active)
                    .tip(ui::tip(if query.paused {
                        "Resume this source in future subscription runs"
                    } else {
                        "Put this source on hold until it is resumed"
                    })),
                )
                .child(
                    ui::small_button(("query-edit", query_id), "pencil")
                        .disabled(active)
                        .tip(ui::tip("Edit")),
                )
                .child(
                    ui::small_button(("query-delete", query_id), "trash-2")
                        .disabled(active)
                        .tip(ui::tip("Delete")),
                );
            let source = format::site_label(&query.site_id);
            let row = table_row(cells(vec![
                div()
                    .w_full()
                    .truncate()
                    .text_color(theme.text_secondary())
                    .child(source)
                    .into_any_element(),
                name.into_any_element(),
                number(query.posts, cx),
                number(query.media, cx),
                actions.into_any_element(),
            ]));
            rows.push(
                self.body_row(("subscription-query", query_id).into(), row, cx)
                    .on_hover(move |hovered, _, cx| {
                        fade.update(cx, |fade, cx| {
                            fade.set(*hovered, motion::SMALL_BUTTON);
                            cx.notify();
                        })
                    })
                    .into_any_element(),
            );
        }
        self.table("subscription-sources", header, rows, None, window, cx)
    }

    fn history(&self, detail: &Detail, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if detail.runs.is_empty() {
            return div()
                .pt(px(VIEWPORT_TOP + 4.))
                .px(px(16.))
                .text(theme::TEXT_MD)
                .text_color(theme::theme(cx).text_tertiary())
                .child("No runs yet.")
                .into_any_element();
        }
        let widths = format::tracks(
            self.row_width(),
            10.,
            &[
                Track::Fixed(150.),
                Track::Fixed(150.),
                Track::Fixed(90.),
                Track::Fixed(70.),
                Track::Flex(120., 1.),
            ],
        );
        let cells = |values: Vec<AnyElement>| -> Vec<(f32, AnyElement)> {
            widths.iter().copied().zip(values).collect()
        };
        let header = table_row(cells(vec![
            heading("Started", false),
            heading("Status", false),
            heading("Fetched", true),
            heading("Reused", true),
            heading("Notes", false),
        ]));
        let rows = detail
            .runs
            .iter()
            .map(|run| {
                let (tone, label) = format::run_status(run);
                let row = table_row(cells(vec![
                    time(format::date_time(Some(&run.started_at)), cx),
                    div()
                        .flex()
                        .child(
                            ui::status_badge(("run-status", run.id as usize), tone, label)
                                .tip(run.error_message.clone().map(ui::tip)),
                        )
                        .into_any_element(),
                    number(run.downloaded, cx),
                    number(run.already_in_library, cx),
                    time(run.error_message.clone().unwrap_or_default(), cx),
                ]));
                self.body_row(("subscription-run", run.id as usize).into(), row, cx)
                    .into_any_element()
            })
            .collect();
        self.table("subscription-history", header, rows, None, window, cx)
    }

    fn problems(&self, detail: &Detail, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let narrow = self.narrow();
        let columns = if narrow {
            vec![
                Track::Flex(80., 0.35),
                Track::Flex(0., 1.),
                Track::Flex(0., 1.),
                Track::Fixed(52.),
            ]
        } else {
            vec![
                Track::Flex(92., 0.25),
                Track::Flex(140., 0.8),
                Track::Flex(180., 1.),
                Track::Between(82., 110.),
                Track::Fixed(52.),
            ]
        };
        let widths = format::tracks(self.row_width(), 10., &columns);
        let cells = |mut values: Vec<AnyElement>| -> Vec<(f32, AnyElement)> {
            if narrow {
                values.remove(3);
            }
            widths.iter().copied().zip(values).collect()
        };
        let header = table_row(cells(vec![
            heading("Status", false),
            heading("Post", false),
            heading("Why it failed", false),
            heading("Last seen", false),
            div().into_any_element(),
        ]));
        let now = chrono::Local::now();
        let rows = detail
            .issues
            .items
            .iter()
            .map(|issue| {
                let target_label = issue
                    .target
                    .clone()
                    .unwrap_or_else(|| "Subscription".to_owned());
                let target = match issue.url.clone() {
                    Some(url) => div()
                        .id(("issue-link", issue.id as usize))
                        .w_full()
                        .truncate()
                        .cursor_pointer()
                        .hover(move |style| style.text_color(theme.primary()).underline())
                        .child(target_label)
                        .on_click(move |_, _, cx| cx.open_url(&url))
                        .into_any_element(),
                    None => div()
                        .w_full()
                        .truncate()
                        .child(target_label)
                        .into_any_element(),
                };
                // TODO: open the accounts window for a login problem.
                let fix = (issue.kind.contains("auth")
                    || issue.kind.contains("credential")
                    || issue.kind == "unauthorized")
                    .then(|| {
                        ui::small_button(("issue-fix", issue.id as usize), "shield-lock")
                            .glyph(13.)
                            .tip(ui::tip("Fix login"))
                    });
                let row = table_row(cells(vec![
                    div()
                        .flex()
                        .child(ui::status_badge(
                            ("issue-status", issue.id as usize),
                            Tone::Attention,
                            format::issue_label(&issue.kind),
                        ))
                        .into_any_element(),
                    target,
                    div()
                        .w_full()
                        .truncate()
                        .text_color(theme.text_secondary())
                        .child(issue.message.clone())
                        .into_any_element(),
                    time(format::relative_time(Some(&issue.last_seen_at), now), cx),
                    div()
                        .w_full()
                        .flex()
                        .justify_end()
                        .children(fix)
                        .into_any_element(),
                ]));
                self.body_row(("subscription-issue", issue.id as usize).into(), row, cx)
                    .into_any_element()
            })
            .collect();
        let more = detail.issues.next.is_some().then(|| {
            div()
                .min_h(px(40.))
                .px(px(12.))
                .flex()
                .items_center()
                .justify_end()
                .gap(px(6.))
                .border_t_1()
                .border_color(theme.border_secondary())
                .child(
                    ui::action_button("issues-more", Variant::Secondary, "Load more")
                        .compact()
                        .disabled(self.loading_more)
                        .on_click(cx.listener(|this, _, _, cx| this.load_more_issues(cx))),
                )
                .into_any_element()
        });
        self.table("subscription-problems", header, rows, more, window, cx)
    }

    /// The add-query bar under the sources (`AddQueryBar.tsx`): the source,
    /// its query, and Add.
    fn add_query_bar(cx: &App) -> Div {
        let theme = theme::theme(cx);
        let (site, example) = format::first_query_site();
        // TODO: choose the source, type the query with the site's tag
        // suggestions, and add it to the subscription.
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::select("add-query-site", site, 140.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(theme::CONTROL_LG))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(theme::RADIUS_SM))
                    .border_1()
                    .border_color(theme.border())
                    .bg(theme.control_surface())
                    .text(theme::TEXT_MD)
                    .text_color(theme.text_tertiary())
                    .truncate()
                    .child(format!("e.g. {example}")),
            )
            .child(
                ui::action_button("add-query", Variant::Secondary, "Add")
                    .compact()
                    .disabled(true),
            )
    }
}
