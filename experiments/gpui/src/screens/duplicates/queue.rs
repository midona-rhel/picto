//! The queue down the left of the desk: the Identical and Similar tabs, then
//! the tab's sets, most alike first, each a row with a picture, the name
//! the set would end with and how many files it holds and how alike they
//! are, as the rows of a picker's list. Only the rows in view are drawn.
use super::{DuplicatesScreen, Tab};
use crate::grid::layout::Tile;
use crate::grid::prefs::ViewPrefs;
use crate::grid::tile::{TileLook, tile_art};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::query::RootSummary;
use picto_library::{Lifecycle, Rating, RootId, RootKind};

/// Eagle's merge editor's list, narrower so the stage keeps its room
/// beside the rail: 200 points wide, 56-point rows with 44-point pictures.
pub const WIDTH: f32 = 200.;
pub const ROW: f32 = 56.;
const THUMB: f32 = 44.;
/// The queue's width when it shows only its sets' pictures: when the
/// stage would be narrower than [`STAGE_MIN`] beside the full queue.
pub const NARROW: f32 = 64.;
pub const STAGE_MIN: f32 = 360.;
/// The list's padding above its first row.
const LIST_TOP: f32 = 4.;

/// Where the queue's row `at` starts in its list.
pub fn row_top(at: usize) -> f32 {
    LIST_TOP + at as f32 * ROW
}

impl DuplicatesScreen {
    /// The Identical and Similar tabs with their counts: over the queue,
    /// or at the head of the stage while the queue is narrow.
    pub(super) fn tabs(&self, screen: WeakEntity<Self>) -> ui::segmented::Segmented {
        let (identical, similar) = self.tab_counts();
        ui::segmented::segmented(
            "duplicates-tabs",
            ui::segmented::Look::FilledTabs,
            vec![
                ui::segmented::tab(format!("{} {identical}", en("Identical"))),
                ui::segmented::tab(format!("{} {similar}", en("Similar"))),
            ],
            Some(if self.tab == Tab::Identical { 0 } else { 1 }),
        )
        .name(t("Duplicates"))
        .on_select(move |index, _, cx| {
            let _ = screen.update(cx, |this, cx| {
                this.set_tab(
                    if index == 0 {
                        Tab::Identical
                    } else {
                        Tab::Similar
                    },
                    cx,
                )
            });
        })
    }

    /// Whether the queue shows only its sets' pictures, its tabs then at
    /// the head of the stage.
    pub(super) fn queue_narrow(&self) -> bool {
        self.width > 0. && self.width - WIDTH < STAGE_MIN
    }

    pub(super) fn queue(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let narrow = self.queue_narrow();
        let tabs = (!narrow).then(|| self.tabs(cx.entity().downgrade()));
        let top = -f32::from(self.queue_scroll.offset().y);
        let view = f32::from(self.queue_bounds.size.height);
        let first = ((top - LIST_TOP) / ROW).floor().max(0.) as usize;
        let last = (((top + view - LIST_TOP) / ROW).ceil() as usize).min(self.order.len());
        let mut list = div()
            .relative()
            .w_full()
            .h(px(row_top(self.order.len()) + LIST_TOP));
        for at in first..last {
            let row = self.queue_row(at, window, cx);
            list = list.child(row);
        }
        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.queue_bounds != bounds {
                        this.queue_bounds = bounds;
                        cx.notify();
                        window.request_animation_frame();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        // The library holds the first 500 pairs of each kind at a time: the
        // queue says so under its last row.
        let capped = self.capped.filter(|_| !narrow).map(|(shown, total)| {
            div()
                .flex_none()
                .px(px(12.))
                .py(px(8.))
                .border_t_1()
                .border_color(theme.border_secondary())
                .text(theme::TEXT_CAPTION)
                .text_color(theme.text_tertiary())
                .child(format!(
                    "{} {} {} {} {}",
                    en("Showing"),
                    crate::i18n::format::grouped(shown as u64),
                    en("of"),
                    crate::i18n::format::grouped(total),
                    en("pairs; settle these to see the rest.")
                ))
        });
        div()
            .w(px(if narrow { NARROW } else { WIDTH }))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme.border_secondary())
            .children(tabs.map(|tabs| div().flex_none().p(px(8.)).child(tabs)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(measure)
                    .child(
                        div()
                            .id("duplicate-queue")
                            .role(Role::List)
                            .aria_label(t("Duplicates"))
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.queue_scroll)
                            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                            .child(list),
                    )
                    .child(ui::scrollbar(
                        "duplicate-queue-scrollbar",
                        &self.queue_scroll,
                        window,
                        cx,
                    ))
                    .children((self.order.is_empty() && !narrow).then(|| {
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text(theme::TEXT_SM)
                            .text_color(theme.text_tertiary())
                            .child(match self.tab {
                                Tab::Identical => en("You have no identical items."),
                                Tab::Similar => en("You have no similar items."),
                            })
                    })),
            )
            .children(capped)
    }

