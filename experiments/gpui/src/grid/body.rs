//! The grid's body: the tiles in their scrolling list, or the error,
//! loading or empty state in their place, and what is laid over it (the
//! pointer followed outside it, the wheel's zoom, the drag's ghost and the
//! hover preview).
use super::prefs::ViewPrefs;
use super::tile::TileData;
use super::{
    EmptyMenuRequested, GridView, Surface, capture_pointer, drag, nav, pointer, selection,
};
use crate::i18n::t;
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder;
use gpui::*;

impl GridView {
    /// What fills the grid's place, faded to `fade`.
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
                    // `chooseAndImportFiles` / `chooseAndImportFolder` into
                    // this destination, held while an import runs.
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
                // The whole destination's height, so the scrollbar stands
                // for every item, loaded or not.
                // The subfolder header, then the whole destination's height
                // below it.
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
                // `.marquee`: square corners, over the tiles.
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
                // The tiles' list: several can be selected.
                div()
                    .id("media-grid")
                    .role(Role::ListBox)
                    .aria_multiselectable(true)
                    .size_full()
                    .opacity(fade)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    // The frame that follows a scroll notices the new position.
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
                    // A right-click no image took is on empty space.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.empty_menu(event.position, window, cx)
                        }),
                    )
                    // A click that no image took is a click on empty space,
                    // unless it ends a rubber band or was a secondary click.
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

    /// What lies over the body: while a rubber band is out, or after a
    /// press on a tile, the pointer followed anywhere in the window; the
    /// wheel's zoom over tiles that `scrolls`; the drag's ghost; the hover
    /// preview.
    pub(super) fn overlays(
        &mut self,
        mut root: Div,
        scrolls: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // While a rubber band is out, or after a press on a tile (which can
        // become a drag), the pointer is followed and the release heard
        // anywhere in the window, as pointer capture does.
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
        // Settings › Controls "Mouse wheel: Zoom" (`onGridWheel`): the wheel
        // over the tiles changes the tile size instead of scrolling. The
        // setting is read when the wheel turns, so a change applies at once.
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
                            // The browser's `deltaY` is positive scrolling
                            // down; GPUI's is the other way round.
                            let delta_y =
                                -f32::from(event.delta.pixel_delta(window.line_height()).y);
                            cx.stop_propagation();
                            let _ = entity.update(cx, |this, cx| {
                                this.session.update(cx, |session, cx| {
                                    if let Some(size) = nav::wheel_zoom(session.tile, delta_y) {
                                        session.set_tile(size, cx);
                                    }
                                })
                            });
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
