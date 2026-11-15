//! The list of sets: each set a cell with its header (the bulk merge's
//! check, what the set is, what merging saves, Merge or Compare, Not
//! duplicates) over its files, drawn as the grid draws its tiles, the file
//! kept carrying the grid's selection stroke and a check. Cells sit in
//! shelves of equal columns, as many as the width holds; a set of many
//! files takes several columns, and scrolls sideways when even the whole
//! width is too narrow for it. The footer totals the checked sets.
use super::sets::Verdict;
use super::{DuplicatesScreen, Entry, Tab};
use crate::grid::layout::{TEXT_ROW_HEIGHT, Tile};
use crate::grid::prefs::ViewPrefs;
use crate::grid::thumbnails::Thumb;
use crate::grid::tile::{TileLook, tile_art};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::query::RootSummary;
use picto_library::{Lifecycle, Rating, RootId, RootKind};

/// Eagle's tile: 160px wide, its picture fitted in a 160px square.
pub const TILE: f32 = 160.;
/// Between two tiles: room for both selection strokes.
pub const TILE_GAP: f32 = 8.;
/// A cell's padding left and right.
pub const PAD_X: f32 = 8.;
const PAD_TOP: f32 = 8.;
const HEADER: f32 = 24.;
const HEADER_GAP: f32 = 8.;
/// One caption row under a tile's name, as the grid's resolution row.
const CAPTION_ROW: f32 = 17.;
/// A tile's name and its two caption rows.
const TEXT_HEIGHT: f32 = TEXT_ROW_HEIGHT + 2. * CAPTION_ROW;
const PAD_BOTTOM: f32 = 12.;
/// A cell less its pictures, whose band is as tall as the tallest picture
/// of its shelf.
const CELL_CHROME: f32 = PAD_TOP + HEADER + HEADER_GAP + TEXT_HEIGHT + PAD_BOTTOM;
/// The narrowest cell: a pair of tiles.
const UNIT: f32 = 2. * TILE + TILE_GAP + 2. * PAD_X;
pub const FOOTER: f32 = 52.;

/// The width a set's tiles need.
fn needed(files: usize) -> f32 {
    files as f32 * TILE + files.saturating_sub(1) as f32 * TILE_GAP + 2. * PAD_X
}

/// The columns of the list at its width.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    cols: usize,
    cell: f32,
}

/// Where a set's cell is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub shelf: usize,
    pub col: usize,
    pub span: usize,
    pub x: f32,
    pub width: f32,
    pub top: f32,
    /// The shelf's picture band: its tallest picture.
    pub band: f32,
}

impl Place {
    pub fn height(&self) -> f32 {
        self.band + CELL_CHROME
    }
}

impl Layout {
    pub fn new(width: f32) -> Self {
        let cols = ((width / UNIT).floor() as usize).max(1);
        Self {
            cols,
            cell: (width / cols as f32).floor(),
        }
    }

    /// Each set's place, in order: a set starts a new shelf when it does
    /// not fit beside the ones before it, and the last cell of a shelf
    /// takes the shelf's remaining columns, so no shelf ends in a hole.
    /// Each set is its count of files and its tallest picture; a shelf is
    /// as tall as the tallest picture in it, so a shelf of wide pictures
    /// wastes no height on a square box.
    pub fn places(&self, sets: impl Iterator<Item = (usize, f32)>) -> Vec<Place> {
        let sets = sets.collect::<Vec<_>>();
        let spans = sets
            .iter()
            .map(|(files, _)| ((needed(*files) / self.cell).ceil() as usize).clamp(1, self.cols))
            .collect::<Vec<_>>();
        let mut places = Vec::with_capacity(spans.len());
        let (mut shelf, mut col) = (0, 0);
        for span in spans {
            if col + span > self.cols {
                if let Some(last) = places.last_mut() {
                    self.widen(last, self.cols);
                }
                shelf += 1;
                col = 0;
            }
            places.push(Place {
                shelf,
                col,
                span,
                x: 0.,
                width: 0.,
                top: 0.,
                band: 0.,
            });
            col += span;
        }
        if let Some(last) = places.last_mut() {
            // The last shelf keeps its columns: nothing follows to fill it.
            let span = last.span;
            self.widen(last, last.col + span);
        }
        for place in &mut places {
            if place.width == 0. {
                let span = place.span;
                self.widen(place, place.col + span);
            }
        }
        let shelves = Self::shelves(&places);
        let mut bands = vec![0f32; shelves];
        for (place, (_, tallest)) in places.iter().zip(&sets) {
            bands[place.shelf] = bands[place.shelf].max(*tallest);
        }
        let mut tops = Vec::with_capacity(shelves);
        let mut top = 0.;
        for band in &bands {
            tops.push(top);
            top += band + CELL_CHROME;
        }
        for place in &mut places {
            place.top = tops[place.shelf];
            place.band = bands[place.shelf];
        }
        places
    }

