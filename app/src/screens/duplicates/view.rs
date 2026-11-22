//! Draws the Duplicates screen: the loading state, the empty state, the row
//! of decision buttons, the row with the similarity, the two files side by
//! side, and the screen's controls in the titlebar.
//!
//! Each file is drawn as a column. From top to bottom the column has a row
//! with the file's name and its Keep left or Keep right button, the
//! picture, and the list of the file's facts.
//!
//! Nothing here measures a size in one frame and uses it in the next frame.
//! The columns are laid out with flex. A picture needs the size of its pane
//! to be placed, and the titlebar's row needs its own width to decide what
//! fits, so both are built with `ui::sized`, which builds an element from
//! its bounds in the frame that lays it out. The screen is therefore drawn
//! correctly in every frame while the window is being resized.
use super::{
    ActualPixels, CONTEXT, Counted, Decision, Drag, DuplicatesScreen, DuplicatesToolbar, FitPair,
    KeepBoth, KeepLeft, KeepRight, NextPair, NotDuplicate, PreviousPair, ShowDifference, Shown,
    Side, SmartMerge, compare, extent, pane_edge,
};
use crate::grid::layout::SELECTION_EXTENT;
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::ui::property_row::RowColor;
use crate::{theme, ui};
use gpui::*;
use picto_library::{Rating, RootKind};
use std::sync::Arc;

/// The space in pixels around the two columns and between them. The
/// inspector uses the same 16 px as its side padding.
const EDGE: f32 = 16.;
/// The height in pixels of the row above each picture, which holds the
/// file's name and its Keep button. It is the height of the Keep button.
const NAME_ROW: f32 = theme::CONTROL_MD;
/// The space in pixels between the name row and the picture, and between
/// the picture and the list of facts.
const AROUND_PICTURE: f32 = 8.;
/// The smallest height in pixels that a picture's pane gets when the window
/// is short.
const PANE_MIN: f32 = 120.;
/// The largest width in pixels of a file's list of facts. The left file's
/// list is placed against the right edge of its column and the right file's
/// list against the left edge of its column, so the two lists are next to
/// each other in the middle of the screen.
const FACTS_WIDTH: f32 = 360.;
/// The height in pixels of one row of the list of facts (`ui::property_row`).
const ROW: f32 = 20.;
/// The number of rows that the list of facts reserves space for. A pair has
/// 8 rows, or 9 when one of its files is in a collection. Reserving 9 keeps
/// the pictures the same size for every pair.
const ROWS: f32 = 9.;
/// The distance in pixels from the left edge of the titlebar's row of
/// controls to the left edge of the screen's title. The toolbar draws the
/// title 80 px from its own left edge. The row of controls starts after the
/// toolbar's 8 px of padding and its Back and Forward buttons.
const TITLE_START: f32 = 80. - 8. - 2. * theme::ICON_BUTTON;
/// The space in pixels that is kept empty between the screen's title and
/// the first control after it.
const TITLE_CLEAR: f32 = 16.;
/// The space in pixels between the groups of controls in the titlebar.
const GROUP_GAP: f32 = 12.;
/// The height in pixels of the row under the titlebar that holds the four
/// decision buttons.
const DECISION_ROW: f32 = 44.;
/// The height in pixels of the row under the decision buttons, which shows
/// the similarity and the pixel measurement.
const SIMILARITY_ROW: f32 = 20.;
/// The space in pixels between the border of a file's panel and what is
/// inside it.
const PANEL_PADDING: f32 = 12.;

/// One fact about the two files: its label and its value for the left and
/// for the right file.
struct Fact {
    label: &'static str,
    values: [String; 2],
}

/// Writes a rating as text: "Unrated", or one star for each point of the
/// rating.
fn rating_text(rating: Rating) -> String {
    match rating {
        Rating::Unrated => t("Unrated").to_owned(),
        rating => "★".repeat(rating.bitmap_key() as usize),
    }
}

