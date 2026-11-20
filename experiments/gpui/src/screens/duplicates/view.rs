//! The Duplicates screen's drawing: the loading and empty states, the two
//! files side by side (each its picture, then one line with its name and
//! its keep action, then its facts as the inspector writes them), and the
//! titlebar's row: the place in the queue, what was measured, the decisions
//! on the pair and the zoom.
//!
//! Everything is laid out by flex in the frame that draws it, and each
//! picture is placed from its pane's size in that same frame (`ui::sized`),
//! so the screen follows the window while it is resized.
use super::{
    ActualPixels, CONTEXT, Decision, Drag, DuplicatesScreen, DuplicatesToolbar, FitPair, KeepBoth,
    KeepLeft, KeepRight, NextPair, NotDuplicate, PreviousPair, ShowDifference, Shown, SmartMerge,
    compare, extent, lean,
};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::ui::property_row::Emphasis;
use crate::{motion, theme, ui};
use gpui::*;
use picto_library::{Rating, RootKind};
use std::sync::Arc;

/// The room around the two files and between them.
const MARGIN: f32 = 12.;
/// A picture's pane is never lower than this, however short the window.
const PANE_MIN: f32 = 120.;
/// A file's name line and facts are no wider than the inspector is, and
/// stand against the middle of the screen, where the two pictures meet.
const FACTS_WIDTH: f32 = 360.;
const NAME_LINE: f32 = 36.;
/// A property row's height, and how many rows a file's facts take at most,
/// so the pictures keep their place from pair to pair.
const ROW: f32 = 20.;
const ROWS: f32 = 9.;
/// How far the file Smart merge would not keep dims while Smart merge is
/// under the pointer or has the keyboard.
const MERGED_AWAY: f32 = 0.35;
/// Where the scope title starts in the titlebar's row of controls (it is
/// drawn 80px into the toolbar, the row starting after the toolbar's 8px
/// and the two history buttons), and the room kept clear after it.
const TITLE_START: f32 = 80. - 8. - 2. * theme::ICON_BUTTON;
const TITLE_CLEAR: f32 = 16.;
/// The room between the titlebar's groups: the place, what was measured,
/// the decisions and the zoom.
const GROUP_GAP: f32 = 12.;
/// Between the similarity and what was counted, and the most the two take.
const MEASURED_BETWEEN: f32 = 5.;
const MEASURED_MOST: f32 = 420.;

/// One fact of the two files, as the inspector would write each.
struct Fact {
    label: &'static str,
    values: [String; 2],
}

/// A rating as the inspector's menus write it.
fn rating_text(rating: Rating) -> String {
    match rating {
        Rating::Unrated => t("Unrated").to_owned(),
        rating => "★".repeat(rating.bitmap_key() as usize),
    }
}

/// The facts the Electron screen lists of each file, in its order, in the
/// inspector's words and formats (the dates as days). Both files have the same rows, so a row
/// of one is beside the same row of the other.
fn facts(shown: &Shown) -> Vec<Fact> {
    let each = |value: &dyn Fn(&super::Side) -> String| shown.sides.each_ref().map(value);
    // The day alone, as the Electron screen writes it (`toLocaleDateString`):
    // two files brought in on one day say the same.
    let date = |at: Option<i64>| {
        use chrono::TimeZone;
        at.filter(|at| *at != 0)
            .and_then(|at| chrono::Local.timestamp_millis_opt(at).single())
            .map(|at| crate::i18n::format::date(at.date_naive()))
            .unwrap_or_else(|| t("Unknown").to_owned())
    };
    let mut facts = vec![
        Fact {
            label: t("Dimensions"),
            values: each(&|side| {
                compare::dimensions(side.file.pixel_width, side.file.pixel_height)
            }),
        },
        Fact {
            label: t("Size"),
            values: each(
                &|side| crate::text::size_to_gigabytes(side.file.size_bytes.max(0) as u64),
            ),
        },
        Fact {
            label: t("Type"),
            values: each(&|side| crate::text::inspector_type(&side.file.mime_type)),
        },
        Fact {
            label: t("Rating"),
            values: each(&|side| {
                rating_text(
                    side.details
                        .as_ref()
                        .map_or(Rating::Unrated, |details| details.rating),
                )
            }),
        },
        Fact {
            label: t("Tags"),
            values: each(&|side| {
                side.details
                    .as_ref()
                    .map_or(0, |details| details.tag_ids.len())
                    .to_string()
            }),
        },
        Fact {
            label: t("Date Created"),
            values: each(&|side| {
                date(
                    side.details
                        .as_ref()
                        .and_then(|details| details.root.captured_at_ms),
                )
            }),
        },
        Fact {
            label: t("Date Imported"),
            values: each(&|side| {
                date(
                    side.details
                        .as_ref()
                        .map(|details| details.root.imported_at_ms),
                )
            }),
        },
    ];
    // A file inside a collection names it; the row shows for both files
    // when either is in one.
    let group = |side: &super::Side| {
        side.details
            .as_ref()
            .map(|details| &details.root)
            .filter(|root| root.kind == RootKind::Collection)
            .map(|root| {
                if root.name.is_empty() {
                    format!("{} {}", t("Group"), root.root_id.0)
                } else {
                    root.name.clone()
                }
            })
    };
    if shown.sides.iter().any(|side| group(side).is_some()) {
        facts.push(Fact {
            label: t("Group"),
            values: each(&|side| group(side).unwrap_or_else(|| t("None").to_owned())),
        });
    }
    facts.push(Fact {
        label: t("Occurrences"),
        values: each(&|side| side.occurrences.to_string()),
    });
    facts
}