    fn widen(&self, place: &mut Place, to: usize) {
        place.span = to - place.col;
        place.x = place.col as f32 * self.cell;
        place.width = place.span as f32 * self.cell;
    }

    pub fn shelves(places: &[Place]) -> usize {
        places.last().map_or(0, |place| place.shelf + 1)
    }

    /// The height of every shelf together.
    pub fn height(places: &[Place]) -> f32 {
        places.last().map_or(0., |place| place.top + place.height())
    }

    /// The place of the set `key` among the tab's sets.
    pub fn place_of(&self, order: &[usize], entries: &[Entry], key: u32) -> Option<Place> {
        let at = order
            .iter()
            .position(|&index| entries[index].set.key == key)?;
        self.places(order.iter().map(|&index| shape(&entries[index])))
            .get(at)
            .copied()
    }

    /// The set in the shelf above (`delta` -1) or below (1) the set `key`,
    /// under the middle of its cell, as a position in `order`.
    pub fn vertical(
        &self,
        order: &[usize],
        entries: &[Entry],
        key: u32,
        delta: isize,
    ) -> Option<usize> {
        let at = order
            .iter()
            .position(|&index| entries[index].set.key == key)?;
        let places = self.places(order.iter().map(|&index| shape(&entries[index])));
        let here = places[at];
        let shelf = here.shelf.checked_add_signed(delta)?;
        let middle = here.x + here.width / 2.;
        places
            .iter()
            .enumerate()
            .filter(|(_, place)| place.shelf == shelf)
            .min_by(|(_, a), (_, b)| {
                let distance = |place: &Place| {
                    if (place.x..place.x + place.width).contains(&middle) {
                        0.
                    } else {
                        (place.x + place.width / 2. - middle).abs()
                    }
                };
                distance(a).total_cmp(&distance(b))
            })
            .map(|(position, _)| position)
    }
}