/// The facts of the two files, in the order in which the screen lists
/// them: resolution, size, format, rating, number of tags, creation date,
/// import date, collection and number of occurrences. The labels and
/// the formats are the inspector's (Dimensions, Type, Date Created, Date
/// Imported). Both files always have the same rows, so the same fact is at
/// the same height in both lists.
fn facts(shown: &Shown) -> Vec<Fact> {
    let each = |value: &dyn Fn(&Side) -> String| shown.sides.each_ref().map(value);
    // A date in the inspector's format, `YYYY/MM/DD HH:MM`, in local time.
    let date = |at: Option<i64>| {
        use chrono::TimeZone;
        at.filter(|at| *at != 0)
            .and_then(|at| chrono::Local.timestamp_millis_opt(at).single())
            .map(|at| at.format(crate::text::date_format::INSPECTOR).to_string())
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
    // The name of the collection that holds the file. The row is shown for
    // both files when either file is in a collection, and the value of a
    // file that is in no collection is "None".
    let group = |side: &Side| {
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
    /// The screen while the queue is being read for the first time. It is
    /// empty for the first 200 ms and then shows a message.
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

    /// The screen when the queue has no pairs, or when it could not be
    /// read. It has a button that scans the library.
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
        // The scan's progress bar is 180 px wide and 2 px high. Its space
        // is reserved while the bar is not shown, so the button above it
        // does not move when the bar appears.
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

    /// The column of one file. `index` is 0 for the left file and 1 for the
    /// right file.
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
        // The locale files have one key for the tooltip of both buttons,
        // "Keep {value0}", and one for the label of both buttons, "Keep ".
        // The word "left" or "right" that is put into them is English in
        // every language.
        let which = if index == 0 { "left" } else { "right" };
        let keep = ui::action_button(("duplicate-keep", index), Look::Standard)
            .icon("check", 15.)
            .label(format!("{}{which}", t("Keep ")))
            .height(NAME_ROW)
            .padding(10.)
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
        // The row above the picture: the file's name on the left and the
        // Keep button on the right. A name that does not fit is cut off
        // with an ellipsis, and the tooltip shows the whole name.
        let name_row = div()
            .w_full()
            .h(px(NAME_ROW))
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
                NAME_ROW,
                window,
                cx,
            ))
            .child(keep);
        // A row whose value is the same for both files is drawn in the
        // secondary text colour. A row whose value differs is drawn in the
        // primary text colour for both files, so the rows that differ are
        // the brightest text in the list.
        let rows = facts.iter().map(|fact| {
            ui::property_row(fact.label, fact.values[index].clone()).color(
                if fact.values[0] == fact.values[1] {
                    RowColor::Secondary
                } else {
                    RowColor::Primary
                },
            )
        });
        // Assistive technology reads the list as one group that has the
        // file's name.
        let list = div()
            .id(("duplicate-facts", index))
            .role(Role::Group)
            .aria_label(name)
            .w_full()
            .max_w(px(FACTS_WIDTH))
            .h(px(ROW * ROWS))
            .flex_none()
            .flex()
            .flex_col()
            .children(rows);
        // Each file is in its own panel, drawn like the content panel of the
        // Settings window: rounded corners, a faint border and a slightly
        // lighter background than the screen. The two panels show which
        // name, picture and facts belong to which file.
        let column = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .p(px(PANEL_PADDING))
            .rounded(px(theme::RADIUS_LG))
            .border_1()
            .border_color(theme.contrast(5.))
            .bg(theme.rail_raised())
            .flex()
            .flex_col()
            .gap(px(AROUND_PICTURE))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_file_menu(index, event.position, window, cx);
                }),
            )
            .child(name_row)
            .child(self.pane(index, cx))
            .child(list);
        if index == 0 {
            column.items_end()
        } else {
            column.items_start()
        }
    }

    /// Opens a file's context menu at `position`. It has two rows: one
    /// opens the file in its default application, and one opens it in a
    /// detail window.
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
                    // The file is opened from the path at which the library
                    // stores it.
                    if let Some(media) = &side.media {
                        crate::desktop::open_default(std::path::Path::new(&media.file_path), cx);
                    }
                } else if key == rows::Action::OpenInNewWindow.key() {
                    // The detail window shows this one file. It has no
                    // other files to step to.
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

    /// The pane of one picture. It takes all the height of the column that
    /// the name row and the list of facts leave. It has no border and no
    /// background of its own, so nothing is drawn around the picture.
    fn pane(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let entity = cx.weak_entity();
        let dragging = self.drag.as_ref().is_some_and(|drag| drag.side == index);
        div()
            .id(("duplicate-pane", index))
            .relative()
            .w_full()
            .flex_1()
            .min_h(px(PANE_MIN))
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
                        // A double click zooms both pictures to fit.
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

    /// Builds what a pane shows. `ui::sized` calls this with the pane's
    /// bounds while the pane is being laid out, so the picture is placed
    /// for the pane's size in the same frame.
    ///
    /// The pane shows the file's thumbnail, with the file's original drawn
    /// over it as soon as the original is decoded. While the difference is
    /// shown, the pane shows the difference picture instead. The picture
    /// that Smart merge would keep gets the grid's selection outline while
    /// the pointer is on the Smart merge button or the button has keyboard
    /// focus.
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
        // The rectangle of the picture inside the pane, as the zoom places
        // it. It is `None` when the library does not know the file's size
        // in pixels, and the picture then fills the pane.
        let frame = (pane.width > 0. && pane.height > 0. && image.width > 0. && image.height > 0.)
            .then(|| self.zoom.frame(pane, image, pane_edge(index)));
        let place = |layer: Div| match frame {
            Some(frame) => layer
                .absolute()
                .left(px(frame.left))
                .top(px(frame.top))
                .w(px(frame.width))
                .h(px(frame.height)),
            None => layer.absolute().inset_0(),
        };
        let fill = |image: Arc<RenderImage>, fit: ObjectFit| {
            img(image).absolute().inset_0().size_full().object_fit(fit)
        };
        // Everything in this element is clipped to the pane, so a picture
        // that is zoomed in does not cover the other picture.
        let mut clipped = div()
            .absolute()
            .inset_0()
            .overflow_hidden()
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
            // The difference picture is drawn on black over the rectangle of
            // the picture.
            clipped = clipped.child(
                place(div().bg(rgb(theme::DIFFERENCE_BACKGROUND))).children(
                    shown
                        .difference
                        .clone()
                        .map(|(image, _)| fill(image, ObjectFit::Fill)),
                ),
            );
        } else {
            let (thumbnail, original) = self.pictures_of(side);
            let opacity = if original.is_some() {
                side.original_opacity()
            } else {
                0.
            };
            // While the original is fading in over the thumbnail, the
            // screen asks for another frame.
            if original.is_some() && opacity < 1. {
                window.request_animation_frame();
            }
            let mut layers = place(div());
            if opacity < 1.
                && let Some(thumbnail) = thumbnail
            {
                layers = layers.child(fill(thumbnail, ObjectFit::Contain));
            }
            if let Some(original) = original {
                layers = layers.child(fill(original, ObjectFit::Contain).opacity(opacity));
            }
            clipped = clipped.child(layers);
        }
        // The outline is the one the grid draws around a selected tile: a
        // 2 px line in the grid's selection colour, `SELECTION_EXTENT`
        // pixels outside the picture. It is drawn around the part of the
        // picture that is inside the pane, and it is not clipped to the
        // pane.
        let outline = (self.outlined_side() == Some(index)).then(|| {
            let (left, top, right, bottom) = match frame {
                Some(frame) => (
                    frame.left.max(0.),
                    frame.top.max(0.),
                    (frame.left + frame.width).min(pane.width),
                    (frame.top + frame.height).min(pane.height),
                ),
                None => (0., 0., pane.width, pane.height),
            };
            div()
                .absolute()
                .left(px(left - SELECTION_EXTENT))
                .top(px(top - SELECTION_EXTENT))
                .w(px(right - left + 2. * SELECTION_EXTENT))
                .h(px(bottom - top + 2. * SELECTION_EXTENT))
                .rounded(px(theme::TILE_RADIUS + SELECTION_EXTENT))
                .border_2()
                .border_color(rgb(theme::GRID_SELECTION))
        });
        div()
            .relative()
            .size_full()
            .child(clipped)
            .children(outline)
            .into_any_element()
    }

    /// Builds the screen's row of controls in the titlebar. `ui::sized`
    /// calls this with the row's bounds while the titlebar is being laid
    /// out, so the row is built for its width in the same frame.
    ///
    /// The row has three parts. At its left is empty space under the
    /// screen's title, which the toolbar draws. Centred in the row are the
    /// Previous pair button, the position in the queue (for example
    /// "1 / 500") and the Next pair button. At its right end are the
    /// Zoom to fit and Actual pixels buttons.
    ///
    /// The Smart merge, Not duplicates, Keep both and Show Difference
    /// buttons are in a row of their own under the titlebar
    /// ([`Self::decision_row`]). The similarity and the pixel measurement
    /// are in the row under those buttons ([`Self::similarity_row`]).
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
        let width_of = |text: &str, size: f32, window: &Window| {
            f32::from(
                ui::shape_line(
                    text.to_owned(),
                    font(theme::FONT_UI),
                    size,
                    theme.text(),
                    window,
                )
                .width,
            )
            .ceil()
        };
        // The width of the empty space at the start of the row. The
        // toolbar draws the screen's title there.
        let lead = TITLE_START + width_of(t("Duplicates"), theme::TEXT_MD, window) + TITLE_CLEAR;
        // The position is given the width of its widest text, for example
        // "500 / 500", so the Next pair button does not move when the
        // number of digits changes.
        let place = format!("{} / {}", model.index + 1, model.total);
        let place_width =
            width_of(&format!("{0} / {0}", model.total), theme::TEXT_SM, window) + 12.;
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
        let row_width = f32::from(bounds.size.width);
        let zoom_width = 2. * theme::ICON_BUTTON + ui::titlebar_button::BETWEEN;
        // The Previous pair button, the position and the Next pair button
        // are one group.
        let group_width = 2. * theme::ICON_BUTTON + place_width;
        // When the row is too narrow for everything, the zoom buttons are
        // removed first and the group after them.
        let zoom_shown = row_width - lead >= group_width + GROUP_GAP + zoom_width;
        let group_shown = row_width - lead >= group_width;
        // The group is centred in the titlebar's row. The row does not move
        // or change width when the sidebar or the inspector is hidden or
        // shown, so the group stays where it is. If the row is so narrow
        // that the centred group would cover the screen's title or the zoom
        // buttons, the group is placed directly after the title.
        let right_limit = row_width
            - if zoom_shown {
                zoom_width + GROUP_GAP
            } else {
                0.
            };
        let centred = (row_width - group_width) / 2.;
        let group_left = if centred >= lead && centred + group_width <= right_limit {
            centred
        } else {
            lead
        };
        let mut row = div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            .overflow_hidden();
        if group_shown {
            row = row.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(group_left))
                    .flex()
                    .items_center()
                    .child(step(
                        "duplicates-previous",
                        "toolbar-chevron-left",
                        t("Previous pair"),
                        "dup.prevPair",
                        -1,
                        model.can_previous,
                    ))
                    .child(
                        div()
                            .w(px(place_width))
                            .flex_none()
                            .text_center()
                            .whitespace_nowrap()
                            .text(theme::TEXT_SM)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary())
                            .child(place),
                    )
                    .child(step(
                        "duplicates-next",
                        "toolbar-chevron-right",
                        t("Next pair"),
                        "dup.nextPair",
                        1,
                        model.can_next,
                    )),
            );
        }
        row = row.child(div().flex_1());
        if zoom_shown {
            row = row.child(
                div()
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

    /// The row under the decision buttons. It shows one centred line of
    /// text: how similar the two pictures are (for example "100% similar")
    /// and, after it in the secondary text colour, what the comparison of
    /// their pixels found (for example "0% of pixels differ"). The second
    /// part is shown once the comparison has finished, and it does not
    /// change while the pair is on screen. The row has no line above or
    /// below it.
    fn similarity_row(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let shown = self.shown.as_ref().expect("a pair is on screen");
        let counted = match shown.counted {
            Counted::Done(measure) => {
                let (counted, more) = measure.sentence();
                Some(match more {
                    Some(more) => format!("{counted} · {more}"),
                    None => counted,
                })
            }
            Counted::Impossible | Counted::Waiting => None,
        };
        div()
            .flex_none()
            .h(px(SIMILARITY_ROW))
            .px(px(EDGE))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text(theme::TEXT_SM)
            .child(ui::tooltip::attach(
                div()
                    .id("duplicates-similarity")
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text())
                    .child(compare::similarity_label(shown.distance)),
                "duplicates-similarity-tip",
                ui::tip(t("Perceptual similarity is not pixel equality")),
                20.,
                window,
                cx,
            ))
            .children(counted.map(|counted| {
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.text_secondary())
                    .child(format!("· {counted}"))
            }))
    }

    /// The row under the titlebar that holds the four decision buttons,
    /// each with its icon and its full label, centred in the row. The row
    /// is as wide as the screen and has no line under it.
    ///
    /// The buttons show their labels at every width at which the four
    /// labelled buttons fit in the row. Only when the row is narrower than
    /// that (a very narrow window, or a large interface zoom) do all four
    /// show their icons alone, so that none is cut off.
    fn decision_row(&mut self, cx: &mut Context<Self>) -> Div {
        // Whether the labels fit depends on the row's width, so the buttons
        // are built for the row's bounds in the frame that lays the row
        // out (`ui::sized_view`).
        let buttons = ui::sized_view(cx.weak_entity(), |this, bounds, window, cx| {
            // The width of the four buttons with their labels depends on
            // the language. It is measured here by laying the four buttons
            // out once without drawing them.
            let labelled = f32::from(
                this.decisions(true, cx)
                    .into_any_element()
                    .layout_as_root(
                        size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                        window,
                        cx,
                    )
                    .width,
            );
            let labels = labelled <= f32::from(bounds.size.width) - 2. * EDGE;
            div()
                .size_full()
                .px(px(EDGE))
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .child(this.decisions(labels, cx))
                .into_any_element()
        });
        div()
            .flex_none()
            .h(px(DECISION_ROW))
            .child(buttons.size_full())
    }

    /// The four buttons that make a decision on the pair or show the
    /// difference, in this order: Smart merge, Not duplicates, Keep both,
    /// Show Difference. With `labels` each button shows its icon and its
    /// label, and its tooltip describes what it does. Without `labels` each
    /// button shows only its icon, and its tooltip is its label. In both
    /// cases the tooltip also shows the button's key.
    fn decisions(&self, labels: bool, cx: &mut Context<Self>) -> Div {
        let button = |id: &'static str,
                      look: Look,
                      icon: &'static str,
                      label: &'static str,
                      about: &'static str,
                      shortcut: &'static str| {
            ui::titlebar_button(id, look, icon, label, labels)
                .tip(ui::tip(if labels { about } else { label }).shortcut(shortcut, cx))
                .disabled(self.resolving)
        };
        // Smart merge keeps the file that the library rates as better and
        // merges the other file's item into its item. The screen records
        // whether the pointer is on the button and whether the button has
        // keyboard focus, because the picture that would be kept gets an
        // outline in both cases.
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
            // The button reports a focus change while a frame is being
            // drawn. The screen needs one more frame to draw the outline.
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
        // After a click on the button the difference stays on screen until
        // the next press of a mouse button anywhere in the window. This
        // flag records the click.
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

    /// Runs when a key is released. If it is the key bound to Show
    /// Difference, the screen stops showing the difference that the key
    /// started. If no key is bound to Show Difference, any key does.
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
            // Pressing the Show Difference key shows the difference, and
            // releasing that key stops showing it.
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
        // While a picture is being dragged, the screen listens to the
        // pointer in the whole window until the button is released, so the
        // drag goes on when the pointer leaves the picture's pane. While
        // the difference is shown because of a click on Show Difference,
        // the next press of a mouse button anywhere in the window stops
        // showing it.
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
        let facts = facts(self.shown.as_ref().expect("a pair is on screen"));
        let files = div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(EDGE))
            .p(px(EDGE))
            .overflow_hidden()
            .child(self.side(0, &facts, window, cx))
            .child(self.side(1, &facts, window, cx));
        let menu = self.menu.layer(window, cx);
        let decisions = self.decision_row(cx);
        let similarity = self.similarity_row(window, cx);
        root.child(decisions)
            .child(similarity)
            .child(files)
            // The scan's progress bar is drawn over the top edge of the
            // screen, so it does not move the pictures.
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
        // The screen builds the row, because the row's buttons call the
        // screen's methods. The row is built from its own bounds while it
        // is laid out.
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
