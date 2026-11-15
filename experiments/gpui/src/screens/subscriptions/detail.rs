//! One subscription's page (`SubscriptionDetail.tsx`): a compact header
//! with its cover, name and state, over its sources, runs and problems as
//! full-width tables. Its counters, schedule and where new files go are in
//! the right rail, built as the inspector is; Run now, Hold and the overflow
//! menu are in the titlebar (`toolbar.rs`).
use super::data::{self, Detail};
use super::format::{self, Track};
use super::{HISTORY, SOURCES, SubscriptionsScreen, TABS, home, measure, place_of};
use crate::i18n::t;
use crate::theme::Text as _;
use crate::ui::status_badge::Tone;
use crate::ui::{self, action_button::Look};
use crate::{motion, theme};
use gpui::{prelude::FluentBuilder as _, *};
use picto_library::query::ItemScope;

/// The page's side padding, the grid's.
const INSET: f32 = 16.;
/// A row's own padding inside the table, so the hover surface reaches past
/// its text while the text lines up with the header's.
const ROW_INSET: f32 = 8.;
/// The cover in the header.
const COVER: f32 = 48.;
/// A table row's and the column header's height.
const ROW: f32 = 32.;
const HEADING: f32 = 28.;
/// The space between columns.
const COLUMN_GAP: f32 = 12.;
/// The add-source row under the sources.
const ADD_ROW: f32 = 44.;

/// A table column: its track, and when it gives way as the page narrows
/// (the lowest rank first; `0` never).
#[derive(Clone, Copy)]
struct Column {
    track: Track,
    drops: u8,
}

const fn column(track: Track, drops: u8) -> Column {
    Column { track, drops }
}

fn minimum(track: Track) -> f32 {
    match track {
        Track::Fixed(size) | Track::Between(size, _) | Track::Flex(size, _) => size,
    }
}

/// Each column's width across `width`, or `None` for one that gave way:
/// columns are dropped, lowest rank first, until the rest fit at their
/// minimums.
fn fit(width: f32, columns: &[Column]) -> Vec<Option<f32>> {
    let mut shown = vec![true; columns.len()];
    loop {
        let count = shown.iter().filter(|shown| **shown).count();
        let needed: f32 = columns
            .iter()
            .zip(&shown)
            .filter(|(_, shown)| **shown)
            .map(|(column, _)| minimum(column.track))
            .sum::<f32>()
            + COLUMN_GAP * count.saturating_sub(1) as f32;
        if needed <= width {
            break;
        }
        let next = columns
            .iter()
            .enumerate()
            .filter(|(index, column)| shown[*index] && column.drops > 0)
            .min_by_key(|(_, column)| column.drops)
            .map(|(index, _)| index);
        match next {
            Some(index) => shown[index] = false,
            None => break,
        }
    }
    let tracks: Vec<Track> = columns
        .iter()
        .zip(&shown)
        .filter(|(_, shown)| **shown)
        .map(|(column, _)| column.track)
        .collect();
    let mut widths = format::tracks(width, COLUMN_GAP, &tracks).into_iter();
    shown
        .into_iter()
        .map(|shown| if shown { widths.next() } else { None })
        .collect()
}

/// One row of a table, the cells of the columns shown at their widths.
fn table_row(widths: &[Option<f32>], cells: Vec<AnyElement>) -> Div {
    let mut row = div()
        .w_full()
        .px(px(ROW_INSET))
        .flex()
        .items_center()
        .gap(px(COLUMN_GAP));
    for (width, cell) in widths.iter().zip(cells) {
        if let Some(width) = width {
            row = row.child(
                div()
                    .w(px(*width))
                    .flex_none()
                    .min_w_0()
                    .overflow_hidden()
                    .child(cell),
            );
        }
    }
    row
}

/// A column's heading.
fn heading(label: &'static str, right: bool) -> AnyElement {
    div()
        .w_full()
        .flex()
        .when(right, |cell| cell.justify_end())
        .truncate()
        .child(label)
        .into_any_element()
}

