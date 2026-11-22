//! The two functions that build the content of the grid view.
//!
//! `GridView::body` builds what fills the grid's area. That is one of four
//! things: the scrolling list of tiles, an error message with a Retry
//! button, an empty area while the first page of results is loading, or
//! the empty state when there are no items to show.
//!
//! `GridView::overlays` adds what is drawn over that area or listens for
//! events over it: the listeners that follow the pointer anywhere in the
//! window during a rubber band selection or a drag, the listener that zooms
//! the grid with the mouse wheel, the picture of the dragged items that
//! follows the pointer, and the hover preview.
use super::prefs::ViewPrefs;
use super::tile::TileData;
use super::{EmptyMenuRequested, GridView, Surface, capture_pointer, drag, pointer, selection};
use crate::i18n::t;
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder;
use gpui::*;

impl GridView {
    /// Builds what fills the grid's area for `surface`: the tiles, the
    /// error message, the empty area shown while loading, or the empty
    /// state. The tiles, the error message and the empty state are drawn at
    /// the opacity `fade`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn body(
        &mut self,
        surface: Surface,
        tiles: Vec<TileData>,
        prefs: &ViewPrefs,
        fade: f32,
        now: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        match surface {
            Surface::Error(error) => div()
                .id("grid-error")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .opacity(fade)
                .text(theme::TEXT_MD)
                .text_color(theme.negative())
                .child(error)
                .child(
                    div()
                        .id("grid-retry")
                        .role(Role::Button)
                        .aria_label(t("Retry"))
                        .px(px(12.))
                        .py(px(4.))
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(theme.border())
                        .text(theme::TEXT_SM)
                        .text_color(theme.text_secondary())
                        .cursor_pointer()
                        .hover(move |style| {
                            style.text_color(theme.text()).bg(theme.surface_hover())
                        })
                        .child(t("Retry"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.update(cx, |session, cx| session.reload(cx));
                        })),
                ),
            Surface::Loading => div().id("grid-loading").size_full(),
            Surface::Empty(copy) => {
                let actions = if copy.import {
                    // The "Import Files" and "Import Folder" buttons open a
                    // file chooser or a folder chooser and import what is
                    // chosen into the destination shown. Both buttons are
                    // disabled while `imports::blocked` gives a reason
                    // that an import cannot start, such as an import that
                    // is running.
                    let destination = {
                        let session = self.session.read(cx);
                        session
                            .nav
                            .get(session.displayed)
                            .map(|entry| crate::imports::plan::Destination::for_scope(&entry.scope))
                            .unwrap_or_default()
                    };
                    let blocked = crate::imports::blocked(cx);
                    let start = move |folder: bool| {
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            if let Some(imports) = crate::imports::get(cx) {
                                imports.update(cx, |imports, cx| {
                                    if folder {
                                        imports.choose_folder(destination, window, cx)
                                    } else {
                                        imports.choose_files(destination, window, cx)
                                    }
                                });
                            }
                        }
                    };
                    vec![
                        ui::empty_state_action("grid-import-files", "upload", t("Import Files"))
                            .disabled(blocked.is_some())
                            .on_click(start(false))
                            .into_any_element(),
                        ui::empty_state_action(
                            "grid-import-folder",
                            "folder-plus",
                            t("Import Folder"),
                        )
                        .disabled(blocked.is_some())
                        .on_click(start(true))
                        .into_any_element(),
                    ]
                } else {
                    Vec::new()
                };
                div()
                    .id("grid-empty")
                    .size_full()
                    .opacity(fade)
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|_, event: &MouseDownEvent, _, cx| {
                            cx.emit(EmptyMenuRequested(event.position))
                        }),
                    )
                    .child(ui::empty_state(
                        "empty-photo",
                        copy.title,
                        copy.description,
                        actions,
                        cx,
                    ))
            }
            Surface::Tiles => {
                // `content` holds the tiles. It starts below the subfolder
                // header and is as tall as all of the destination's items
                // laid out (`placement.height`), not only the items that
                // are loaded. The scrollbar's size and position therefore
                // account for every item in the destination.
                let header = self.subfolders.height();
                let mut content = div()
                    .absolute()
                    .left_0()
                    .top(px(header))
                    .w_full()
                    .h(px(self.placement.height));
                let building = crate::stats::Started::now();
                for data in tiles {
                    content = content.child(self.tile(data, &prefs, now, window, cx));
                }
                crate::stats::grid_part(crate::stats::GridPart::Tiles, building);
                if let Some(marker) = self.reorder_marker(&prefs) {
                    content = content.child(marker);
                }
                // The rectangle of a rubber band selection is drawn over
                // the tiles. It has square corners, a fill at 12% opacity
                // and a 1 px border.
                if let Some(rect) = self.marquee.as_ref().and_then(|marquee| marquee.rect) {
                    content = content.child(
                        div()
                            .absolute()
                            .left(px(rect.left))
                            .top(px(rect.top + self.placement.top))
                            .w(px(rect.width))
                            .h(px(rect.height))
                            .bg(rgba(theme::MARQUEE_FILL))
                            .border_1()
                            .border_color(rgba(theme::MARQUEE_BORDER)),
                    );
                }
                // To assistive technology the scrolling list of tiles is a
                // list box in which more than one item can be selected.
                div()
                    .id("media-grid")
                    .role(Role::ListBox)
                    .aria_multiselectable(true)
                    .size_full()
                    .opacity(fade)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    // Each scroll event asks for the grid to be built again,
                    // so that the next frame is built for the new scroll
                    // position.
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        this.pointer_moved(event.position, cx)
                    }))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered {
                            this.pointer_left(cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.pressed(event, window, cx)
                        }),
                    )
                    // A tile's image stops a right-click on it from reaching
                    // this handler. A right-click that arrives here was
                    // therefore on empty space: between tiles, or on the
                    // text rows under an image, which count as empty space.
                    // It opens the grid's own menu.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.empty_menu(event.position, window, cx)
                        }),
                    )
                    // A tile's image also stops a click on it from reaching
                    // this handler, so a click that arrives here was on
                    // empty space. It clears the selection and gives the
                    // grid the keyboard focus. Two kinds of click are
                    // ignored. One is the click made by the release that
                    // ends a rubber band selection (`swallow_click`). The
                    // other is a secondary click (a Control-click on
                    // macOS), whose press has already opened the grid's
                    // menu.
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        if std::mem::take(&mut this.swallow_click)
                            || selection::Press::here(event.modifiers()) == selection::Press::Menu
                        {
                            return;
                        }
                        this.session
                            .update(cx, |session, cx| session.clear_selection(cx));
                        window.focus(&this.focus, cx);
                    }))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(header + self.placement.height))
                            .when(header > 0., |all| {
                                all.child(self.subfolder_header(window, cx))
                            })
                            .child(content),
                    )
            }
        }
    }

    /// Adds to `root` what is drawn over the grid's body or listens for
    /// events over it, and returns `root`:
    ///
    /// - While a rubber band selection is in progress, a listener that
    ///   follows the pointer and the button's release anywhere in the
    ///   window.
    /// - After a press on a tile and during a drag of items, a listener of
    ///   the same kind for the drag.
    /// - When `scrolls` is true, which is when the grid shows tiles, a
    ///   listener that zooms the grid when a scroll event arrives over the
    ///   tiles and the "Mouse wheel" setting is "Adjust thumbnail size".
    /// - During a drag, the picture of the dragged items that follows the
    ///   pointer (`drag::ghost`).
    /// - The hover preview, while one is shown.
    pub(super) fn overlays(
        &mut self,
        mut root: Div,
        scrolls: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // During a rubber band selection, and after a press on a tile
        // (which becomes a drag once the pointer has moved far enough), the
        // grid has to receive the pointer's moves and the button's release
        // even when the pointer is outside the grid. `capture_pointer` adds
        // a listener for the whole window that passes them on to the grid.
        if self.marquee.is_some() {
            root = root.child(capture_pointer(
                cx.entity().downgrade(),
                |this, position, _, cx| this.marquee_moved(position, cx),
                |this, window, cx| this.marquee_released(window, cx),
            ));
        }
        if self.press.is_some() || self.dragging {
            root = root.child(capture_pointer(
                cx.entity().downgrade(),
                |this, position, window, cx| this.drag_moved(position, window, cx),
                |this, _, cx| this.drag_released(cx),
            ));
        }
        // When "Mouse wheel" in Settings › Controls is set to "Adjust
        // thumbnail size", a scroll event over the tiles zooms the grid
        // instead of scrolling it. The setting is read each time a scroll
        // event arrives, so a change to it applies to the next event. The
        // listener handles the event in the capture phase and stops it, so
        // the list of tiles does not scroll as well.
        if scrolls {
            let entity = cx.entity().downgrade();
            root = root.child(
                canvas(
                    |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |_, hitbox, window, _| {
                        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                            if phase != DispatchPhase::Capture
                                || !hitbox.is_hovered_at(event.position, window)
                                || crate::settings::current(cx).grid_wheel_action != "zoom"
                            {
                                return;
                            }
                            // `wheel_zoom` takes a distance that is
                            // positive for scrolling down. GPUI's sign is
                            // the opposite, so the distance is negated. A
                            // mouse wheel reports lines and a trackpad
                            // reports pixels.
                            let (delta_y, notch) = match event.delta {
                                ScrollDelta::Lines(lines) => (-lines.y, true),
                                ScrollDelta::Pixels(pixels) => (-f32::from(pixels.y), false),
                            };
                            cx.stop_propagation();
                            let _ =
                                entity.update(cx, |this, cx| this.wheel_zoom(delta_y, notch, cx));
                        });
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        }
        if self.dragging
            && let Some(items) = drag::model(cx).read(cx).items.clone()
        {
            root = root.child(drag::ghost(&items, &self.thumbnails, cx));
        }
        root.children(pointer::preview_overlay(&self.preview, window))
    }
}
