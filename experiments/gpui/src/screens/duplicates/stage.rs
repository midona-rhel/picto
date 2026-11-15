//! The stage: the set's files one at a time, flipping in place with ← → or
//! the wheel while the zoom and the place in the picture stay, so what
//! differs blinks. Over it, the difference button, which while the pointer
//! rests on it shows the pixel difference of the shown file and its
//! partner (as the Electron screen's Show Difference does), and the
//! sentence saying what comparing their pixels found. Under it, a strip of
//! the set's files, and the decisions.
use super::DuplicatesScreen;
use super::compare::{Extent, Mode as ZoomMode};
use super::difference::Measure;
use super::queue::{prefs, summary};
use crate::grid::layout::Tile;
use crate::grid::tile::{TileLook, tile_art};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::{theme, ui};
use gpui::*;

/// The strip of the set's files under the picture.
const STRIP: f32 = 64.;
const STRIP_TILE: f32 = 44.;
/// The row over the picture: the difference button and what was measured.
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
    /// The stage: the row over the picture, the picture, its strip and the
    /// decisions.
    pub(super) fn stage(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(index) = self.current_index() else {
            return div().flex_1().into_any_element();
        };
        let header = self.stage_header(index, cx);
        let picture = self.picture(index, cx);
        let strip = self.strip(index, window, cx);
        let footer = self.footer(cx);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            // While the queue is narrow its tabs head the stage, across it.
            .children(self.queue_narrow().then(|| {
                let theme = theme::theme(cx);
                div()
                    .flex_none()
                    .p(px(8.))
                    .border_b_1()
                    .border_color(theme.border_secondary())
                    .child(self.tabs(cx.entity().downgrade()))
            }))
            .child(header)
            .child(picture)
            .child(strip)
            .child(footer)
            .into_any_element()
    }

    /// The difference button and the sentence, where the eye is.
    fn stage_header(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
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
        // Narrow, the button keeps its glyph and its tooltip and leaves
        // the room to the sentence.
        let wide = f32::from(self.stage.pane.size.width) >= 360.;
        let button = ui::action_button("duplicates-difference", Look::Standard)
            .icon("toolbar-difference", 16.)
            .height(theme::CONTROL_MD)
            .text_size(theme::TEXT_SM);
        let button = if wide {
            button.label(t("Show Difference")).padding(12.)
        } else {
            button.width(theme::CONTROL_MD).padding(0.)
        };
        let button = button
            .disabled(!measurable)
            .tip(ui::tip(t("Show differences while held")))
            .on_hover(move |hovered, _, cx| {
                let _ = hover.update(cx, |this, cx| {
                    if this.stage.difference_hovered != hovered {
                        this.stage.difference_hovered = hovered;
                        cx.notify();
                    }
                });
            });
        // The verdict on one line, and under it the detail, if any.
        div()
            .h(px(HEADER))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .border_b_1()
            .border_color(theme.border_secondary())
            .child(div().flex_none().child(button))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .text(theme::TEXT_SM)
                    .line_height(px(16.))
                    .child(
                        div()
                            .truncate()
                            .text_color(theme.text())
                            .child(format!("{}{verdict}", against.unwrap_or_default())),
                    )
                    .children(detail.map(|detail| {
                        div()
                            .truncate()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .child(detail)
                    })),
            )
    }

    /// The shown file, zoomed and placed as the zoom says, or its
    /// difference from its partner while the difference button is held.
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
        if self.stage.difference_hovered {
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

    /// The set's files, numbered: the shown one, which a merge keeps,
    /// carries the grid's selection stroke, and the one it is measured
    /// against a quiet ring.
    fn strip(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let count = self.entries[index].set.files.len();
        let partner = self.partner();
        let mut row = div()
            .id("duplicate-strip")
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .h_full()
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
            let look = TileLook {
                tile: Tile {
                    x: 0.,
                    y: 0.,
                    w: STRIP_TILE,
                    h: STRIP_TILE,
                },
                item: summary(side, &entry.facts[file], name.clone()),
                thumbnail,
                selected: file == self.shown,
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
            row = row.child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .text(theme::TEXT_CAPTION)
                            .text_color(if file == self.shown {
                                theme.text()
                            } else {
                                theme.text_tertiary()
                            })
                            .child((file + 1).to_string()),
                    )
                    .child(
                        div()
                            .id(("duplicate-strip-file", file))
                            .role(Role::Button)
                            .aria_label(name)
                            .relative()
                            .flex_none()
                            .size(px(STRIP_TILE))
                            .cursor_pointer()
                            .child(art)
                            .children(ring)
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
                    ),
            );
        }
        div()
            .h(px(STRIP))
            .flex_none()
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

    /// The decisions, then Skip and Undo at the right end. The row never
    /// wraps: when it is short, Skip and Undo lose their words first, then
    /// "Not duplicates", then the merge says only "Keep this", then shows
    /// its glyph, and last Skip and Undo go; each keeps its tooltip and its
    /// key.
    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let width = f32::from(self.stage.pane.size.width);
        let words = |needed: f32| width <= 0. || width >= needed;
        let button =
            |id: &'static str, look: Look, icon: &'static str, label: &'static str, full: bool| {
                let button = ui::action_button(id, look)
                    .icon(icon, 15.)
                    .disabled(self.busy);
                if full {
                    button.label(label)
                } else {
                    button.width(theme::ACTION_BUTTON).padding(0.)
                }
            };
        let merge = ui::action_button("duplicates-keep-this", Look::Primary)
            .icon("action-arrows-join", 16.);
        let merge = if words(460.) {
            merge.label(en("Keep this, merge the rest"))
        } else if words(320.) {
            merge.label(en("Keep this"))
        } else {
            merge.width(theme::ACTION_BUTTON).padding(0.)
        };
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
                    .tip(
                        ui::tip(en("Keep the shown file and merge the others into it"))
                            .shortcut("dup.merge", cx),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.merge(cx))),
            )
            .child(
                button(
                    "duplicates-not",
                    Look::Standard,
                    "action-x",
                    t("Not duplicates"),
                    words(560.),
                )
                .tip(ui::tip(t("Not duplicates")).shortcut("dup.notDuplicate", cx))
                .on_click(cx.listener(|this, _, _, cx| this.not_duplicates(cx))),
            )
            .child(div().flex_1())
            .children(words(220.).then(|| {
                button(
                    "duplicates-skip",
                    Look::Ghost,
                    "player-skip-forward",
                    t("Skip"),
                    words(660.),
                )
                .tip(ui::tip(en("Skip to the next set")).shortcut("dup.skip", cx))
                .on_click(cx.listener(|this, _, _, cx| this.skip(cx)))
            }))
            .children(words(220.).then(|| {
                button(
                    "duplicates-undo",
                    Look::Ghost,
                    "menu-arrow-back-up",
                    t("Undo"),
                    words(660.),
                )
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