impl DuplicatesScreen {
    pub(super) fn layout(&self) -> Layout {
        Layout::new((f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(UNIT))
    }

    fn places(&self, layout: &Layout) -> Vec<Place> {
        layout.places(self.order.iter().map(|&index| shape(&self.entries[index])))
    }

    /// The list: the tab's sets in shelves, only the shelves in view drawn.
    pub(super) fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let layout = self.layout();
        let places = self.places(&layout);
        let top = -f32::from(self.scroll.offset().y);
        let view = f32::from(self.bounds.size.height);
        let mut content = div().relative().w_full().h(px(Layout::height(&places)));
        let focused = self.focus.is_focused(window);
        for (position, place) in places.iter().enumerate() {
            if place.top + place.height() < top || place.top > top + view {
                continue;
            }
            let index = self.order[position];
            content = content.child(self.cell(index, *place, &layout, focused, window, cx));
        }
        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.bounds != bounds {
                        this.bounds = bounds;
                        cx.notify();
                        window.request_animation_frame();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let empty = self.order.is_empty();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(measure)
            .child(
                div()
                    .id("duplicate-sets")
                    .role(Role::List)
                    .aria_label(t("Duplicates"))
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .on_scroll(cx.listener(|this, _, _, cx| {
                        this.preview.close(cx);
                        cx.notify();
                    }))
                    .child(content),
            )
            .children(empty.then(|| self.tab_empty(cx)))
            .child(ui::scrollbar(
                "duplicate-sets-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
            .children(self.scan_progress.then(|| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .child(ui::progress_bar().height(2.))
            }))
            .bg(theme.background())
            .into_any_element()
    }

    /// A tab whose sets are all settled, or all below the similarity.
    fn tab_empty(&self, cx: &App) -> Div {
        let description = match self.tab {
            Tab::Identical => en("You have no identical items."),
            Tab::Similar => en("You have no similar items at this similarity."),
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(ui::empty_state(
                "empty-copy",
                en("Enjoy your day"),
                description,
                Vec::new(),
                cx,
            ))
    }

    /// One set's cell.
    fn cell(
        &mut self,
        index: usize,
        place: Place,
        layout: &Layout,
        focused: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let key = self.entries[index].set.key;
        let cursor = self
            .cursor
            .filter(|(set, _)| *set == key)
            .map(|(_, file)| file);
        let ends_row = place.col + place.span >= layout.cols;
        let header = self.header(index, place.width, window, cx);
        let tiles = self.tiles(index, place, cursor.filter(|_| focused), window, cx);
        div()
            .id(("duplicate-set", key as usize))
            .role(Role::ListItem)
            .absolute()
            .left(px(place.x))
            .top(px(place.top))
            .w(px(place.width))
            .h(px(place.height()))
            .border_b_1()
            .when(!ends_row, |cell| cell.border_r_1())
            .border_color(theme.border_secondary())
            .when(cursor.is_some(), |cell| cell.bg(theme.footer_surface()))
            .child(header)
            .child(tiles)
    }

    /// The set's header line.
    fn header(&self, index: usize, width: f32, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let Entry { set, .. } = &self.entries[index];
        let key = set.key;
        let checked = self.checked(index);
        let unsure = set.verdict == Verdict::Unsure;
        let (noun, phrase) = match set.verdict {
            Verdict::Identical => (en("identical files"), None),
            Verdict::Clear => (en("similar files"), Some(en("the best one is chosen"))),
            Verdict::Unsure => (en("similar files"), Some(en("look before merging"))),
        };
        let figure = if unsure {
            super::compare::similarity_label(set.distance())
        } else {
            format!(
                "{} {}",
                en("saves"),
                crate::text::size_to_megabytes(set.saving(self.keeper(index)))
            )
        };
        // The phrase goes first when the line is short of room; the count,
        // the figure and the actions always show.
        let font = font(theme::FONT_UI);
        let measure = |text: &str| ui::text_width(text, font.clone(), theme::TEXT_SM, window);
        let action_label = if unsure { en("Compare") } else { en("Merge") };
        let fixed = 14.
            + 8.
            + measure(&format!("{} {noun}", set.files.len()))
            + 12.
            + measure(&figure)
            + 8.
            + measure(action_label)
            + 16.
            + 4.
            + 24.
            + 2. * PAD_X;
        let phrase = phrase.filter(|phrase| fixed + measure(&format!(", {phrase}")) <= width);
        let check = div()
            .id(("duplicate-check", key as usize))
            .role(Role::CheckBox)
            .aria_label(en("Include in merge"))
            .aria_toggled(ui::a11y::toggled(checked))
            .flex_none()
            .cursor_pointer()
            .child(ui::check::check_box(
                if checked {
                    ui::check::Mark::Selected
                } else {
                    ui::check::Mark::Empty
                },
                ui::check::Size::Compact,
            ))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_check(key, cx)));
        let title = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .text(theme::TEXT_SM)
            .child(
                div()
                    .flex_none()
                    .text_color(theme.primary())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(set.files.len().to_string()),
            )
            .child(
                div()
                    .flex_none()
                    .ml(px(4.))
                    .text_color(theme.text())
                    .child(noun),
            )
            .children(phrase.map(|phrase| {
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.text_tertiary())
                    .child(format!(", {phrase}"))
            }));
        let action = ui::action_button(("duplicate-action", key as usize), Look::Ghost)
            .label(action_label)
            .height(theme::CONTROL_SM - 2.)
            .padding(8.)
            .text_size(theme::TEXT_SM)
            .disabled(self.busy)
            .tip(if unsure {
                ui::tip(en("Compare the files one at a time")).shortcut("dup.compare", cx)
            } else {
                ui::tip(en("Keep the marked file and merge the rest into it"))
                    .shortcut("dup.merge", cx)
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if unsure {
                    let shown = this.entry_index(key).map_or(0, |index| this.keeper(index));
                    this.open_compare(key, shown, cx);
                } else {
                    this.merge_set(key, cx);
                }
            }));
        let dismiss = ui::small_button(("duplicate-dismiss", key as usize), "action-x")
            .tip(ui::tip(t("Not duplicates")).shortcut("dup.notDuplicate", cx))
            .disabled(self.busy)
            .on_click(cx.listener(move |this, _, _, cx| this.keep_apart(key, cx)));
        div()
            .absolute()
            .top(px(PAD_TOP))
            .left(px(PAD_X))
            .right(px(PAD_X - 4.))
            .h(px(HEADER))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(check)
            .child(title)
            .child(
                div()
                    .flex_none()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .whitespace_nowrap()
                    .child(figure),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(action)
                    .child(dismiss),
            )
    }

    /// The set's files, scrolling sideways when they outgrow the cell.
    fn tiles(
        &mut self,
        index: usize,
        place: Place,
        cursor: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = self.entries[index].set.key;
        let count = self.entries[index].set.files.len();
        let overflow = needed(count) > place.width;
        let keeper = self.keeper(index);
        let differ = Differences::of(&self.entries[index]);
        let band = place.band;
        let mut row = div().flex().gap(px(TILE_GAP)).h(px(band + TEXT_HEIGHT));
        for file in 0..count {
            row = row.child(self.tile(
                index,
                file,
                band,
                file == keeper,
                cursor == Some(file),
                &differ,
                window,
                cx,
            ));
        }
        let area = div()
            .absolute()
            .top(px(PAD_TOP + HEADER + HEADER_GAP))
            .left(px(PAD_X))
            .right(px(PAD_X))
            .h(px(band + TEXT_HEIGHT + 4.));
        if !overflow {
            return area.child(row).into_any_element();
        }
        let handle = self.rows.entry(key).or_default().clone();
        area.child(
            div()
                .id(("duplicate-row", key as usize))
                .size_full()
                .overflow_x_scroll()
                .track_scroll(&handle)
                .on_scroll(cx.listener(|this, _, _, cx| {
                    this.preview.close(cx);
                    cx.notify();
                }))
                .child(row),
        )
        .into_any_element()
    }

    /// One file: the grid's tile, its name, and two caption rows whose
    /// facts are brighter where the set's files differ.
    #[allow(clippy::too_many_arguments)]
    fn tile(
        &mut self,
        index: usize,
        file: usize,
        band: f32,
        kept: bool,
        cursor: bool,
        differ: &Differences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let entry = &self.entries[index];
        let key = entry.set.key;
        let side = &entry.set.files[file];
        let facts = &entry.facts[file];
        let name = facts.name(&entry.set, file);
        let thumbnail = self.session.read(cx).thumbnail(&side.file.file_hash);
        let resource = Resource::Path(thumbnail.clone().into());
        let thumb = self
            .thumbnails
            .read(cx)
            .thumb(&resource, &side.file.mime_type);
        let item = summary(side, facts, name);
        let media = media_rect(&side.file, band);
        // Pictures stand on the band's floor, so every name of a shelf is
        // on one line.
        let look = TileLook {
            tile: Tile {
                x: 0.,
                y: media.1,
                w: TILE,
                h: media.3 + TEXT_ROW_HEIGHT,
            },
            item,
            thumbnail,
            selected: kept,
            thumb,
            original: None,
            // The files not kept are dimmed.
            alphas: if kept {
                (1., (thumb == Thumb::Pending) as u8 as f32)
            } else {
                (0.45, 0.)
            },
            magnifier: false,
            grayscale: false,
        };
        let art = tile_art(
            look,
            &tile_prefs(),
            &mut self.art,
            &self.thumbnails,
            window,
            cx,
        );
        let badge = kept.then(|| {
            div()
                .absolute()
                .left(px(media.0 + media.2 - 18. - 5.))
                .top(px(media.1 + 5.))
                .size(px(18.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(theme::GRID_SELECTION))
                .child(ui::icon("check", 12., rgb(theme::BADGE_STRONG_TEXT)))
        });
        let ring = cursor.then(|| {
            let extent = 6.;
            div()
                .absolute()
                .left(px(media.0 - extent))
                .top(px(media.1 - extent))
                .w(px(media.2 + 2. * extent))
                .h(px(media.3 + 2. * extent))
                .rounded(px(theme::TILE_RADIUS + extent))
                .border_1()
                .border_color(theme.border_focus())
        });
        let span = |text: String, bright: bool| {
            div()
                .text_color(if bright {
                    theme.text()
                } else {
                    theme.text_tertiary()
                })
                .child(text)
        };
        let dot = || div().text_color(theme.text_tertiary()).child("·");
        let caption = |top: f32| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(top))
                .h(px(15.))
                .flex()
                .justify_center()
                .gap(px(4.))
                .whitespace_nowrap()
                .text(theme::TEXT_CAPTION)
                .line_height(px(15.))
        };
        let size = crate::text::size_to_megabytes(side.file.size_bytes);
        let format = crate::grid::format::format_label(&side.file.mime_type);
        let first = caption(band + TEXT_ROW_HEIGHT + 2.)
            .child(span(format, differ.format))
            .child(dot())
            .child(span(size, differ.size));
        let mut second = caption(band + TEXT_ROW_HEIGHT + 2. + CAPTION_ROW).child(span(
            super::compare::dimensions(side.file.pixel_width, side.file.pixel_height),
            differ.dimensions,
        ));
        if let Some(tags) = facts.tags() {
            second = second.child(dot()).child(span(
                if tags == 1 {
                    en("1 tag").to_owned()
                } else {
                    format!("{tags} {}", en("tags"))
                },
                differ.tags,
            ));
        }
        let hash = side.file.file_hash.clone();
        let path = facts
            .media(&side.file.file_hash)
            .filter(|media| media.facts.content_hash == side.file.file_hash)
            .map(|media| std::path::PathBuf::from(&media.file_path));
        let previewable = side.file.mime_type.starts_with("image/");
        div()
            .id(("duplicate-file", side.file.file_id.0 as usize))
            .relative()
            .flex_none()
            .w(px(TILE))
            .h(px(band + TEXT_HEIGHT))
            .child(art)
            .children(badge)
            .children(ring)
            .child(first)
            .child(second)
            // The picture takes the pointer, as the grid's tile does.
            .child(
                div()
                    .id(("duplicate-file-hit", side.file.file_id.0 as usize))
                    .role(Role::Button)
                    .aria_label(name_for_a11y(&self.entries[index], file, kept))
                    .absolute()
                    .left(px(media.0))
                    .top(px(media.1))
                    .w(px(media.2))
                    .h(px(media.3))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.focus, cx);
                            this.preview.close(cx);
                            if event.click_count >= 2 {
                                this.open_compare(key, file, cx);
                            } else {
                                this.choose_keeper(key, file, cx);
                            }
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.preview.close(cx);
                            this.open_tile_menu(key, file, event.position, window, cx);
                        }),
                    )
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && previewable {
                            let path = path.clone();
                            this.preview.rest_on(
                                Some(hash.clone()),
                                move |_| async move { path },
                                |this: &mut Self| &mut this.preview,
                                cx,
                            );
                        } else {
                            this.preview.away(|this: &mut Self| &mut this.preview, cx);
                        }
                    })),
            )
    }

    /// A tile's menu: the app's rows for opening, revealing and copying
    /// the file, then taking it out of its set.
    fn open_tile_menu(
        &mut self,
        key: u32,
        file: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::grid::menu as rows;
        let Some(entry) = self.entry(key) else {
            return;
        };
        let side = &entry.set.files[file];
        let description = rows::Description {
            single_hash: Some(side.file.file_hash.clone()),
            handlers: rows::Handlers {
                open_default: true,
                reveal: true,
                open_new_window: true,
                copy_path: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let mut built = rows::open_menu(&description, &hints);
        built.push(rows::Row::Separator);
        built.extend(rows::copy_menu(&description, &hints));
        let mut entries = rows::entries(built, &mut |_| ui::menu::Entry::Separator);
        if entry.set.files.len() > 2 {
            entries.push(ui::menu::Entry::Separator);
            entries.push(
                ui::menu::item(REMOVE_FROM_SET, en("Remove from This Set"))
                    .icon("menu-x")
                    .into(),
            );
        }
        let menu = cx.new(|cx| ui::menu::Menu::new(entries, position, cx).search(false));
        self.menu
            .open(menu, window, cx, move |this, _, chosen, _, cx| {
                let Some(entry) = this.entry(key) else {
                    return;
                };
                let side = &entry.set.files[file];
                let facts = &entry.facts[file];
                let path = facts
                    .media(&side.file.file_hash)
                    .map(|media| std::path::PathBuf::from(&media.file_path));
                if chosen == REMOVE_FROM_SET {
                    this.remove_file(key, file, cx);
                } else if chosen == rows::Action::OpenDefault.key() {
                    if let Some(path) = path {
                        crate::desktop::open_default(&path, cx);
                    }
                } else if chosen == rows::Action::Reveal.key() {
                    if let Some(path) = path {
                        crate::desktop::reveal(&path, cx);
                    }
                } else if chosen == rows::Action::CopyPath.key() {
                    if let Some(path) = path
                        && let Err(error) = crate::desktop::copy_paths(&[path], cx)
                    {
                        ui::notify::error(en("Could not copy file path"), error, cx);
                    }
                } else if chosen == rows::Action::OpenInNewWindow.key() {
                    let Some(details) = &facts.details else {
                        return;
                    };
                    let detail = crate::viewer::detail::Entry {
                        root: details.root.root_id,
                        hash: side.file.file_hash.clone(),
                        name: facts.name(&entry.set, file),
                        width: side.file.pixel_width,
                        height: side.file.pixel_height,
                        collection: false,
                    };
                    crate::viewer::detail::open(this.session.clone(), vec![detail], 0, cx);
                }
            });
    }

    /// The footer: what the checked sets come to, the Similar tab's
    /// similarity, and the bulk merge. Nothing in it wraps: the note on
    /// sets still to come goes first, then the similarity's label, then
    /// the slider, and the sentence truncates last.
    pub(super) fn footer(&self, _: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let (chosen, saving) = self.selected();
        let width = f32::from(self.bounds.size.width);
        let needs_check = chosen
            .iter()
            .any(|&index| super::verify::differs(&self.entries[index]));
        let label = if chosen.is_empty() {
            en("Merge").to_owned()
        } else if needs_check {
            format!("{} ({})", t("Next"), chosen.len())
        } else {
            format!("{} ({})", en("Merge"), chosen.len())
        };
        let button = ui::action_button("duplicates-merge-checked", Look::Primary)
            .icon("action-arrows-join", 16.)
            .label(label)
            .disabled(chosen.is_empty() || self.busy)
            .tip(ui::tip(en(
                "Merge every checked set, keeping each marked file",
            )))
            .on_click(cx.listener(|this, _, _, cx| this.next_or_merge(cx)));
        let strong = |text: String| div().text_color(theme.text()).child(text);
        let quiet = |text: &str| {
            div()
                .text_color(theme.text_secondary())
                .child(text.to_owned())
        };
        let sentence = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text(theme::TEXT_SM)
            .child(quiet(t("Selected")))
            .child(strong(crate::i18n::format::grouped(chosen.len() as u64)))
            .child(quiet(if chosen.len() == 1 {
                en("set, merging saves")
            } else {
                en("sets, merging saves")
            }))
            .child(strong(format!(
                "{}.",
                crate::text::size_to_megabytes(saving)
            )))
            .children((self.capped && width > 1100.).then(|| {
                div()
                    .ml(px(8.))
                    .text_color(theme.text_tertiary())
                    .truncate()
                    .child(en("More sets follow once these are settled."))
            }));
        let similar = self.tab == Tab::Similar;
        let entity = cx.entity().downgrade();
        let slider = (similar && width > 640.).then(|| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(10.))
                .text(theme::TEXT_SM)
                .children((width > 760.).then(|| {
                    div()
                        .text_color(theme.text_secondary())
                        .child(en("Similarity"))
                }))
                .child(
                    ui::slider(
                        "duplicates-similarity",
                        self.similarity as f64,
                        super::SIMILARITY_MIN as f64,
                        99.,
                        1.,
                    )
                    .look(ui::slider::Look::Filled)
                    .width(120.)
                    .name(en("Similarity"))
                    .on_change(move |value, cx| {
                        let _ = entity.update(cx, |this, cx| this.set_similarity(value as u32, cx));
                    }),
                )
                .child(
                    div()
                        .w(px(32.))
                        .text_color(theme.text())
                        .font_features(theme::tabular_figures())
                        .child(format!("{}%", self.similarity)),
                )
        });
        div()
            .h(px(FOOTER))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(16.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .bg(theme.footer_surface())
            .child(sentence)
            .children(slider)
            .child(button)
    }
}