impl DuplicatesScreen {
    /// The queue's first read, with its message after 200 ms.
    fn loading_state(&self, cx: &App) -> Div {
        let theme = theme::theme(cx);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text(theme::TEXT_SM)
            .text_color(theme.text_secondary())
            .children(
                self.loading_message
                    .then_some(t("Loading duplicate review queue...")),
            )
    }

    fn empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let resolved = self.initial_total.saturating_sub(self.total);
        let (title, description) = match &self.error {
            Some(error) => (
                t("Unable to load duplicate review").to_owned(),
                error.clone(),
            ),
            None if resolved > 0 => (
                en("Review complete").to_owned(),
                format!("{resolved} decisions saved."),
            ),
            None => (
                en("No duplicate pairs").to_owned(),
                en("Scan the library to find similar images.").to_owned(),
            ),
        };
        let action = ui::empty_state_action(
            "duplicates-scan",
            "action-refresh",
            if self.error.is_some() {
                t("Retry")
            } else {
                t("Scan library")
            },
        )
        .disabled(self.scanning || self.resolving)
        .on_click(cx.listener(|this, _, _, cx| this.scan(cx)));
        ui::empty_state(
            "empty-copy",
            title,
            description,
            vec![action.into_any_element()],
            cx,
        )
        // `.progress`: the 180 × 2 slot of the scan's progress bar, which
        // keeps its place while empty.
        .child(
            div()
                .w(px(180.))
                .h(px(2.))
                .flex_none()
                .overflow_hidden()
                .children(self.scan_progress.then(|| ui::progress_bar().height(2.))),
        )
        .into_any_element()
    }

    /// One file: its picture, the line with its name and its keep action,
    /// and its facts. The line and the facts stand against the middle of
    /// the screen, as the pictures do.
    fn side(
        &self,
        index: usize,
        facts: &[Fact],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let shown = self.shown.as_ref().expect("a pair is on screen");
        let side = &shown.sides[index];
        let name = side.media.as_ref().map_or_else(
            || side.file.file_hash.clone(),
            |media| media.media_name.clone(),
        );
        // Production fills the side's English name in: `t("Keep {value0}")`
        // for the tip, `{t("Keep ")}{side}` on the button.
        let which = if index == 0 { "left" } else { "right" };
        // Keeps this file and merges the other's item into it. While Smart
        // merge is previewed, the keep action it would take is the lit one.
        let previewed = self.merge_previewed();
        let keep = ui::action_button(
            ("duplicate-keep", index),
            if previewed == Some(index) {
                Look::Primary
            } else {
                Look::Ghost
            },
        )
        .icon("check", 14.)
        .label(format!("{}{which}", t("Keep ")))
        .height(theme::CONTROL_SM)
        .padding(10.)
        .gap(6.)
        .text_size(theme::TEXT_SM)
        .tip(ui::tip(crate::tf!("Keep {value0}", which)).shortcut(
            if index == 0 {
                "dup.keepLeft"
            } else {
                "dup.keepRight"
            },
            cx,
        ))
        .disabled(self.resolving)
        .on_click(cx.listener(move |this, _, window, cx| this.keep(index, window, cx)));
        let name_line = div()
            .h(px(NAME_LINE))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::tooltip::attach(
                div()
                    .id(("duplicate-name", index))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text(theme::TEXT_MD)
                    .text_color(theme.text())
                    .child(name.clone()),
                ("duplicate-name-tip", index),
                ui::tip(name.clone()),
                NAME_LINE,
                window,
                cx,
            ))
            .child(keep);
        // What the two files say differently stands out; what they say
        // alike stays quiet.
        let rows = facts.iter().map(|fact| {
            ui::property_row(fact.label, fact.values[index].clone()).emphasis(
                if fact.values[0] == fact.values[1] {
                    Emphasis::Quiet
                } else {
                    Emphasis::Strong
                },
            )
        });
        // Read as one group, named by its file.
        let about = div()
            .id(("duplicate-facts", index))
            .role(Role::Group)
            .aria_label(name.clone())
            .w_full()
            .max_w(px(FACTS_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .child(name_line)
            .child(
                div()
                    .h(px(ROW * ROWS))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .children(rows),
            );
        // The file Smart merge would merge away dims while Smart merge is
        // previewed, over 140 ms.
        let fade = self.survivor.value();
        let dimmed = if self.preview_side == Some(1 - index) {
            1. - (1. - MERGED_AWAY) * fade
        } else {
            1.
        };
        let column = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .opacity(dimmed)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_file_menu(index, event.position, window, cx);
                }),
            )
            .child(self.pane(index, cx))
            .child(about);
        if index == 0 {
            column.items_end()
        } else {
            column.items_start()
        }
    }

    /// A file's menu (`MediaCard`'s): open it in its default application,
    /// or in a window of its own.
    fn open_file_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::grid::menu as rows;
        let Some(side) = self.shown.as_ref().map(|shown| &shown.sides[index]) else {
            return;
        };
        let description = rows::Description {
            single_hash: Some(side.file.file_hash.clone()),
            handlers: rows::Handlers {
                open_default: true,
                open_new_window: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let built = rows::open_menu(&description, &hints);
        let entries = rows::entries(built, &mut |_| ui::menu::Entry::Separator);
        let menu = cx.new(|cx| ui::menu::Menu::new(entries, position, cx).search(false));
        self.menu
            .open(menu, window, cx, move |this, _, key, _, cx| {
                let Some(side) = this.shown.as_ref().map(|shown| &shown.sides[index]) else {
                    return;
                };
                if key == rows::Action::OpenDefault.key() {
                    // `openDefaultAppForHash`: where the library keeps the file.
                    if let Some(media) = &side.media {
                        crate::desktop::open_default(std::path::Path::new(&media.file_path), cx);
                    }
                } else if key == rows::Action::OpenInNewWindow.key() {
                    // `openDetailWindow({ hash, width, height })`: the file alone.
                    let Some(details) = &side.details else {
                        return;
                    };
                    let entry = crate::viewer::detail::Entry {
                        root: details.root.root_id,
                        hash: side.file.file_hash.clone(),
                        name: side
                            .media
                            .as_ref()
                            .map(|media| media.media_name.clone())
                            .unwrap_or_else(|| side.file.file_hash.clone()),
                        width: side.file.pixel_width,
                        height: side.file.pixel_height,
                        collection: false,
                    };
                    crate::viewer::detail::open(this.session.clone(), vec![entry], 0, cx);
                }
            });
    }

    /// A file's pane: all the room its side has above its name. It has no
    /// frame and no fill of its own; the picture is what shows.
    fn pane(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let entity = cx.weak_entity();
        let dragging = self.drag.as_ref().is_some_and(|drag| drag.side == index);
        div()
            .id(("duplicate-pane", index))
            .relative()
            .w_full()
            .flex_1()
            .min_h(px(PANE_MIN))
            .overflow_hidden()
            .cursor(if dragging {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .child(
                ui::sized(move |bounds, window, cx| {
                    entity
                        .update(cx, |this, cx| this.picture(index, bounds, window, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .absolute()
                .inset_0(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if event.click_count == 2 {
                        this.drag = None;
                        this.fit(cx);
                    } else {
                        this.drag = Some(Drag {
                            side: index,
                            from: event.position,
                            center: this.zoom.center(),
                        });
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| {
                this.wheel(index, event, window, cx);
                cx.stop_propagation();
            }))
    }

    /// What a pane of these bounds shows, built in the frame that gives the
    /// pane its bounds: the picture where the shared zoom puts it (its
    /// original over its thumbnail once that is decoded), or the difference
    /// picture while the difference is asked for.
    fn picture(
        &mut self,
        index: usize,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.panes[index] = bounds;
        let theme = theme::theme(cx);
        let Some(shown) = &self.shown else {
            return div().into_any_element();
        };
        let side = &shown.sides[index];
        let (pane, image) = (extent(bounds), side.image());
        let frame = |layer: Div| {
            if pane.width > 0. && pane.height > 0. && image.width > 0. && image.height > 0. {
                let frame = self.zoom.frame(pane, image, lean(index));
                layer
                    .absolute()
                    .left(px(frame.left))
                    .top(px(frame.top))
                    .w(px(frame.width))
                    .h(px(frame.height))
            } else {
                // A file whose size the library does not know fills the
                // pane, fitted.
                layer.absolute().inset_0()
            }
        };
        let fill = |image: Arc<RenderImage>, fit: ObjectFit| {
            img(image).absolute().inset_0().size_full().object_fit(fit)
        };
        let mut inside = div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .children(side.details.is_none().then(|| {
                div()
                    .text_color(theme.text_secondary())
                    .text(theme::TEXT_SM)
                    .child(t("Media details unavailable"))
            }));
        if self.difference_shown() {
            // `.differenceComposite`: on black, over the picture's place.
            inside = inside.child(
                frame(div().bg(rgb(theme::DIFFERENCE_BACKGROUND))).children(
                    shown
                        .difference
                        .clone()
                        .map(|(image, _)| fill(image, ObjectFit::Fill)),
                ),
            );
        } else {
            let (thumbnail, original) = self.pictures_of(side);
            let opacity = if original.is_some() {
                side.sharp_opacity()
            } else {
                0.
            };
            // Painting continues only while an arrived original fades in.
            if original.is_some() && opacity < 1. {
                window.request_animation_frame();
            }
            let mut layers = frame(div());
            if opacity < 1.
                && let Some(thumbnail) = thumbnail
            {
                layers = layers.child(fill(thumbnail, ObjectFit::Contain));
            }
            if let Some(original) = original {
                layers = layers.child(fill(original, ObjectFit::Contain).opacity(opacity));
            }
            inside = inside.child(layers);
        }
        inside.into_any_element()
    }

    /// The screen's titlebar, built for the titlebar's own width in the
    /// frame that gives it that width, as a subscription's page builds its
    /// actions: after the screen's title the place in the queue between the
    /// buttons for the pair before and after, and quietly beside it how
    /// alike the scan found the files and what comparing their pixels
    /// counted; at the right end the decisions on the pair, Smart merge
    /// first, then the zoom.
    ///
    /// A titlebar too short for all of it gives way in one order: what was
    /// counted shortens to an ellipsis, down to the similarity alone; the
    /// four decisions show their glyphs, all together, each named with its
    /// key by its tooltip; the similarity goes; then the place's figures,
    /// the zoom and last the two arrows. Nothing moves to another row.
    pub(super) fn titlebar(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(model) = self.toolbar_model() else {
            return div().into_any_element();
        };
        let theme = theme::theme(cx);
        let width_of = |text: &str, window: &Window| {
            f32::from(
                ui::shape_line(
                    text.to_owned(),
                    font(theme::FONT_UI),
                    theme::TEXT_SM,
                    theme.text(),
                    window,
                )
                .width,
            )
            .ceil()
        };
        // The scope title is drawn over the start of this row.
        let title = f32::from(
            ui::shape_line(
                t("Duplicates").to_owned(),
                font(theme::FONT_UI),
                theme::TEXT_MD,
                theme.text(),
                window,
            )
            .width,
        )
        .ceil();
        let lead = TITLE_START + title + TITLE_CLEAR;
        // The place keeps the width of its widest figures, so the arrows
        // stay put from pair to pair.
        let place = format!("{} / {}", model.index + 1, model.total);
        let place_width = width_of(&format!("{0} / {0}", model.total), window) + 12.;
        let shown = self.shown.as_ref();
        let similarity = shown.map(|shown| compare::similarity_label(shown.distance));
        // What was counted is written once it has been, and stays as it is
        // for as long as the pair does.
        let counted = shown.and_then(|shown| match shown.measure {
            Some(measure) => {
                let (counted, more) = measure.sentence();
                Some(match more {
                    Some(more) => format!("{counted} · {more}"),
                    None => counted,
                })
            }
            None if !shown.measurable() => {
                Some(en("Pixels are compared in still images only").to_owned())
            }
            None => None,
        });
        let similarity_width = similarity
            .as_deref()
            .map_or(0., |text| width_of(text, window));
        let measured_width = (similarity_width
            + counted.as_deref().map_or(0., |text| {
                MEASURED_BETWEEN + width_of(&format!("· {text}"), window)
            }))
        .min(MEASURED_MOST);
        // How wide the four decisions are with their words, in this
        // language: laid out once to be measured, as a list measures a row.
        let labelled = f32::from(
            self.decisions(true, cx)
                .into_any_element()
                .layout_as_root(
                    size(AvailableSpace::MaxContent, bounds.size.height.into()),
                    window,
                    cx,
                )
                .width,
        );
        let fit = fit(
            f32::from(bounds.size.width) - lead,
            Widths {
                arrows: 2. * theme::ICON_BUTTON,
                place: place_width,
                similarity: similarity_width,
                labelled,
                glyphs: 4. * ui::titlebar_button::HEIGHT + 3. * ui::titlebar_button::BETWEEN,
                zoom: 2. * theme::ICON_BUTTON + ui::titlebar_button::BETWEEN,
            },
        );
        let step = |id: &'static str,
                    icon: &'static str,
                    label: &'static str,
                    key: &'static str,
                    delta: isize,
                    enabled: bool| {
            ui::icon_button(id, icon)
                .disabled(!enabled || model.disabled)
                .tip(ui::tip(label).shortcut(key, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.step(delta, cx)))
        };
        let mut row = div()
            .size_full()
            .flex()
            .items_center()
            .overflow_hidden()
            .child(div().w(px(lead)).flex_none());
        if fit.arrows {
            row = row.child(step(
                "duplicates-previous",
                "toolbar-chevron-left",
                t("Previous pair"),
                "dup.prevPair",
                -1,
                model.can_previous,
            ));
        }
        if fit.place {
            row = row.child(
                div()
                    .w(px(place_width))
                    .flex_none()
                    .text_center()
                    .whitespace_nowrap()
                    .text(theme::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary())
                    .child(place),
            );
        }
        if fit.arrows {
            row = row.child(step(
                "duplicates-next",
                "toolbar-chevron-right",
                t("Next pair"),
                "dup.nextPair",
                1,
                model.can_next,
            ));
        }
        if let (Some(room), Some(similarity)) = (fit.measured, similarity) {
            let line = div()
                .w(px(room.min(measured_width)))
                .ml(px(GROUP_GAP))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(MEASURED_BETWEEN))
                .text(theme::TEXT_SM)
                .child(ui::tooltip::attach(
                    div()
                        .id("duplicates-similarity")
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(theme.text_secondary())
                        .child(similarity),
                    "duplicates-similarity-tip",
                    ui::tip(t("Perceptual similarity is not pixel equality")),
                    20.,
                    window,
                    cx,
                ));
            row = row.child(match counted {
                Some(counted) => line.child(ui::tooltip::attach(
                    div()
                        .id("duplicates-counted")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_tertiary())
                        .child(format!("· {counted}")),
                    "duplicates-counted-tip",
                    ui::tip(counted),
                    20.,
                    window,
                    cx,
                )),
                None => line,
            });
        }
        row = row
            .child(div().flex_1().min_w(px(GROUP_GAP)))
            .child(self.decisions(fit.words, cx));
        if fit.zoom {
            row = row.child(
                div()
                    .ml(px(GROUP_GAP))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(ui::titlebar_button::BETWEEN))
                    .child(
                        ui::icon_button("duplicates-fit", "toolbar-fit")
                            .toggled(model.fit)
                            .tip(ui::tip(t("Zoom to fit")).shortcut("dup.fitToWindow", cx))
                            .on_click(cx.listener(|this, _, _, cx| this.fit(cx))),
                    )
                    .child(
                        ui::icon_button("duplicates-actual", "toolbar-actual-size")
                            .toggled(model.actual)
                            .tip(ui::tip(t("Actual pixels")).shortcut("view.actualSize", cx))
                            .on_click(cx.listener(|this, _, _, cx| this.actual(cx))),
                    ),
            );
        }
        row.into_any_element()
    }

    /// The four decisions on the pair, Smart merge first: with their words
    /// or, where the titlebar is short, as glyphs named by their tooltips.
    fn decisions(&self, words: bool, cx: &mut Context<Self>) -> Div {
        let button = |id: &'static str,
                      look: Look,
                      icon: &'static str,
                      label: &'static str,
                      about: &'static str,
                      shortcut: &'static str| {
            ui::titlebar_button(id, look, icon, label, words)
                .tip(ui::tip(if words { about } else { label }).shortcut(shortcut, cx))
                .disabled(self.resolving)
        };
        // Keeps the better file of the two and merges the other into it.
        // Under the pointer or with the keyboard, it shows which.
        let (hover, focus) = (cx.weak_entity(), cx.weak_entity());
        let smart_merge = button(
            "duplicates-smart-merge",
            Look::Primary,
            "action-arrows-join",
            t("Smart merge"),
            t("Keep the stronger file and preserve item metadata"),
            "dup.smartMerge",
        )
        .on_click(cx.listener(|this, _, window, cx| this.decide(Decision::Smart, window, cx)))
        .on_hover(move |hovered, _, cx| {
            let _ = hover.update(cx, |this, cx| {
                this.smart_hovered = hovered;
                cx.notify();
            });
        })
        .on_focus(move |focused, window, cx| {
            let _ = focus.update(cx, |this, cx| {
                this.smart_focused = focused;
                cx.notify();
            });
            window.request_animation_frame();
        });
        let not_duplicates = button(
            "duplicates-not",
            Look::Standard,
            "action-x",
            t("Not duplicates"),
            t("These are different media"),
            "dup.notDuplicate",
        )
        .on_click(cx.listener(|this, _, window, cx| this.keep_both(window, cx)));
        let keep_both = button(
            "duplicates-keep-both",
            Look::Standard,
            "action-copy",
            t("Keep both"),
            t("Keep both files"),
            "dup.keepBoth",
        )
        .on_click(cx.listener(|this, _, window, cx| this.keep_both(window, cx)));
        let (hover, focus) = (cx.weak_entity(), cx.weak_entity());
        let difference = button(
            "duplicates-difference",
            Look::Standard,
            "toolbar-difference",
            t("Show Difference"),
            t("Show differences while held"),
            "dup.difference",
        )
        // A press gives the button the keyboard in the Electron app, so the
        // difference stays until a press elsewhere.
        .on_click(cx.listener(|this, _, _, cx| {
            this.difference_pressed = true;
            cx.notify();
        }))
        .on_hover(move |hovered, _, cx| {
            let _ = hover.update(cx, |this, cx| {
                this.difference_hovered = hovered;
                cx.notify();
            });
        })
        .on_focus(move |focused, window, cx| {
            let _ = focus.update(cx, |this, cx| {
                this.difference_focused = focused;
                cx.notify();
            });
            window.request_animation_frame();
        });
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(ui::titlebar_button::BETWEEN))
            .child(smart_merge)
            .child(not_duplicates)
            .child(keep_both)
            .child(difference)
    }

    /// A key came up: the difference's own ends the difference it held.
    fn key_released(&mut self, event: &KeyUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = window
            .bindings_for_action_in(&ShowDifference, &self.focus)
            .iter()
            .filter_map(|binding| {
                binding
                    .keystrokes()
                    .last()
                    .map(|stroke| stroke.inner().key.clone())
            })
            .collect::<Vec<_>>();
        if keys.is_empty() || keys.contains(&event.keystroke.key) {
            self.hold_difference(false, cx);
        }
    }
}

impl Render for DuplicatesScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let mut root = div()
            .id("duplicates")
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .relative()
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(theme.background())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousPair, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &NextPair, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &FitPair, _, cx| this.fit(cx)))
            .on_action(cx.listener(|this, _: &ActualPixels, _, cx| this.actual(cx)))
            .on_action(cx.listener(|this, _: &KeepBoth, window, cx| this.keep_both(window, cx)))
            .on_action(cx.listener(|this, _: &NotDuplicate, window, cx| this.keep_both(window, cx)))
            .on_action(cx.listener(|this, _: &KeepLeft, window, cx| this.keep(0, window, cx)))
            .on_action(cx.listener(|this, _: &KeepRight, window, cx| this.keep(1, window, cx)))
            .on_action(cx.listener(|this, _: &SmartMerge, window, cx| {
                this.decide(Decision::Smart, window, cx)
            }))
            // The difference shows from its key going down until that key
            // comes up.
            .on_action(
                cx.listener(|this, _: &ShowDifference, _, cx| this.hold_difference(true, cx)),
            )
            .on_key_up(cx.listener(Self::key_released));
        if self.shown.is_none() {
            return root.child(if self.loading {
                self.loading_state(cx).into_any_element()
            } else {
                self.empty_state(cx)
            });
        }
        // A drag follows the pointer anywhere in the window until the
        // button is released, as pointer capture does; and a press anywhere
        // ends the difference a press on its button began.
        if self.drag.is_some() || self.difference_pressed {
            let entity = cx.weak_entity();
            root = root.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let moved = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ =
                                    moved.update(cx, |this, cx| this.dragged(event.position, cx));
                            }
                        });
                        let released = entity.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = released.update(cx, |this, cx| {
                                    if this.drag.take().is_some() {
                                        cx.notify();
                                    }
                                });
                            }
                        });
                        let pressed = entity.clone();
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = pressed.update(cx, |this, cx| {
                                    if std::mem::take(&mut this.difference_pressed) {
                                        cx.notify();
                                    }
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        }
        // `mergePreviewActive`: the side Smart merge would keep, while it
        // is under the pointer or has the keyboard.
        let previewed = self.merge_previewed();
        if previewed.is_some() {
            self.preview_side = previewed;
        }
        self.survivor
            .set(previewed.is_some(), motion::MERGE_PREVIEW);
        if self.survivor.animating() {
            window.request_animation_frame();
        }
        let facts = facts(self.shown.as_ref().expect("a pair is on screen"));
        let files = div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(MARGIN))
            .p(px(MARGIN))
            .overflow_hidden()
            .child(self.side(0, &facts, window, cx))
            .child(self.side(1, &facts, window, cx));
        let menu = self.menu.layer(window, cx);
        root.child(files)
            .children(self.scan_progress.then(|| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .child(ui::progress_bar().height(2.))
            }))
            .children(menu)
    }
}

