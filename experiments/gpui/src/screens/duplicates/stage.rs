//! The desk: one set at a time, to be decided from the keyboard. Its files
//! flip in place with ← → while the zoom and the place in the picture stay,
//! so what differs blinks; ↑ ↓ go to the set before and after. Over the
//! picture, the Identical and Similar tabs and which set of the tab this
//! is. Under it, centred, the set's files, each with the facts a choice
//! turns on, the shown one being the file a merge keeps; then the
//! decisions, the difference button (the pixel difference of the shown
//! file and its partner while the pointer rests on it or D is held, as the
//! Electron screen's Show Difference does) and what comparing their pixels
//! found, in words.
use super::compare::{Extent, Mode as ZoomMode};
use super::difference::Measure;
use super::{DuplicatesScreen, Tab};
use crate::grid::layout::Tile;
use crate::grid::prefs::ViewPrefs;
use crate::grid::tile::{TileLook, tile_art};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::{theme, ui};
use gpui::*;
use picto_library::query::RootSummary;
use picto_library::{Lifecycle, Rating, RootId, RootKind};

/// The row of the set's files under the picture.
const STRIP: f32 = 68.;
const STRIP_TILE: f32 = 44.;
/// A file of the strip with its facts beside its picture, and the least
/// room each needs before the strip shows pictures alone.
const CARD: f32 = 232.;
/// The row over the picture: the tabs and the place in the tab.
const HEADER: f32 = 44.;
pub const FOOTER: f32 = 52.;
const REMOVE_FROM_SET: &str = "duplicates-remove-from-set";

/// A share of the picture, as the sentence writes it.
fn share(value: f32) -> String {
    let percent = value * 100.;
    if percent > 0. && percent < 0.1 {
        "<0.1%".to_owned()
    } else if percent >= 99.95 {
        "100%".to_owned()
    } else {
        format!("{percent:.1}%")
    }
}

/// What comparing the two files' pixels found, in words: the first part
/// is the verdict, the second the detail.
fn sentence(measure: &Measure) -> (String, Option<String>) {
    let verdict = if measure.changed == 0. {
        en("Pixels identical").to_owned()
    } else if measure.visible == 0. {
        format!(
            "{} {}",
            share(measure.changed),
            en("of pixels differ slightly")
        )
    } else {
        format!("{} {}", share(measure.visible), en("differs visibly"))
    };
    let mut detail = Vec::new();
    if measure.visible > 0. && measure.changed > measure.visible {
        detail.push(format!(
            "{} {}",
            share(measure.changed),
            en("differ at all")
        ));
    }
    if let Some(((aw, ah), (bw, bh))) = measure.sizes {
        detail.push(format!(
            "{} {aw} × {ah} {} {bw} × {bh}, {}",
            en("sizes differ,"),
            t("and"),
            en("compared stretched")
        ));
    }
    (verdict, (!detail.is_empty()).then(|| detail.join(" · ")))
}