const REMOVE_FROM_SET: &str = "duplicates-remove-from-set";

/// Which facts differ between a set's files.
struct Differences {
    format: bool,
    size: bool,
    dimensions: bool,
    tags: bool,
}

impl Differences {
    fn of(entry: &Entry) -> Self {
        let files = &entry.set.files;
        let varies = |value: &dyn Fn(usize) -> String| {
            (1..files.len()).any(|index| value(index) != value(0))
        };
        Self {
            format: varies(&|index| files[index].file.mime_type.clone()),
            size: varies(&|index| crate::text::size_to_megabytes(files[index].file.size_bytes)),
            dimensions: varies(&|index| {
                format!(
                    "{:?}{:?}",
                    files[index].file.pixel_width, files[index].file.pixel_height
                )
            }),
            tags: varies(&|index| format!("{:?}", entry.facts[index].tags())),
        }
    }
}

/// The grid's tile options for a duplicate's tile: the picture fitted in
/// its square, with its name under it.
fn tile_prefs() -> ViewPrefs {
    ViewPrefs {
        mode: crate::grid::layout::ViewMode::Grid,
        show_name: true,
        show_resolution: false,
        show_extension_label: false,
        show_extension: false,
        show_item_count: false,
        fit_cover: false,
        ..ViewPrefs::default()
    }
}

/// Where the picture sits: fitted in a square as the grid fits it, and
/// standing on the floor of a band `band` tall.
fn media_rect(file: &picto_library::DuplicateFile, band: f32) -> (f32, f32, f32, f32) {
    let (x, _, width, height) = match (file.pixel_width, file.pixel_height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            crate::viewer::camera::contain((width as f32, height as f32), (TILE, TILE))
                .unwrap_or((0., 0., TILE, TILE))
        }
        _ => (0., 0., TILE, TILE),
    };
    (x, band - height, width, height)
}

/// A set as the shelves lay it out: its count of files and its tallest
/// picture.
fn shape(entry: &Entry) -> (usize, f32) {
    let tallest = entry
        .set
        .files
        .iter()
        .map(|side| media_rect(&side.file, 0.).3)
        .fold(0., f32::max);
    (entry.set.files.len(), tallest)
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

/// A tile as assistive technology reads it.
fn name_for_a11y(entry: &Entry, file: usize, kept: bool) -> String {
    let name = entry.facts[file].name(&entry.set, file);
    if kept {
        format!("{name}, {}", en("kept"))
    } else {
        name
    }
}