/// A number cell: right-aligned figures in the face the sidebar's and the
/// inspector's numbers use.
fn number(value: i64, cx: &App) -> AnyElement {
    div()
        .w_full()
        .text_right()
        .font_family(theme::FONT_INSPECTOR_MONO)
        .font_features(theme::tabular_figures())
        .text(theme::TEXT_SM)
        .text_color(theme::theme(cx).text_secondary())
        .child(crate::i18n::format::grouped(value))
        .into_any_element()
}

/// A time or a note.
fn quiet(text: String, cx: &App) -> AnyElement {
    div()
        .w_full()
        .truncate()
        .text_color(theme::theme(cx).text_tertiary())
        .child(text)
        .into_any_element()
}

/// A line of the table that says there is nothing (yet) to show.
fn table_note(text: &'static str, cx: &App) -> AnyElement {
    div()
        .h(px(ROW))
        .px(px(ROW_INSET))
        .flex()
        .items_center()
        .text_color(theme::theme(cx).text_tertiary())
        .child(text)
        .into_any_element()
}

/// A row of the rail's Properties holding a control: the property's label
/// column, the control at the right.
fn control_row(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .h(px(theme::CONTROL_SM))
        .mt(px(4.))
        .flex()
        .items_center()
        .gap(px(12.))
        .whitespace_nowrap()
        .text(theme::TEXT_CAPTION)
        .child(div().min_w(px(88.)).flex_none().child(label))
        .child(div().flex_1().min_w_0().flex().justify_end().child(control))
}