impl Render for DuplicatesToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = div().flex_1().min_w_0().h_full();
        let shown = self
            .screen
            .upgrade()
            .is_some_and(|screen| screen.read(cx).toolbar_model().is_some());
        if !shown {
            return bar;
        }
        // The row is the screen's, built for this width as it is laid out.
        let screen = self.screen.clone();
        bar.child(
            ui::sized(move |bounds, window, cx| {
                screen
                    .update(cx, |screen, cx| screen.titlebar(bounds, window, cx))
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .size_full(),
        )
    }
}

/// How wide each part of the titlebar's row is.
struct Widths {
    arrows: f32,
    place: f32,
    similarity: f32,
    labelled: f32,
    glyphs: f32,
    zoom: f32,
}

/// What of the titlebar's row is shown in the room it has.
struct Fit {
    /// The decisions carry their words.
    words: bool,
    /// The room for the similarity and what was counted, when they show.
    measured: Option<f32>,
    place: bool,
    zoom: bool,
    arrows: bool,
}

/// What a titlebar with `room` after the title shows: everything, or as
/// much as fits, given up in one order.
fn fit(room: f32, widths: Widths) -> Fit {
    let rest = |fit: &Fit| {
        let mut left = if fit.arrows { widths.arrows } else { 0. };
        if fit.place {
            left += widths.place;
        }
        let mut right = if fit.words {
            widths.labelled
        } else {
            widths.glyphs
        };
        if fit.zoom {
            right += GROUP_GAP + widths.zoom;
        }
        room - left - GROUP_GAP - right
    };
    let mut fit = Fit {
        words: true,
        measured: None,
        place: true,
        zoom: true,
        arrows: true,
    };
    // The similarity whole, with or without the decisions' words.
    let needed = GROUP_GAP + widths.similarity;
    if rest(&fit) < needed {
        fit.words = false;
    }
    let left = rest(&fit);
    if left >= needed {
        fit.measured = Some(left - GROUP_GAP);
        return fit;
    }
    for part in [
        |fit: &mut Fit| fit.place = false,
        |fit: &mut Fit| fit.zoom = false,
        |fit: &mut Fit| fit.arrows = false,
    ] {
        if rest(&fit) >= 0. {
            break;
        }
        part(&mut fit);
    }
    fit
}