    /// One set of the queue: its first file's picture, the name it would
    /// end with, and how many files there are and how alike.
    fn queue_row(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let index = self.order[at];
        let current = self.current_index() == Some(index);
        let entry = &self.entries[index];
        let side = &entry.set.files[0];
        let name =
            (1..entry.set.files.len()).fold(entry.facts[0].name(&entry.set, 0), |best, file| {
                picto_library::preferred_name(&best, &entry.facts[file].name(&entry.set, file))
                    .to_owned()
            });
        let thumbnail = self.session.read(cx).thumbnail(&side.file.file_hash);
        let resource = Resource::Path(thumbnail.clone().into());
        let thumb = self
            .thumbnails
            .read(cx)
            .thumb(&resource, &side.file.mime_type);
        let alike = super::compare::similarity_label(entry.set.distance());
        let detail = format!("{} {}, {alike}", entry.set.files.len(), en("files"));
        let look = TileLook {
            tile: Tile {
                x: 0.,
                y: 0.,
                w: THUMB,
                h: THUMB,
            },
            item: summary(side, &entry.facts[0], name.clone()),
            thumbnail,
            selected: false,
            thumb,
            original: None,
            alphas: (1., 0.),
            magnifier: false,
            grayscale: false,
        };
        let art = tile_art(look, &prefs(), &mut self.art, &self.thumbnails, window, cx);
        let key = entry.set.key;
        let narrow = self.queue_narrow();
        let row = div()
            .id(("duplicate-queue-row", key as usize))
            .role(Role::ListItem)
            .aria_selected(current)
            .aria_label(format!("{name}, {detail}"))
            .size_full()
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .when(current, |row| row.bg(theme.surface_active()))
            .when(!current, |row| {
                row.hover(|row| row.bg(theme.surface_hover()))
            })
            .child(div().relative().size(px(THUMB)).flex_none().child(art))
            .when(narrow, |row| row.justify_center().px_0())
            .when(!narrow, |row| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text(theme::TEXT_SM)
                                .text_color(theme.text())
                                .truncate()
                                .child(name.clone()),
                        )
                        .child(
                            div()
                                .text(theme::TEXT_CAPTION)
                                .text_color(theme.text_tertiary())
                                .truncate()
                                .child(detail.clone()),
                        ),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                let index = this.entry_index(key);
                this.show_set(index, cx);
            }));
        // Narrow, a row is its picture alone, and says what it is when the
        // pointer rests on it.
        let row = if narrow {
            ui::tooltip::attach(
                row,
                ("duplicate-queue-tip", key as usize),
                ui::tip(format!("{name}, {detail}")),
                ROW,
                window,
                cx,
            )
        } else {
            row
        };
        div()
            .absolute()
            .top(px(row_top(at)))
            .left(px(6.))
            .right(px(6. + ui::scrollbar::GUTTER))
            .h(px(ROW))
            .child(row)
    }
}

/// The grid's tile options for a small picture: fitted, with nothing under
/// it.
pub(super) fn prefs() -> ViewPrefs {
    ViewPrefs {
        mode: crate::grid::layout::ViewMode::Grid,
        show_name: false,
        show_resolution: false,
        show_extension_label: false,
        show_extension: false,
        show_item_count: false,
        fit_cover: false,
        ..ViewPrefs::default()
    }
}

/// The file as the grid's tile drawing takes an item.
pub(super) fn summary(
    side: &picto_library::DuplicateCandidateSide,
    facts: &super::Facts,
    name: String,
) -> RootSummary {
    let root = facts.details.as_ref().map(|details| &details.root);
    RootSummary {
        root_id: root.map_or(RootId(0), |root| root.root_id),
        kind: RootKind::Media,
        lifecycle: Lifecycle::Active,
        name,
        cover_media_id: root.map_or(picto_library::MediaId(0), |root| root.cover_media_id),
        content_hash: side.file.file_hash.clone(),
        mime: side.file.mime_type.clone(),
        width: side.file.pixel_width,
        height: side.file.pixel_height,
        duration_ms: None,
        frame_count: side.file.frame_count,
        palette: Vec::new(),
        imported_at_ms: root.map_or(0, |root| root.imported_at_ms),
        captured_at_ms: root.and_then(|root| root.captured_at_ms),
        modified_at_ms: root.map_or(0, |root| root.modified_at_ms),
        media_count: 1,
        total_size_bytes: side.file.size_bytes.max(0) as u64,
        rating: Rating::Unrated,
    }
}
