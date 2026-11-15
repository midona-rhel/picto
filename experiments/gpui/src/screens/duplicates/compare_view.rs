//! Compare: one of a set's files at a time, flipping in place with ← → or
//! the wheel while the zoom and the place in the picture stay, a strip of
//! the set's files under it, and beside it what the shown file has over
//! the others (the inspector's sections and rows). The footer holds the
//! decisions.
use super::compare::{Extent, Mode as ZoomMode};
use super::list::{FOOTER, summary};
use super::sets::{Lean, resolution_share};
use super::{DuplicatesScreen, Entry};
use crate::grid::layout::Tile;
use crate::grid::prefs::ViewPrefs;
use crate::grid::tile::{TileLook, tile_art};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::{theme, ui};
use gpui::*;

/// The facts column, as wide as the spec draws it.
const COLUMN: f32 = 210.;
/// The strip of the set's files under the picture.
const STRIP: f32 = 64.;
const STRIP_TILE: f32 = 44.;

/// One statement about the shown file.
struct Statement {
    lean: Lean,
    text: String,
}

/// What the shown file has over the rest of its set, or lacks.
fn statements(entry: &Entry, shown: usize) -> Vec<Statement> {
    let set = &entry.set;
    let mine = &set.files[shown].file;
    let others = (0..set.files.len())
        .filter(|index| *index != shown)
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    if let Some(share) = resolution_share(set, shown) {
        let (lean, text) = match share {
            190.. => (Lean::Better, en("Twice the resolution or more")),
            101..=189 => (Lean::Better, en("Higher resolution")),
            100 => (Lean::Same, en("Same resolution")),
            51..=99 => (Lean::Worse, en("Lower resolution")),
            _ => (Lean::Worse, en("Half the resolution or less")),
        };
        out.push(Statement {
            lean,
            text: text.into(),
        });
    }
    let format = crate::grid::format::format_label(&mine.mime_type);
    let other_formats = others
        .iter()
        .map(|&index| crate::grid::format::format_label(&set.files[index].file.mime_type))
        .collect::<std::collections::BTreeSet<_>>();
    if other_formats.len() == 1 && other_formats.contains(&format) {
        out.push(Statement {
            lean: Lean::Same,
            text: format!("{} {format}", t("All")),
        });
    } else {
        let lossless =
            |mime: &str| matches!(mime, "image/png" | "image/bmp" | "image/tiff" | "image/gif");
        let lossy = |mime: &str| matches!(mime, "image/jpeg" | "image/jpg");
        let lean = if lossless(&mine.mime_type)
            && others
                .iter()
                .all(|&index| lossy(&set.files[index].file.mime_type))
        {
            Lean::Better
        } else if lossy(&mine.mime_type)
            && others
                .iter()
                .all(|&index| lossless(&set.files[index].file.mime_type))
        {
            Lean::Worse
        } else {
            Lean::Same
        };
        let rest = other_formats.into_iter().collect::<Vec<_>>().join(", ");
        out.push(Statement {
            lean,
            text: format!("{format}, {} {rest}", en("the others are")),
        });
    }
    let tags = |index: usize| entry.facts[index].tags();
    if let Some(count) = tags(shown)
        && let Some(other) = others
            .iter()
            .map(|&index| tags(index))
            .collect::<Option<Vec<_>>>()
    {
        let (most, least) = (
            other.iter().copied().max().unwrap_or(0),
            other.iter().copied().min().unwrap_or(0),
        );
        let label = if count == 1 {
            en("1 tag").to_owned()
        } else {
            format!("{count} {}", en("tags"))
        };
        let (lean, text) = if count > most {
            (Lean::Better, format!("{label}, {}", en("the most")))
        } else if count < least {
            (Lean::Worse, format!("{label}, {}", en("the fewest")))
        } else {
            (Lean::Same, label)
        };
        out.push(Statement { lean, text });
    }
    let added = |index: usize| {
        entry.facts[index]
            .details
            .as_ref()
            .map(|details| details.root.imported_at_ms)
    };
    if let Some(at) = added(shown)
        && let Some(other) = others
            .iter()
            .map(|&index| added(index))
            .collect::<Option<Vec<_>>>()
    {
        let date = super::compare::short_date(at);
        let text = if other.iter().all(|other| at < *other) {
            format!("{}, {date}", en("Added first"))
        } else if other.iter().all(|other| at > *other) {
            format!("{}, {date}", en("Added last"))
        } else {
            format!("{} {date}", t("Added"))
        };
        out.push(Statement {
            lean: Lean::Same,
            text,
        });
    }
    let distance = set
        .edges
        .iter()
        .filter(|edge| edge.a == mine.file_id || edge.b == mine.file_id)
        .map(|edge| edge.distance)
        .max()
        .unwrap_or(0);
    out.push(Statement {
        lean: Lean::Same,
        text: if distance == 0 {
            en("Same picture").to_owned()
        } else {
            super::compare::similarity_label(distance)
        },
    });
    out
}