impl DuplicatesScreen {
    /// The desk: the row over the picture, the picture, the set's files
    /// and the decisions.
    pub(super) fn stage(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let header = self.stage_header(window, cx);
        let column = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header);
        let Some(index) = self.current_index() else {
            // A tab with nothing in it says so where its sets would be.
            let theme = theme::theme(cx);
            return column
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text(theme::TEXT_SM)
                        .text_color(theme.text_tertiary())
                        .child(match self.tab {
                            Tab::Identical => en("You have no identical items."),
                            Tab::Similar => en("You have no similar items."),
                        }),
                )
                .into_any_element();
        };
        let picture = self.picture(index, cx);
        let strip = self.strip(index, window, cx);
        let footer = self.footer(index, cx);
        column
            .child(picture)
            .child(strip)
            .child(footer)
            .into_any_element()
    }

    /// Across a narrow desk the tabs drop their counts.
    fn tabs_counted(&self) -> bool {
        let width = f32::from(self.stage.pane.size.width);
        width <= 0. || width >= 420.
    }

    /// The Identical and Similar tabs with their counts.
    fn tabs(&self, screen: WeakEntity<Self>) -> ui::segmented::Segmented {
        let (identical, similar) = self.tab_counts();
        let counted = self.tabs_counted();
        let label = |name: &'static str, count: usize| {
            if counted {
                format!("{name} {count}")
            } else {
                name.to_owned()
            }
        };
        ui::segmented::segmented(
            "duplicates-tabs",
            ui::segmented::Look::FilledTabs,
            vec![
                ui::segmented::tab(label(en("Identical"), identical)),
                ui::segmented::tab(label(en("Similar"), similar)),
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

    /// The tabs, and at the right which set of the tab is on the desk,
    /// between the buttons ↑ and ↓ press.
    fn stage_header(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let counted = self.tabs_counted();
        let at = self
            .current_index()
            .and_then(|index| self.order.iter().position(|&each| each == index));
        let count = self.order.len();
        let place = at.map(|at| {
            let text = div()
                .id("duplicates-place")
                .flex_none()
                .text(theme::TEXT_SM)
                .text_color(theme.text_secondary())
                .child(format!(
                    "{} {} {}",
                    crate::i18n::format::grouped(at as u64 + 1),
                    en("of"),
                    crate::i18n::format::grouped(count as u64)
                ));
            // The library hands out so many pairs at a time; when it has
            // more, the place says so under the pointer.
            match self.capped {
                Some((shown, total)) => ui::tooltip::attach(
                    text,
                    "duplicates-place-tip",
                    ui::tip(format!(
                        "{} {} {} {} {}",
                        en("Showing"),
                        crate::i18n::format::grouped(shown as u64),
                        en("of"),
                        crate::i18n::format::grouped(total),
                        en("pairs; settle these to see the rest.")
                    )),
                    20.,
                    window,
                    cx,
                ),
                None => text,
            }
        });
        let step = |id: &'static str,
                    icon: &'static str,
                    label: &'static str,
                    key: &'static str,
                    delta: isize,
                    enabled: bool| {
            ui::icon_button(id, icon)
                .tip(ui::tip(label).shortcut(key, cx))
                .disabled(!enabled)
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.focus, cx);
                    this.step_set(delta, cx);
                }))
        };
        div()
            .h(px(HEADER))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .border_b_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .flex_none()
                    .w(px(if counted { 216. } else { 150. }))
                    .child(self.tabs(cx.entity().downgrade())),
            )
            .child(div().flex_1())
            // How alike the scan found the set's files, least alike pair.
            .children(self.current_index().map(|index| {
                div()
                    .flex_none()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(super::compare::similarity_label(
                        self.entries[index].set.distance(),
                    ))
            }))
            .children(place)
            .children(at.map(|at| {
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(step(
                        "duplicates-previous-set",
                        "chevron-up",
                        en("Previous set"),
                        "dup.previousSet",
                        -1,
                        at > 0,
                    ))
                    .child(step(
                        "duplicates-next-set",
                        "chevron-down",
                        en("Next set"),
                        "dup.nextSet",
                        1,
                        at + 1 < count,
                    ))
            }))
    }

    /// The shown file, zoomed and placed as the zoom says, or its
    /// difference from its partner while the difference is asked for.
    fn picture(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let entry = &self.entries[index];
        let side = &entry.set.files[self.shown];
        let id = side.file.file_id.0;
        let entity = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                let changed = entity
                    .update(cx, |this, _| {
                        std::mem::replace(&mut this.stage.pane, bounds) != bounds
                    })
                    .unwrap_or(false);
                if changed {
                    let entity = entity.clone();
                    window.on_next_frame(move |_, cx| {
                        let _ = entity.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let pane = super::pane_extent(self.stage.pane);
        let image = Extent::new(
            side.file.pixel_width.unwrap_or(0) as f32,
            side.file.pixel_height.unwrap_or(0) as f32,
        );
        let mut layers =
            if pane.width > 0. && pane.height > 0. && image.width > 0. && image.height > 0. {
                let frame = self.stage.zoom.frame(pane, image);
                div()
                    .absolute()
                    .left(px(frame.left))
                    .top(px(frame.top))
                    .w(px(frame.width))
                    .h(px(frame.height))
            } else {
                div().absolute().inset_0()
            };
        let fill = |image: std::sync::Arc<RenderImage>| img(image).absolute().inset_0().size_full();
        let difference = self
            .stage
            .difference
            .as_ref()
            .filter(|((shown, _), _, _)| *shown == id)
            .map(|(_, image, _)| image.clone());
        if self.difference_shown() {
            // `.differenceComposite`: on black, the difference stretched
            // over the shown file's place.
            layers = layers
                .bg(rgb(theme::DIFFERENCE_BACKGROUND))
                .children(difference.map(|image| fill(image).object_fit(ObjectFit::Fill)));
        } else {
            let thumbnail = self.session.read(cx).thumbnail(&side.file.file_hash);
            layers = layers.child(
                img(thumbnail)
                    .image_cache(&self.thumbnails)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            );
            if let Some(full) = self.stage.originals.get(&id).cloned() {
                layers = layers.child(fill(full).object_fit(ObjectFit::Contain));
            }
        }
        let dragging = self.stage.drag.is_some();
        div()
            .id("duplicate-picture")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(theme.background())
            .cursor(if dragging {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .child(measure)
            .child(layers)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if event.click_count == 2 {
                        this.stage.drag = None;
                        this.fit(cx);
                    } else {
                        this.stage.drag = Some((event.position, this.stage.zoom.center()));
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                this.wheel(event, window, cx);
                cx.stop_propagation();
            }))
    }

    /// Moves the picture with a drag from the window's pointer events.
    pub(super) fn dragged(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some((from, center)) = self.stage.drag else {
            return;
        };
        let Some(image) = self.shown_extent() else {
            return;
        };
        let moved = position - from;
        self.stage.zoom.pan(
            super::pane_extent(self.stage.pane),
            image,
            center,
            (f32::from(moved.x), f32::from(moved.y)),
        );
        cx.notify();
    }

    /// The set's files, centred and numbered, each with what a choice
    /// turns on beside its picture: its size in pixels, its format and its
    /// bytes, bright where the files differ in it, and the library's
    /// suggestion. The shown one, which a merge keeps, carries the grid's
    /// selection stroke, and in a set of more than two the one it is
    /// measured against a quiet ring. When they do not all fit with their
    /// facts, the pictures stand alone.
    fn strip(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let count = self.entries[index].set.files.len();
        let partner = self.partner();
        let width = f32::from(self.stage.pane.size.width);
        let facts = width <= 0. || count as f32 * CARD + 32. <= width;
        // Which of the facts differ between the set's files.
        let (sizes_differ, formats_differ, bytes_differ) = {
            let files = &self.entries[index].set.files;
            let differ = |same: &dyn Fn(usize, usize) -> bool| (1..count).any(|at| !same(0, at));
            (
                differ(&|a, b| {
                    (files[a].file.pixel_width, files[a].file.pixel_height)
                        == (files[b].file.pixel_width, files[b].file.pixel_height)
                }),
                differ(&|a, b| files[a].file.mime_type == files[b].file.mime_type),
                differ(&|a, b| files[a].file.size_bytes == files[b].file.size_bytes),
            )
        };
        let mut row = div()
            .id("duplicate-strip")
            .flex()
            .items_center()
            .gap(px(if facts { 20. } else { 12. }))
            .h_full()
            .max_w_full()
            .overflow_x_scroll();
        for file in 0..count {
            let entry = &self.entries[index];
            let side = &entry.set.files[file];
            let thumbnail = self.session.read(cx).thumbnail(&side.file.file_hash);
            let resource = Resource::Path(thumbnail.clone().into());
            let thumb = self
                .thumbnails
                .read(cx)
                .thumb(&resource, &side.file.mime_type);
            let name = entry.facts[file].name(&entry.set, file);
            let shown = file == self.shown;
            let suggested = entry.set.suggested == Some(file);
            let fact = |text: String, differs: bool| {
                div()
                    .flex_none()
                    .text_color(if differs {
                        theme.text()
                    } else {
                        theme.text_tertiary()
                    })
                    .child(text)
            };
            let dot = || div().flex_none().text_color(theme.text_tertiary()).child("·");
            let words = facts.then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text(theme::TEXT_SM)
                            .line_height(px(16.))
                            .child(fact(
                                super::compare::dimensions(
                                    side.file.pixel_width,
                                    side.file.pixel_height,
                                ),
                                sizes_differ,
                            ))
                            .children(suggested.then(|| {
                                div()
                                    .flex_none()
                                    .px(px(5.))
                                    .rounded(px(theme::RADIUS_SM))
                                    .border_1()
                                    .border_color(theme.border_secondary())
                                    .text(theme::TEXT_XS)
                                    .line_height(px(14.))
                                    .text_color(theme.text_secondary())
                                    .child(t("Suggested"))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text(theme::TEXT_CAPTION)
                            .line_height(px(14.))
                            .child(fact(
                                crate::grid::format::format_label(&side.file.mime_type),
                                formats_differ,
                            ))
                            .child(dot())
                            .child(fact(
                                crate::text::size_to_megabytes(side.file.size_bytes),
                                bytes_differ,
                            )),
                    )
            });
            let look = TileLook {
                tile: Tile {
                    x: 0.,
                    y: 0.,
                    w: STRIP_TILE,
                    h: STRIP_TILE,
                },
                item: summary(side, &entry.facts[file], name.clone()),
                thumbnail,
                selected: shown,
                thumb,
                original: None,
                alphas: (1., 0.),
                magnifier: false,
                grayscale: false,
            };
            let art = tile_art(look, &prefs(), &mut self.art, &self.thumbnails, window, cx);
            let ring = (Some(file) == partner && count > 2).then(|| {
                div()
                    .absolute()
                    .inset(px(-5.))
                    .rounded(px(theme::TILE_RADIUS + 5.))
                    .border_1()
                    .border_color(theme.text_tertiary())
            });
            // Without its facts a suggested file keeps a mark on its
            // picture.
            let mark = (suggested && !facts).then(|| {
                div()
                    .absolute()
                    .right(px(-3.))
                    .top(px(-3.))
                    .size(px(8.))
                    .rounded_full()
                    .bg(theme.text_secondary())
            });
            row = row.child(
                div()
                    .id(("duplicate-strip-file", file))
                    .role(Role::Button)
                    .aria_label(name)
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_none()
                            .text(theme::TEXT_CAPTION)
                            .text_color(if shown {
                                theme.text()
                            } else {
                                theme.text_tertiary()
                            })
                            .child((file + 1).to_string()),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .size(px(STRIP_TILE))
                            .child(art)
                            .children(ring)
                            .children(mark),
                    )
                    .children(words)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.focus, cx);
                        this.show_file(file, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.open_file_menu(file, event.position, window, cx);
                        }),
                    ),
            );
        }
        div()
            .h(px(STRIP))
            .flex_none()
            .flex()
            .justify_center()
            .px(px(16.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .child(row)
    }

    /// A file's menu: the app's rows for opening, revealing and copying the
    /// file, then taking it out of its set.
    fn open_file_menu(
        &mut self,
        file: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::grid::menu as rows;
        let Some(index) = self.current_index() else {
            return;
        };
        let entry = &self.entries[index];
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
        let key = entry.set.key;
        let menu = cx.new(|cx| ui::menu::Menu::new(entries, position, cx).search(false));
        self.menu
            .open(menu, window, cx, move |this, _, chosen, _, cx| {
                let Some(index) = this.entry_index(key) else {
                    return;
                };
                let entry = &this.entries[index];
                let side = &entry.set.files[file];
                let facts = &entry.facts[file];
                let path = facts
                    .media(&side.file.file_hash)
                    .map(|media| std::path::PathBuf::from(&media.file_path));
                if chosen == REMOVE_FROM_SET {
                    this.remove_file(file, cx);
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

    /// The decisions, the difference and what was measured, then Skip and
    /// Undo at the right end. The row never wraps: when it is short the
    /// buttons lose their words, "Not duplicates" first, and last the
    /// sentence and Skip and Undo go; each button keeps its tooltip and
    /// its key.
    fn footer(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let width = f32::from(self.stage.pane.size.width);
        let words = |needed: f32| width <= 0. || width >= needed;
        let button =
            |id: &'static str, look: Look, icon: &'static str, label: &'static str, full: bool| {
                let button = ui::action_button(id, look).icon(icon, 15.);
                if full {
                    button.label(label)
                } else {
                    button.width(theme::ACTION_BUTTON).padding(0.)
                }
            };
        // On the file the library suggests the merge is Smart merge; on any
        // other it keeps that file instead.
        let smart = self.shown_is_suggested();
        let merge = button(
            "duplicates-keep-this",
            Look::Primary,
            "action-arrows-join",
            if smart {
                t("Smart merge")
            } else {
                en("Keep this")
            },
            words(300.),
        );
        let merge_tip = if smart {
            ui::tip(t("Keep the stronger file and preserve item metadata"))
                .shortcut("dup.smartMerge", cx)
        } else {
            ui::tip(en("Keep the shown file and merge the others into it")).shortcut("dup.merge", cx)
        };
        let entry = &self.entries[index];
        let partner = self.partner();
        let measurable = partner.is_some_and(|partner| {
            super::original(entry, self.shown).is_some()
                && super::original(entry, partner).is_some()
        });
        let (verdict, detail) = match (measurable, self.measure()) {
            (false, _) => (
                en("Not measured: only still images are compared pixel by pixel.").to_owned(),
                None,
            ),
            (true, None) => (en("Comparing the pixels…").to_owned(), None),
            (true, Some(measure)) => sentence(&measure),
        };
        // Which file the shown one is measured against, in a set of more.
        let against = (entry.set.files.len() > 2)
            .then(|| partner.map(|partner| format!("{} {}: ", en("Against file"), partner + 1)))
            .flatten();
        let hover = cx.weak_entity();
        let difference = button(
            "duplicates-difference",
            Look::Standard,
            "toolbar-difference",
            t("Show Difference"),
            words(500.),
        )
        .disabled(!measurable)
        .tip(ui::tip(t("Show differences while held")).shortcut("dup.difference", cx))
        .on_hover(move |hovered, _, cx| {
            let _ = hover.update(cx, |this, cx| {
                if this.stage.difference_hovered != hovered {
                    this.stage.difference_hovered = hovered;
                    cx.notify();
                }
            });
        });
        div()
            .h(px(FOOTER))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .bg(theme.footer_surface())
            .child(
                merge
                    .disabled(self.busy)
                    .tip(merge_tip)
                    .on_click(cx.listener(|this, _, _, cx| this.merge(cx))),
            )
            .child(
                button(
                    "duplicates-not",
                    Look::Standard,
                    "action-x",
                    t("Not duplicates"),
                    words(780.),
                )
                .disabled(self.busy)
                .tip(ui::tip(t("Not duplicates")).shortcut("dup.notDuplicate", cx))
                .on_click(cx.listener(|this, _, _, cx| this.not_duplicates(cx))),
            )
            .child(difference)
            // The verdict on one line, and under it the detail, if any.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .text(theme::TEXT_SM)
                    .line_height(px(16.))
                    .children(words(340.).then(|| {
                        div()
                            .truncate()
                            .text_color(theme.text())
                            .child(format!("{}{verdict}", against.unwrap_or_default()))
                    }))
                    .children(detail.filter(|_| words(340.)).map(|detail| {
                        div()
                            .truncate()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .child(detail)
                    })),
            )
            .children(words(260.).then(|| {
                button(
                    "duplicates-skip",
                    Look::Ghost,
                    "player-skip-forward",
                    t("Skip"),
                    false,
                )
                .disabled(self.busy)
                .tip(ui::tip(en("Skip to the next set")).shortcut("dup.skip", cx))
                .on_click(cx.listener(|this, _, _, cx| this.skip(cx)))
            }))
            .children(words(260.).then(|| {
                button(
                    "duplicates-undo",
                    Look::Ghost,
                    "menu-arrow-back-up",
                    t("Undo"),
                    false,
                )
                .disabled(self.busy)
                .tip(ui::tip(t("Undo")).shortcut("edit.undo", cx))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.session.update(cx, |session, cx| session.undo(cx));
                }))
            }))
    }

    /// The titlebar's zoom state.
    pub(super) fn zoom_mode(&self) -> ZoomMode {
        self.stage.zoom.mode
    }
}

/// The grid's tile options for a small picture: fitted, with nothing under
/// it.
fn prefs() -> ViewPrefs {
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
fn summary(
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