impl SubscriptionsScreen {
    pub(super) fn workspace(
        &mut self,
        subscription: &data::Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let header = self.header(subscription, window, cx);
        let tabs = ui::segmented::segmented(
            "subscription-tabs",
            ui::segmented::Look::Tabs,
            TABS.iter().map(|tab| ui::segmented::tab(t(tab))).collect(),
            Some(self.tab),
        )
        .name(t("Subscription details"))
        .on_select({
            let view = cx.entity().downgrade();
            move |tab, _, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.tab = tab;
                    this.table_scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                });
            }
        });
        let table = self.plane(subscription, window, cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(theme::FONT_UI)
            .child(measure(self.plane_width.clone(), window))
            .child(header)
            .child(
                div()
                    .flex_none()
                    .mt(px(16.))
                    .px(px(INSET))
                    .flex()
                    .child(tabs),
            )
            .child(div().relative().flex_1().min_h_0().mt(px(8.)).child(table))
    }

    // ── The header ──────────────────────────────────────────────

    /// The cover, the name (double-click renames), the state with what is
    /// wrong in its tooltip, and the progress of a run while one is active.
    fn header(
        &mut self,
        subscription: &data::Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
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
                .unwrap_or_else(|| crate::i18n::en("Run interrupted").to_owned())
        });
        let cover = home::cover(
            "subscription-header-cover",
            subscription.cover.as_ref(),
            COVER,
            theme::RADIUS_MD,
            20.,
            &self.covers,
            cx,
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme::RADIUS_MD))
                .border_1()
                .border_color(theme.border()),
        );
        let name = ui::tooltip::attach(
            ui::a11y::text(
                div().id("subscription-header-name"),
                subscription.name.clone(),
            )
            .max_w_full()
            .text(theme::TEXT_XL)
            .line_height(px(22.))
            .font_weight(FontWeight::SEMIBOLD)
            .truncate()
            .child(subscription.name.clone())
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 {
                    this.open_rename(id, window, cx);
                }
            })),
            "subscription-header-name-tip",
            ui::tip(t("Double-click to rename")),
            22.,
            window,
            cx,
        );
        let state = div()
            .h(px(18.))
            .mt(px(4.))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                ui::status_badge("subscription-header-status", tone, label)
                    .tip(problem.map(ui::tip)),
            )
            .children(Self::run_progress(subscription, cx));
        div()
            .flex_none()
            .pt(px(INSET))
            .px(px(INSET))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(cover)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(name)
                    .child(state)
                    .child(Self::run_bar(subscription, cx)),
            )
    }

    /// While a run is active, how far it has got, after the state.
    fn run_progress(subscription: &data::Subscription, cx: &App) -> Option<Div> {
        subscription.active_run_id?;
        let progress = subscription.progress;
        let settled = progress.posts_added + progress.posts_skipped;
        Some(
            div()
                .min_w_0()
                .truncate()
                .text(theme::TEXT_CAPTION)
                .font_features(theme::tabular_figures())
                .text_color(theme::theme(cx).text_tertiary())
                .child(format!(
                    "{} / {} {} · {} {}",
                    crate::i18n::format::grouped(settled),
                    crate::i18n::format::grouped(format::run_target(subscription)),
                    t("posts processed"),
                    crate::i18n::format::grouped(progress.downloaded),
                    t("files downloaded")
                )),
        )
    }

    /// The run's progress line under the state, kept in the layout and
    /// hidden while no run is active.
    fn run_bar(subscription: &data::Subscription, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let active = subscription.active_run_id.is_some();
        let settled = subscription.progress.posts_added + subscription.progress.posts_skipped;
        let target = format::run_target(subscription);
        let share = if active && target > 0 {
            (settled as f32 / target as f32).min(1.)
        } else {
            0.
        };
        div()
            .mt(px(3.))
            .max_w(px(280.))
            .opacity(if active { 1. } else { 0. })
            .child(
                ui::progress_bar()
                    .height(3.)
                    .radius(9999.)
                    .track(theme.surface_active())
                    .filled(
                        ElementId::NamedInteger("run-progress".into(), subscription.id as u64),
                        share,
                    ),
            )
    }

    // ── The rail ────────────────────────────────────────────────

    /// The rail beside the page, built as the inspector's: its scrolling
    /// column, sections and property rows.
    pub(super) fn rail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(subscription) = self.opened().cloned() else {
            return div().into_any_element();
        };
        let properties = self.properties(&subscription, cx);
        let destination = self.destination(&subscription, cx);
        let content = div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                self.rail_section(t("Properties"), properties, cx)
                    .leading(true),
            )
            .child(self.rail_section(t("New files"), destination, cx));
        div()
            .relative()
            .size_full()
            .child(place_of(self.rail_bounds.clone()))
            .child(
                div()
                    .id("subscription-rail")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.rail_scroll)
                    .pl(px(16.))
                    .pr(px(16. + ui::scrollbar::GUTTER))
                    .pt(px(16.))
                    .pb(px(48.))
                    .font_family(theme::FONT_INSPECTOR)
                    .child(content),
            )
            .child(ui::scrollbar(
                "subscription-rail-scrollbar",
                &self.rail_scroll,
                window,
                cx,
            ))
            .into_any_element()
    }

    /// A section of the rail, folded as the inspector's are.
    fn rail_section(
        &self,
        title: &'static str,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> ui::PanelSection {
        ui::panel_section(title, title)
            .expanded(!self.collapsed.contains(title))
            .content(Some(content))
            .on_toggle(cx.listener(move |this, _, _, cx| {
                if !this.collapsed.remove(title) {
                    this.collapsed.insert(title);
                }
                cx.notify();
            }))
    }

    /// The run's counts and the last check as property rows, then the
    /// schedule and the run limit.
    fn properties(&self, subscription: &data::Subscription, cx: &mut Context<Self>) -> AnyElement {
        let id = subscription.id;
        let view = cx.entity().downgrade();
        let posts_view = view.clone();
        let has_run = subscription.run_status.is_some();
        let progress = subscription.progress;
        let stored_posts: i64 = subscription.queries.iter().map(|query| query.posts).sum();
        let of_run = |value: i64| crate::i18n::format::grouped(if has_run { value } else { 0 });
        div()
            .flex()
            .flex_col()
            .child(ui::property_row(
                t("Posts traversed"),
                of_run(progress.posts_traversed),
            ))
            .child(ui::property_row(
                t("Posts added"),
                crate::i18n::format::grouped(if has_run {
                    progress.posts_added
                } else {
                    stored_posts
                }),
            ))
            .child(ui::property_row(
                t("Posts skipped"),
                of_run(progress.posts_skipped),
            ))
            .child(ui::property_row(
                t("Files downloaded"),
                of_run(progress.downloaded),
            ))
            .child(ui::property_row(
                t("Files already in library"),
                of_run(progress.files_already_in_library),
            ))
            .child(ui::property_row(
                t("Last check"),
                format::relative_time(format::last_check(subscription), chrono::Local::now()),
            ))
            // `setSchedule`
            .child(
                control_row(
                    t("Schedule"),
                    ui::select(
                        "subscription-schedule",
                        subscription.schedule.clone(),
                        vec![
                            ui::choice("manual", "Manual"),
                            ui::choice("daily", "Daily"),
                            ui::choice("weekly", "Weekly"),
                            ui::choice("monthly", "Monthly"),
                        ],
                    )
                    .width(100.)
                    .name(t("Schedule"))
                    .on_change(move |schedule, _, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.perform(vec![id], |_| super::Command::Schedule(schedule), cx)
                        });
                    }),
                )
                .mt(px(8.)),
            )
            // `setPostsPerRun`
            .child(control_row(
                t("Posts per run"),
                ui::number_input(
                    "subscription-posts-per-run",
                    subscription.posts_per_run,
                    1,
                    10_000,
                )
                .name(t("Posts per run"))
                .disabled(format::running(subscription))
                .on_commit(move |posts, cx| {
                    let _ = posts_view.update(cx, |this, cx| {
                        this.perform(vec![id], |_| super::Command::PostsPerRun(posts), cx)
                    });
                }),
            ))
            .into_any_element()
    }

    /// The inspector's "+" after a list of chips, or its wide add row when
    /// the list is empty; the picker opens beside the rail at it.
    fn add_control(
        &self,
        id: &'static str,
        label: &'static str,
        empty: bool,
        place: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
        open: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = std::rc::Rc::new(open);
        let listener = cx.listener(move |this, _: &ClickEvent, window, cx| open(this, window, cx));
        if empty {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .relative()
                .w_full()
                .child(place_of(place))
                .child(ui::action_row(
                    SharedString::from(format!("{id}-action")),
                    "inspector-add",
                    label,
                ))
                .on_click(listener)
                .into_any_element()
        } else {
            div()
                .relative()
                .child(place_of(place))
                .child(
                    ui::icon_button(id, "inspector-add")
                        .look(ui::icon_button::Look::TagAdd)
                        .tip(ui::tip(label))
                        .on_click(listener),
                )
                .into_any_element()
        }
    }

    /// Where new files go (`.subscriptionDestination`): the folders and tags
    /// every downloaded file receives, each removed with its chip's button
    /// and added through the folder and tag pickers.
    fn destination(&self, subscription: &data::Subscription, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let id = subscription.id;
        let nav = &self.session.read(cx).nav;
        let folder_label = |folder: i64| {
            nav.iter()
                .find(|entry| match &entry.scope {
                    ItemScope::FolderTree { folder_id } | ItemScope::Folder { folder_id } => {
                        i64::from(folder_id.0) == folder
                    }
                    _ => false,
                })
                .map(|entry| entry.label.clone())
        };
        let mut folders = div().flex().flex_wrap().items_center().gap(px(4.));
        // `selectedFolders`: the folders the library still has.
        let mut shown = 0;
        for folder in &subscription.folder_ids {
            let Some(label) = folder_label(*folder) else {
                continue;
            };
            shown += 1;
            let folder = *folder;
            folders = folders.child(
                ui::chip(
                    ("subscription-folder", folder as usize),
                    label,
                    crate::inspector::FOLDER_COLOR,
                )
                .on_remove(cx.listener(move |this, _, _, cx| this.remove_folder(id, folder, cx))),
            );
        }
        folders = folders.child(self.add_control(
            "subscription-add-folders",
            t("Add to folders"),
            shown == 0,
            self.add_folder.clone(),
            move |this, window, cx| this.pick_folders(id, window, cx),
            cx,
        ));
        let show_prefixes = crate::settings::current(cx).show_tag_prefixes;
        let mut tags = div().flex().flex_wrap().items_center().gap(px(4.));
        for (index, tag) in subscription.automatic_tags.iter().enumerate() {
            let (namespace, subtag) = crate::tags::split(tag);
            let namespace = namespace.unwrap_or("general");
            let removed = tag.clone();
            tags = tags.child(
                ui::chip(
                    ("subscription-tag", index),
                    subtag.to_owned(),
                    crate::tags::chip_color(namespace),
                )
                .namespace(
                    (show_prefixes && crate::tags::chip_shows_namespace(namespace))
                        .then(|| namespace.to_owned().into()),
                )
                .on_remove(cx.listener(move |this, _, _, cx| this.remove_tag(id, &removed, cx))),
            );
        }
        tags = tags.child(self.add_control(
            "subscription-add-tags",
            t("Add tags"),
            subscription.automatic_tags.is_empty(),
            self.add_tag.clone(),
            move |this, window, cx| this.pick_tags(id, window, cx),
            cx,
        ));
        let caption = |text: &'static str| {
            div()
                .h(px(20.))
                .flex()
                .items_center()
                .text(theme::TEXT_CAPTION)
                .text_color(theme.text_secondary())
                .child(text)
        };
        div()
            .flex()
            .flex_col()
            .child(caption(t("Automatically add to folders")))
            .child(div().mt(px(4.)).child(folders))
            .child(caption(t("Automatically add tags")).mt(px(10.)))
            .child(div().mt(px(4.)).child(tags))
            .into_any_element()
    }

    // ── The tables ──────────────────────────────────────────────

    fn plane(
        &mut self,
        subscription: &data::Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = self
            .detail
            .as_ref()
            .filter(|detail| detail.subscription == subscription.id)
            .cloned();
        match self.tab {
            SOURCES => self.sources(subscription, window, cx),
            HISTORY => self.history(detail.as_ref(), window, cx),
            _ => self.problems(detail.as_ref(), window, cx),
        }
    }

    /// The width the cells of a row share.
    fn row_width(&self) -> f32 {
        (self.plane_width.get() - 2. * (INSET - ROW_INSET) - ui::scrollbar::GUTTER - 2. * ROW_INSET)
            .max(0.)
    }

    /// A table across the page: its column headings, then the rows, which
    /// scroll under them, and whatever follows the rows.
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
        let side = |element: Div| {
            element
                .pl(px(INSET - ROW_INSET))
                .pr(px(INSET - ROW_INSET + ui::scrollbar::GUTTER))
        };
        let header = side(div().flex_none()).child(
            header
                .h(px(HEADING))
                .border_b_1()
                .border_color(theme.border())
                .text(theme::TEXT_CAPTION)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_tertiary()),
        );
        let column = side(div().pb(px(INSET)).flex().flex_col().text(theme::TEXT_MD))
            .children(rows)
            .children(after);
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id(id)
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.table_scroll)
                            .child(column),
                    )
                    .child(ui::scrollbar((id, 1usize), &self.table_scroll, window, cx)),
            )
            .into_any_element()
    }

    /// A body row: 32px or taller, a hairline under it, and its hover
    /// surface.
    fn body_row(&self, id: ElementId, row: Div, cx: &App) -> Stateful<Div> {
        let theme = theme::theme(cx);
        row.id(id)
            .min_h(px(ROW))
            .border_b_1()
            .border_color(theme.border_secondary())
            .hover(move |style| style.bg(theme.surface_hover()))
    }

    /// Sources: the query, its actions (shown under the pointer) next to
    /// it, and the counts at the right edge. When narrow the media count
    /// gives way first, then the source.
    fn sources(
        &self,
        subscription: &data::Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let widths = fit(
            self.row_width(),
            &[
                column(Track::Flex(84., 0.35), 2),
                column(Track::Flex(140., 1.), 0),
                column(Track::Fixed(128.), 0),
                column(Track::Fixed(64.), 0),
                column(Track::Fixed(64.), 1),
            ],
        );
        let header = table_row(
            &widths,
            vec![
                heading(t("Source"), false),
                heading(t("Query"), false),
                div().into_any_element(),
                heading(t("Posts"), true),
                heading(t("Media"), true),
            ],
        );
        let active = subscription.active_run_id.is_some();
        let running = format::running(subscription);
        // `busy || subscriptionRunning`: the row's buttons wait.
        let waiting = self.busy() || active;
        let mut rows = Vec::new();
        if subscription.queries.is_empty() {
            rows.push(table_note(t("No sources yet."), cx));
        }
        for query in &subscription.queries {
            let query_id = query.id as usize;
            let id = query.id;
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
                .child(
                    // The query keeps up to two thirds of the cell; what is
                    // wrong with it is cut short first.
                    div()
                        .flex_shrink_0()
                        .max_w(relative(0.67))
                        .truncate()
                        .child(query.text.clone()),
                );
            if query.history_complete && query.initial_run_complete {
                name = name.child(ui::tooltip::attach(
                    div()
                        .id(("subscription-query-complete", query_id))
                        .flex_none()
                        .child(ui::icon("circle-check-filled", 13., rgb(theme::SUCCESS))),
                    ("subscription-query-complete-tip", query_id),
                    ui::tip(t("Checked all available posts")),
                    13.,
                    window,
                    cx,
                ));
            }
            // `qAuthChip`: the site needs a login before this source can run.
            if let Some(warning) =
                format::auth_warning(&query.site_id, &self.logins, &self.login_health)
            {
                name = name.child(
                    div()
                        .id(("subscription-query-auth", query_id))
                        .role(Role::Button)
                        .aria_label(warning.clone())
                        .flex_none()
                        .max_w(px(140.))
                        .truncate()
                        .cursor_pointer()
                        .text(theme::TEXT_SM)
                        .text_color(rgb(theme::FAILURE_TEXT))
                        .child(warning)
                        // `onOpenAuth`: the Accounts dialog on the service
                        // that owns this source's login.
                        .on_click({
                            let site = query.site_id.clone();
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.open_accounts(Some(&site), cx);
                            })
                        }),
                );
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
            let name = ui::tooltip::attach(
                name,
                ("subscription-query-tip", query_id),
                ui::tip(t("Double-click for source details")),
                17.,
                window,
                cx,
            );
            let paused = query.paused;
            let group_posts = query.group_posts;
            let edited = query.clone();
            // A double-click on the buttons is theirs, not the row's.
            let actions = div()
                .id(("subscription-query-actions", query_id))
                .a11y_presentational()
                .w_full()
                .flex()
                .justify_end()
                .gap(px(2.))
                .opacity(hover)
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    ui::small_button(("query-run", query_id), "player-play")
                        .disabled(waiting)
                        .tip(ui::tip(t("Run only this source now")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.run_query(id, cx)
                        })),
                )
                .child(
                    ui::small_button(("query-group", query_id), "copy-2")
                        .toggled(group_posts)
                        .disabled(waiting)
                        .tip(ui::tip(if group_posts {
                            t("Group multi-media posts")
                        } else {
                            t("Keep post media separate")
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.group_query(id, !group_posts, cx)
                        })),
                )
                .child(
                    ui::small_button(
                        ("query-hold", query_id),
                        if paused { "player-play" } else { "hand-stop" },
                    )
                    .disabled(waiting)
                    .tip(ui::tip(if paused {
                        t("Resume this source in future subscription runs")
                    } else {
                        t("Put this source on hold until it is resumed")
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.pause_query(id, !paused, cx)
                    })),
                )
                .child(
                    ui::small_button(("query-edit", query_id), "pencil")
                        .disabled(waiting)
                        .tip(ui::tip(t("Edit")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.open_edit_query(&edited, cx)
                        })),
                )
                .child(
                    ui::small_button(("query-delete", query_id), "trash-2")
                        .disabled(waiting)
                        .tip(ui::tip(t("Delete")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.confirm_delete_query(id, cx)
                        })),
                );
            let source = format::site_label(&query.site_id);
            let row = table_row(
                &widths,
                vec![
                    div()
                        .w_full()
                        .truncate()
                        .text_color(theme.text_secondary())
                        .child(source.clone())
                        .into_any_element(),
                    name.into_any_element(),
                    actions.into_any_element(),
                    number(query.posts, cx),
                    number(query.media, cx),
                ],
            );
            let details = query.clone();
            let opened = query.clone();
            let row_name = format!(
                "{source} {}",
                query.display_name.as_ref().unwrap_or(&query.text)
            );
            rows.push(
                self.body_row(("subscription-query", query_id).into(), row, cx)
                    // A row assistive technology presses opens its source's
                    // details, as a double-click does.
                    .role(Role::Row)
                    .aria_label(row_name)
                    .on_a11y_action(AccessibleAction::Click, {
                        let view = cx.entity().downgrade();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.open_details(&opened, cx));
                        }
                    })
                    .on_hover(move |hovered, _, cx| {
                        fade.update(cx, |fade, cx| {
                            fade.set(*hovered, motion::SMALL_BUTTON);
                            cx.notify();
                        })
                    })
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() == 2 {
                            this.open_details(&details, cx);
                        }
                    }))
                    .into_any_element(),
            );
        }
        let add = self.add_query_bar(cx).into_any_element();
        self.table("subscription-sources", header, rows, Some(add), window, cx)
    }

    /// History: the reused count gives way first when narrow, then the
    /// fetched count, then the notes.
    fn history(
        &self,
        detail: Option<&Detail>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let widths = fit(
            self.row_width(),
            &[
                column(Track::Fixed(160.), 0),
                column(Track::Fixed(96.), 0),
                column(Track::Fixed(64.), 2),
                column(Track::Fixed(64.), 1),
                column(Track::Flex(100., 1.), 3),
            ],
        );
        let header = table_row(
            &widths,
            vec![
                heading(t("Started"), false),
                heading(t("Status"), false),
                heading(t("Fetched"), true),
                heading(t("Reused"), true),
                heading(t("Notes"), false),
            ],
        );
        let rows = match detail {
            None => vec![table_note(t("Loading history…"), cx)],
            Some(detail) if detail.runs.is_empty() => vec![table_note(t("No runs yet."), cx)],
            Some(detail) => detail
                .runs
                .iter()
                .map(|run| {
                    let (tone, label) = format::run_status(run);
                    let row = table_row(
                        &widths,
                        vec![
                            div()
                                .w_full()
                                .truncate()
                                .font_features(theme::tabular_figures())
                                .child(format::date_time(Some(&run.started_at)))
                                .into_any_element(),
                            div()
                                .flex()
                                .child(
                                    ui::status_badge(("run-status", run.id as usize), tone, label)
                                        .tip(run.error_message.clone().map(ui::tip)),
                                )
                                .into_any_element(),
                            number(run.downloaded, cx),
                            number(run.already_in_library, cx),
                            quiet(run.error_message.clone().unwrap_or_default(), cx),
                        ],
                    );
                    self.body_row(("subscription-run", run.id as usize).into(), row, cx)
                        .into_any_element()
                })
                .collect(),
        };
        self.table("subscription-history", header, rows, None, window, cx)
    }

    /// Problems: when narrow, the time it was last seen gives way first,
    /// then the post; the state, why it failed and the fix stay.
    fn problems(
        &self,
        detail: Option<&Detail>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let widths = fit(
            self.row_width(),
            &[
                column(Track::Fixed(108.), 0),
                column(Track::Flex(100., 0.6), 2),
                column(Track::Flex(120., 2.), 0),
                column(Track::Fixed(88.), 1),
                column(Track::Fixed(28.), 0),
            ],
        );
        let header = table_row(
            &widths,
            vec![
                heading(t("Status"), false),
                heading(t("Post"), false),
                heading(t("Why it failed"), false),
                heading(t("Last seen"), false),
                div().into_any_element(),
            ],
        );
        let detail = match detail {
            None => {
                let rows = vec![table_note(t("Checking for problems…"), cx)];
                return self.table("subscription-problems", header, rows, None, window, cx);
            }
            Some(detail) if detail.issues.total == 0 => {
                let rows = vec![table_note(t("No problems found."), cx)];
                return self.table("subscription-problems", header, rows, None, window, cx);
            }
            Some(detail) => detail,
        };
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
                        .role(Role::Link)
                        .aria_label(target_label.clone())
                        .w_full()
                        .truncate()
                        .cursor_pointer()
                        .hover(move |style| style.text_color(theme.primary()).underline())
                        .child(target_label)
                        .on_click(move |_, _, cx| crate::desktop::open_link(&url, cx))
                        .into_any_element(),
                    None => div()
                        .w_full()
                        .truncate()
                        .child(target_label)
                        .into_any_element(),
                };
                // `recovery_action` is `fix_credentials` for a login
                // problem; production derives no other action a row shows
                // (`mapIssue`: Retry now and Review source never appear).
                // Fix login opens the Accounts dialog on the service of the
                // problem's source (none: the first service).
                let fix = (issue.kind.contains("auth")
                    || issue.kind.contains("credential")
                    || issue.kind == "unauthorized")
                    .then(|| {
                        let site = issue.query.and_then(|query| {
                            self.subscription(detail.subscription)?
                                .queries
                                .iter()
                                .find(|entry| entry.id == query)
                                .map(|entry| entry.site_id.clone())
                        });
                        ui::small_button(("issue-fix", issue.id as usize), "shield-lock")
                            .glyph(13.)
                            .disabled(self.busy())
                            .tip(ui::tip(t("Fix login")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_accounts(site.as_deref(), cx)
                            }))
                    });
                // The message's tooltip holds it with its detail
                // (`title={[message, detail].join('\n')}`).
                let full = [Some(issue.message.as_str()), issue.detail.as_deref()]
                    .into_iter()
                    .flatten()
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                let message = ui::tooltip::attach(
                    div()
                        .id(("issue-message", issue.id as usize))
                        .w_full()
                        .truncate()
                        .text_color(theme.text_secondary())
                        .child(issue.message.clone()),
                    ("issue-message-tip", issue.id as usize),
                    ui::tip(full),
                    20.,
                    window,
                    cx,
                );
                let row = table_row(
                    &widths,
                    vec![
                        div()
                            .flex()
                            .child(ui::status_badge(
                                ("issue-status", issue.id as usize),
                                Tone::Attention,
                                format::issue_label(&issue.kind),
                            ))
                            .into_any_element(),
                        target,
                        message.into_any_element(),
                        quiet(format::relative_time(Some(&issue.last_seen_at), now), cx),
                        div()
                            .w_full()
                            .flex()
                            .justify_end()
                            .children(fix)
                            .into_any_element(),
                    ],
                );
                self.body_row(("subscription-issue", issue.id as usize).into(), row, cx)
                    .into_any_element()
            })
            .collect();
        let more = detail.issues.next.is_some().then(|| {
            div()
                .h(px(ADD_ROW))
                .px(px(ROW_INSET))
                .flex()
                .items_center()
                .justify_end()
                .child(
                    ui::action_button("issues-more", Look::Standard)
                        .label(t("Load more"))
                        .height(theme::CONTROL_SM)
                        .padding(10.)
                        .disabled(self.loading_more)
                        .on_click(cx.listener(|this, _, _, cx| this.load_more_issues(cx))),
                )
                .into_any_element()
        });
        self.table("subscription-problems", header, rows, more, window, cx)
    }

    /// The add-query row under the sources (`AddQueryBar.tsx`): the source,
    /// its query, and Add, which Enter also presses.
    fn add_query_bar(&self, cx: &mut Context<Self>) -> Div {
        let sites = format::query_sites()
            .into_iter()
            .map(|site| ui::choice(site.id, site.name))
            .collect::<Vec<_>>();
        let typed = !self.add_query.read(cx).value().trim().is_empty();
        let view = cx.entity().downgrade();
        div()
            .h(px(ADD_ROW))
            .px(px(ROW_INSET))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                ui::select("add-query-site", self.add_site, sites)
                    .width(140.)
                    .on_change(move |site, _, cx| {
                        let _ = view.update(cx, |this, cx| this.choose_add_site(site, cx));
                    }),
            )
            .child(div().flex_1().min_w_0().child(ui::field(
                "add-query-text",
                &self.add_query,
                ui::field::Look::ModalSmall,
            )))
            .child(
                ui::action_button("add-query", Look::Standard)
                    .label(t("Add"))
                    .height(theme::CONTROL_SM)
                    .padding(12.)
                    .disabled(self.busy() || !typed)
                    .on_click(cx.listener(|this, _, _, cx| this.submit_query(cx))),
            )
    }
}