impl DuplicatesScreen {
    /// Compare's whole view: the picture and its strip, the facts beside
    /// them, and the footer.
    pub(super) fn compare_view(
        &mut self,
        key: u32,
        shown: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let Some(index) = self.entry_index(key) else {
            return div().into_any_element();
        };
        let keeper = self.keeper(index);
        let picture = self.picture(index, shown, keeper, window, cx);
        let strip = self.strip(index, shown, keeper, window, cx);
        let column = self.facts_column(index, shown, keeper, window, cx);
        let footer = self.compare_footer(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(picture)
                            .child(strip),
                    )
                    .child(
                        div()
                            .w(px(COLUMN))
                            .flex_none()
                            .border_l_1()
                            .border_color(theme.border_secondary())
                            .child(column),
                    ),
            )
            .child(footer)
            .into_any_element()
    }

    /// The shown file, zoomed and placed as the zoom says, or its
    /// difference from the kept file while that button is held.
    fn picture(
        &mut self,
        index: usize,
        shown: usize,
        keeper: usize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let entry = &self.entries[index];
        let side = &entry.set.files[shown];
        let id = side.file.file_id.0;
        let entity = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                let changed = entity
                    .update(cx, |this, _| {
                        std::mem::replace(&mut this.compare.pane, bounds) != bounds
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
        let pane = super::pane_extent(self.compare.pane);
        let image = Extent::new(
            side.file.pixel_width.unwrap_or(0) as f32,
            side.file.pixel_height.unwrap_or(0) as f32,
        );
        let frame = |layer: Div| {
            if pane.width > 0. && pane.height > 0. && image.width > 0. && image.height > 0. {
                let frame = self.compare.zoom.frame(pane, image);
                layer
                    .absolute()
                    .left(px(frame.left))
                    .top(px(frame.top))
                    .w(px(frame.width))
                    .h(px(frame.height))
            } else {
                layer.absolute().inset_0()
            }
        };
        let fill = |image: std::sync::Arc<RenderImage>| img(image).absolute().inset_0().size_full();
        let thumbnail = self.session.read(cx).thumbnail(&side.file.file_hash);
        let difference = self
            .compare
            .difference
            .as_ref()
            .filter(|((a, _), _)| *a == id)
            .map(|(_, image)| image.clone());
        let mut layers = frame(div());
        if self.compare.difference_hovered && shown != keeper {
            layers = layers
                .bg(rgb(theme::DIFFERENCE_BACKGROUND))
                .children(difference.map(|image| fill(image).object_fit(ObjectFit::Fill)));
        } else {
            layers = layers.child(
                img(thumbnail)
                    .image_cache(&self.thumbnails)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            );
            if let Some(full) = self.compare.originals.get(&id).cloned() {
                layers = layers.child(fill(full).object_fit(ObjectFit::Contain));
            }
        }
        // On the picture's corner, or the pane's when the picture is
        // zoomed past it.
        let corner = if pane.width > 0. && image.width > 0. {
            let frame = self.compare.zoom.frame(pane, image);
            (frame.left.max(0.) + 8., frame.top.max(0.) + 8.)
        } else {
            (8., 8.)
        };
        let keeping = (shown == keeper).then(|| {
            ui::badge(6.)
                .left(px(corner.0))
                .top(px(corner.1))
                .gap(px(4.))
                .text(theme::TEXT_CAPTION)
                .text_color(rgba(theme::BADGE_TEXT))
                .child(ui::icon("check", 12., rgba(theme::BADGE_TEXT)))
                .child(ui::badge_text(en("Keeping this one")))
        });
        let dragging = self.compare.drag.is_some();
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
            .children(keeping)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if event.click_count == 2 {
                        this.compare.drag = None;
                        this.fit(cx);
                    } else {
                        this.compare.drag = Some((event.position, this.compare.zoom.center()));
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
        let Some((from, center)) = self.compare.drag else {
            return;
        };
        let Some(image) = self.shown_extent() else {
            return;
        };
        let moved = position - from;
        self.compare.zoom.pan(
            super::pane_extent(self.compare.pane),
            image,
            center,
            (f32::from(moved.x), f32::from(moved.y)),
        );
        cx.notify();
    }

    /// The set's files, the shown one carrying the grid's selection stroke
    /// and the kept one its check.
    fn strip(
        &mut self,
        index: usize,
        shown: usize,
        keeper: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let count = self.entries[index].set.files.len();
        let key = self.entries[index].set.key;
        let prefs = ViewPrefs {
            mode: crate::grid::layout::ViewMode::Grid,
            show_name: false,
            show_resolution: false,
            show_extension_label: false,
            fit_cover: false,
            ..ViewPrefs::default()
        };
        let mut row = div()
            .id("duplicate-strip")
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
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
                selected: file == shown,
                thumb,
                original: None,
                alphas: (1., 0.),
                magnifier: false,
                grayscale: false,
            };
            let art = tile_art(look, &prefs, &mut self.art, &self.thumbnails, window, cx);
            let check = (file == keeper).then(|| {
                div()
                    .absolute()
                    .right(px(-4.))
                    .top(px(-4.))
                    .size(px(14.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(theme::GRID_SELECTION))
                    .border_1()
                    .border_color(theme.background())
                    .child(ui::icon("check", 9., rgb(theme::BADGE_STRONG_TEXT)))
            });
            row = row.child(
                div()
                    .id(("duplicate-strip-file", file))
                    .role(Role::Button)
                    .aria_label(name)
                    .relative()
                    .flex_none()
                    .size(px(STRIP_TILE))
                    .cursor_pointer()
                    .child(art)
                    .children(check)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.mode = super::Mode::Compare {
                            set: key,
                            shown: file,
                        };
                        this.cursor = Some((key, file));
                        this.load_compared(cx);
                        this.changed(cx);
                    })),
            );
        }
        div()
            .h(px(STRIP))
            .flex_none()
            .border_t_1()
            .border_color(theme.border_secondary())
            .child(row)
    }

    /// The shown file's name, its statements, and its properties, in the
    /// inspector's sections and rows.
    fn facts_column(
        &self,
        index: usize,
        shown: usize,
        keeper: usize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let entry = &self.entries[index];
        let side = &entry.set.files[shown];
        let facts = &entry.facts[shown];
        let name = facts.name(&entry.set, shown);
        let statement = |statement: Statement| {
            let (glyph, color) = match statement.lean {
                Lean::Better => ("arrow-up", rgb(theme::SUCCESS)),
                Lean::Worse => ("arrow-up", rgb(theme::DANGER)),
                Lean::Same => ("minus", theme.text_tertiary()),
            };
            let mut icon = ui::icon(glyph, 12., color).flex_none();
            if statement.lean == Lean::Worse {
                icon =
                    icon.with_transformation(Transformation::rotate(radians(std::f32::consts::PI)));
            }
            div()
                .h(px(20.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text(theme::TEXT_CAPTION)
                .text_color(match statement.lean {
                    Lean::Same => theme.text_secondary(),
                    _ => theme.text(),
                })
                .child(icon)
                .child(div().min_w_0().truncate().child(statement.text))
        };
        let details = facts.details.as_ref();
        let folders = {
            let session = self.session.read(cx);
            let all = crate::inspector::folder_picker::folders(&session.nav);
            details
                .map(|details| {
                    details
                        .folder_ids
                        .iter()
                        .filter_map(|id| all.iter().find(|folder| folder.id == *id))
                        .map(|folder| folder.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|names| !names.is_empty())
                .unwrap_or_else(|| t("None").to_owned())
        };
        let source = details
            .and_then(|details| details.root.source_urls.first())
            .map(|url| {
                url.split("://")
                    .nth(1)
                    .unwrap_or(url)
                    .split('/')
                    .next()
                    .unwrap_or(url)
                    .to_owned()
            })
            .unwrap_or_else(|| t("None").to_owned());
        let row = |label: &'static str, value: String| ui::property_row(label, value);
        let properties = div()
            .flex()
            .flex_col()
            .text_color(theme.text_secondary())
            .child(row(
                t("Dimensions"),
                super::compare::dimensions(side.file.pixel_width, side.file.pixel_height),
            ))
            .child(row(
                t("Size"),
                crate::text::size_to_megabytes(side.file.size_bytes),
            ))
            .child(row(
                t("Format"),
                crate::grid::format::format_label(&side.file.mime_type),
            ))
            .child(row(
                t("Tags"),
                facts
                    .tags()
                    .map_or_else(|| t("Unknown").to_owned(), |tags| tags.to_string()),
            ))
            .child(row(t("Folders"), folders))
            .child(row(t("Source"), source))
            .child(row(
                t("Added"),
                details.map_or_else(
                    || t("Unknown").to_owned(),
                    |details| super::compare::short_date(details.root.imported_at_ms),
                ),
            ));
        let lines = statements(entry, shown)
            .into_iter()
            .map(statement)
            .collect::<Vec<_>>();
        div()
            .id("duplicate-facts")
            .size_full()
            .overflow_y_scroll()
            .px(px(12.))
            .pt(px(12.))
            .flex()
            .flex_col()
            .child(
                div()
                    .text(theme::TEXT_MD)
                    .text_color(theme.text())
                    .truncate()
                    .child(name),
            )
            .child(
                div()
                    .mt(px(2.))
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .child(if shown == keeper {
                        en("Kept when the set is merged")
                    } else {
                        en("Merged away when the set is merged")
                    }),
            )
            .child(
                ui::panel_section("duplicate-this-file", en("This file")).content(Some(
                    div().flex().flex_col().children(lines).into_any_element(),
                )),
            )
            .child(
                ui::panel_section("duplicate-properties", t("Properties"))
                    .content(Some(properties.into_any_element())),
            )
    }

    /// The decisions on the set, and the way back.
    /// The decisions, then Skip and Undo at the right end. The row never
    /// wraps: when it is short, Skip and Undo lose their words first, then
    /// "Keep all, related" and "Not duplicates"; each keeps its tooltip.
    fn compare_footer(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let width = f32::from(self.compare.pane.size.width) + COLUMN;
        let words = |needed: f32| width <= COLUMN || width >= needed;
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
        let (decisions, rest) = (words(640.), words(780.));
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
                ui::action_button("duplicates-keep-this", Look::Primary)
                    .icon("action-arrows-join", 16.)
                    .label(en("Keep this, merge the rest"))
                    .disabled(self.busy)
                    .tip(
                        ui::tip(en("Keep the shown file and merge the others into it"))
                            .shortcut("dup.merge", cx),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.merge_key(cx))),
            )
            .child(
                button(
                    "duplicates-keep-all",
                    Look::Standard,
                    "action-copy",
                    en("Keep all, related"),
                    decisions,
                )
                .tip(ui::tip(en("Keep all, related")).shortcut("dup.keepAll", cx))
                .on_click(cx.listener(|this, _, _, cx| this.apart_key(cx))),
            )
            .child(
                button(
                    "duplicates-not",
                    Look::Standard,
                    "action-x",
                    t("Not duplicates"),
                    decisions,
                )
                .tip(ui::tip(t("Not duplicates")).shortcut("dup.notDuplicate", cx))
                .on_click(cx.listener(|this, _, _, cx| this.apart_key(cx))),
            )
            .child(div().flex_1())
            .child(
                button(
                    "duplicates-skip",
                    Look::Ghost,
                    "player-skip-forward",
                    t("Skip"),
                    rest,
                )
                .tip(ui::tip(en("Skip to the next set")).shortcut("dup.compare", cx))
                .on_click(cx.listener(|this, _, _, cx| this.skip(cx))),
            )
            .child(
                button(
                    "duplicates-undo",
                    Look::Ghost,
                    "menu-arrow-back-up",
                    t("Undo"),
                    rest,
                )
                .tip(ui::tip(t("Undo")).shortcut("edit.undo", cx))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.session.update(cx, |session, cx| session.undo(cx));
                })),
            )
    }

    /// The titlebar's zoom state in Compare.
    pub(super) fn zoom_mode(&self) -> ZoomMode {
        self.compare.zoom.mode
    }
}
