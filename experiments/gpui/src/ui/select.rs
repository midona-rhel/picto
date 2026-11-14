use super::popover::{Dismissal, Flip, Placement, Side, floating, place};
use crate::theme;
use crate::theme::Text as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{cell::Cell, rc::Rc};

type ChangeHandler = Box<dyn Fn(&'static str, &mut Window, &mut App) + 'static>;

/// One choice of a select: the value it stands for, its label, and an
/// optional 14px glyph (turned a quarter left when `rotated`).
#[derive(Clone)]
pub struct Choice {
    pub value: &'static str,
    pub label: SharedString,
    pub icon: Option<(&'static str, bool)>,
}

pub fn choice(value: &'static str, label: impl Into<SharedString>) -> Choice {
    Choice {
        value,
        label: label.into(),
        icon: None,
    }
}

impl Choice {
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some((icon, false));
        self
    }
    /// The glyph turned -90°, as the Justified layout's is.
    pub fn rotated_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some((icon, true));
        self
    }
}

/// What a select keeps between frames.
struct State {
    open: bool,
    /// The row under the pointer, which starts at the current choice.
    highlighted: Option<usize>,
    /// Where the button was last painted, which places the menu.
    button: Rc<Cell<Bounds<Pixels>>>,
    /// In a pop-up menu, the list's own pop-up window, and whether it is on
    /// its way.
    list: Option<AnyWindowHandle>,
    opening: bool,
    /// The menu's list, which scrolls past seven choices.
    scroll: ScrollHandle,
}

/// `CmSelect.tsx`: a 26px button showing the current choice with a selector
/// glyph, as wide as its widest choice unless given a width, and a menu of
/// the choices 4px below it (above it when there is no room below), as wide
/// as the button. A press anywhere else closes the menu.
#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    value: SharedString,
    choices: Vec<Choice>,
    width: Option<f32>,
    on_change: Option<ChangeHandler>,
}

pub fn select(
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    choices: Vec<Choice>,
) -> Select {
    Select {
        id: id.into(),
        value: value.into(),
        choices,
        width: None,
        on_change: None,
    }
}

impl Select {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn on_change(
        mut self,
        handler: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

/// The open menu's layer (`--layer-menu`): above whatever holds the
/// select, dialogs included, as `CmSelect`'s portal is.
const LAYER: usize = super::layer::MENU;

/// `--control-height-sm`, the height of the button and of each choice.
const ROW: f32 = theme::CONTROL_SM;
/// `.drop`: 4px padding, rows 2px apart, a 1px border, at most seven rows.
const LIST_PADDING: f32 = 4.;
const LIST_GAP: f32 = 2.;
const LIST_MAX: f32 = VISIBLE as f32 * ROW + 6. * LIST_GAP + 12.;
/// The list keeps this far below (or above) the button.
const LIST_OFFSET: f32 = 4.;
/// `CmSelect`'s placement: as wide as the button, 4px below it, or above it
/// when the menu (as `CmSelect` measures it, 26px a choice and 8px, at
/// most 164px) does not fit above the window's last 8px and there is more
/// room above. It is never moved sideways.
fn placement(choices: usize) -> Placement {
    Placement::new(Side::Below, LIST_OFFSET, 8.)
        .flip(Flip::ToRoomierSide {
            needs: (choices as f32 * ROW + 8.).min(164.),
        })
        .unclamped()
}
/// Room around a pop-up list for its drawn shadow (`--shadow-panel`).
const SHADOW_ROOM: f32 = 40.;

/// The choices the menu shows before it scrolls.
const VISIBLE: usize = 7;

/// The list's height for this many choices.
fn list_height(choices: usize) -> f32 {
    let rows = choices as f32 * ROW + choices.saturating_sub(1) as f32 * LIST_GAP;
    (rows + 2. * LIST_PADDING + 2.).min(LIST_MAX)
}

/// A rectangle as `[x, y, width, height]`.
type Area = [f32; 4];

/// Where a select's list goes when it opens as a pop-up window of its own
/// (inside a pop-up menu): `button` and `area` (the display's usable part)
/// in display coordinates. The list is placed as in the window, the
/// display standing for the window, and moved sideways to stay on the
/// display. Returns the list's rectangle and the window's, which leaves
/// room for the shadow but not past the display.
fn pop_up_list(button: Area, choices: usize, area: Area) -> (Area, Area) {
    let [bx, by, bw, bh] = button;
    let [ax, ay, aw, ah] = area;
    let height = list_height(choices);
    let placed = place(
        Bounds::new(point(px(bx - ax), px(by - ay)), size(px(bw), px(bh))),
        size(bw, height),
        size(px(aw), px(ah)),
        placement(choices),
    );
    let y = ay + placed.y;
    let x = bx.min(ax + aw - bw).max(ax);
    let list = [x, y, bw, height];
    let left = (x - SHADOW_ROOM).max(ax);
    let top = (y - SHADOW_ROOM).max(ay);
    let right = (x + bw + SHADOW_ROOM).min(ax + aw);
    let bottom = (y + height + SHADOW_ROOM).min(ay + ah);
    (
        list,
        [left, top, (right - left).max(0.), (bottom - top).max(0.)],
    )
}

fn glyph(icon: Option<(&'static str, bool)>, color: Rgba) -> Option<AnyElement> {
    let (name, rotated) = icon?;
    let mut glyph = super::icon(name, 14., color);
    if rotated {
        glyph = glyph.with_transformation(Transformation::rotate(radians(
            -std::f32::consts::FRAC_PI_2,
        )));
    }
    Some(
        div()
            .w(px(20.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(glyph)
            .into_any_element(),
    )
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            open: false,
            highlighted: None,
            button: Rc::new(Cell::new(Bounds::default())),
            list: None,
            opening: false,
            scroll: ScrollHandle::new(),
        });
        let current = self
            .choices
            .iter()
            .position(|choice| choice.value == self.value.as_ref());
        let (open, highlighted, measured) = {
            let state = state.read(cx);
            (state.open, state.highlighted, state.button.clone())
        };
        let has_icons = self.choices.iter().any(|choice| choice.icon.is_some());

        // The label column holds every choice's label at no height, so the
        // button is as wide as the widest one.
        let mut label = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div().truncate().child(
                    current
                        .map(|at| self.choices[at].label.clone())
                        .unwrap_or_else(|| self.value.clone()),
                ),
            );
        if self.width.is_none() {
            for choice in &self.choices {
                label = label.child(
                    div()
                        .h_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(choice.label.clone()),
                );
            }
        }
        let toggle_state = state.clone();
        let mut button = div()
            .id(self.id.clone())
            .occlude()
            .relative()
            .h(px(ROW))
            .pl(px(12.))
            .pr(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme.select_surface())
            .border_1()
            .border_color(theme.border_secondary())
            .text_color(theme.text())
            .text(theme::TEXT_MD)
            .line_height(px(24.))
            .whitespace_nowrap()
            .cursor_pointer()
            .children(glyph(
                current.and_then(|at| self.choices[at].icon),
                theme.text(),
            ))
            .child(label)
            .child(
                div()
                    .w(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.5)
                    .child(super::icon("selector-2", 14., theme.text())),
            )
            // Records where the button is, for the menu. A move while open
            // (the page scrolled) draws again with the new place. The canvas
            // fills the button inside its 1px border.
            .child({
                let measured = measured.clone();
                let open = open;
                canvas(
                    move |bounds, window, _| {
                        let bounds = bounds.dilate(px(1.));
                        // The open list is placed from these bounds; while
                        // drawing, only a requested frame repaints it.
                        if measured.replace(bounds) != bounds && open {
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
            })
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                toggle_state.update(cx, |state, cx| {
                    state.open = !state.open;
                    state.highlighted = current;
                    cx.notify();
                });
            });
        if let Some(width) = self.width {
            button = button.w(px(width));
        }
        let handler = self.on_change.map(Rc::new);
        button = keys(
            button,
            &self.id,
            &state,
            self.choices.iter().map(|choice| choice.value).collect(),
            current,
            handler.clone(),
            window,
            cx,
        );
        if super::menu::is_pop_up(window, cx) {
            return pop_up(
                button,
                Listed {
                    choices: self.choices,
                    current,
                    has_icons,
                    state,
                    handler,
                },
                open,
                window,
                cx,
            );
        }
        if !open {
            return div().flex().child(button);
        }

        let bounds = measured.get();
        let choices = self.choices.len();
        let menu = list(
            &Listed {
                choices: self.choices,
                current,
                has_icons,
                state: state.clone(),
                handler,
            },
            highlighted,
            bounds.size.width,
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "menu".into()),
            window,
            cx,
        );
        let corner = place(
            bounds,
            size(f32::from(bounds.size.width), list_height(choices)),
            window.viewport_size(),
            placement(choices),
        );
        div().flex().child(button).child(
            floating(
                ElementId::NamedChild(std::sync::Arc::new(self.id), "floating".into()),
                LAYER,
            )
            .anchored(corner)
            // A press on the button itself is the button's to toggle.
            .dismiss(Dismissal::Outside(bounds), move |_, cx| {
                state.update(cx, |state, cx| {
                    state.open = false;
                    cx.notify();
                })
            })
            .child(menu),
        )
    }
}

/// Makes the button a stop with `CmSelect`'s keys: Space opens or closes
/// the list as a click does; Enter opens it, or takes the highlighted choice
/// and closes it; Up and Down open it at the current choice, then move the
/// highlight; Home and End open it at the first or last choice; Escape
/// closes it (and with it closed reaches the dialog); Tab closes it on its
/// way to the next stop. It draws no ring.
#[allow(clippy::too_many_arguments)]
fn keys(
    button: Stateful<Div>,
    id: &ElementId,
    state: &Entity<State>,
    values: Vec<&'static str>,
    current: Option<usize>,
    handler: Option<Rc<ChangeHandler>>,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    use super::focus::{self, Kind};
    let focus = focus::control(id, window, cx);
    let count = values.len();
    let at = current.unwrap_or(0);
    // Opens the list with `index` highlighted, or moves the highlight there.
    let highlight = {
        let state = state.clone();
        move |index: usize, cx: &mut App| {
            if count == 0 {
                return;
            }
            state.update(cx, |state, cx| {
                state.open = true;
                state.highlighted = Some(index.min(count - 1));
                cx.notify();
            });
        }
    };
    let close = {
        let state = state.clone();
        move |cx: &mut App| {
            if state.read(cx).open {
                state.update(cx, |state, cx| {
                    state.open = false;
                    cx.notify();
                });
            }
        }
    };
    let stepped = |delta: isize| {
        let state = state.clone();
        let highlight = highlight.clone();
        move |cx: &mut App| {
            let (open, highlighted) = {
                let state = state.read(cx);
                (state.open, state.highlighted)
            };
            if !open {
                highlight(at, cx);
            } else {
                let from = highlighted.unwrap_or(at) as isize;
                highlight((from + delta).max(0) as usize, cx);
            }
        }
    };
    let (up, down) = (stepped(-1), stepped(1));
    let (home, end) = (highlight.clone(), highlight.clone());
    let choose_state = state.clone();
    let choose_open = highlight.clone();
    let (cancel_state, cancel_close) = (state.clone(), close.clone());
    let (next_close, previous_close) = (close.clone(), close);
    let toggle_state = state.clone();
    let button = button
        .on_action(move |_: &focus::Up, _, cx| up(cx))
        .on_action(move |_: &focus::Down, _, cx| down(cx))
        .on_action(move |_: &focus::Home, _, cx| home(0, cx))
        .on_action(move |_: &focus::End, _, cx| end(count.saturating_sub(1), cx))
        .on_action(move |_: &focus::Choose, window, cx| {
            let (open, highlighted) = {
                let state = choose_state.read(cx);
                (state.open, state.highlighted)
            };
            if !open {
                choose_open(at, cx);
                return;
            }
            choose_state.update(cx, |state, cx| {
                state.open = false;
                cx.notify();
            });
            if let (Some(index), Some(handler)) = (highlighted, &handler)
                && let Some(value) = values.get(index)
            {
                handler(*value, window, cx);
            }
        })
        .on_action(move |_: &focus::Cancel, _, cx| {
            if cancel_state.read(cx).open {
                cancel_close(cx);
            } else {
                cx.propagate();
            }
        })
        .capture_action(move |_: &focus::Next, _, cx| next_close(cx))
        .capture_action(move |_: &focus::Previous, _, cx| previous_close(cx));
    focus.attach(button, Kind::Select, move |_, _, cx| {
        toggle_state.update(cx, |state, cx| {
            state.open = !state.open;
            state.highlighted = current;
            cx.notify();
        })
    })
}

/// What the list shows and whom it tells.
#[derive(Clone)]
struct Listed {
    choices: Vec<Choice>,
    current: Option<usize>,
    has_icons: bool,
    state: Entity<State>,
    handler: Option<Rc<ChangeHandler>>,
}

/// `.drop` with its rows (`.opt`), the current choice and the row under the
/// pointer lit. Choosing a row closes the list, and its pop-up window when
/// it has one.
fn list(
    listed: &Listed,
    highlighted: Option<usize>,
    width: Pixels,
    id: ElementId,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let theme = theme::theme(cx);
    let scroll = listed.state.read(cx).scroll.clone();
    // More than seven choices scroll, and the list gives the scrollbar's
    // gutter room beside it, as `.drop`'s `overflow-y: auto` does.
    let scrolls = listed.choices.len() > VISIBLE;
    let mut menu = div()
        .id(ElementId::NamedChild(
            std::sync::Arc::new(id.clone()),
            "list".into(),
        ))
        .w_full()
        // Seven rows, their gaps and the padding, inside the border.
        .max_h(px(LIST_MAX - 2.))
        .overflow_y_scroll()
        .track_scroll(&scroll)
        .when(scrolls, |list| list.mr(px(super::scrollbar::GUTTER)))
        .p(px(LIST_PADDING))
        .flex()
        .flex_col()
        .gap(px(LIST_GAP));
    let frame = div()
        .occlude()
        .relative()
        .w(width)
        .flex()
        .flex_col()
        .rounded(px(theme::RADIUS_MD))
        .bg(theme.select_surface())
        .border_1()
        .border_color(theme.border_secondary())
        .shadow(theme.panel_shadow())
        .font_family(theme::FONT_UI)
        .text(theme::TEXT_MD)
        .text_color(theme.text());
    for (index, choice) in listed.choices.iter().enumerate() {
        let lit = highlighted == Some(index) || listed.current == Some(index);
        let (hover_state, click_state) = (listed.state.clone(), listed.state.clone());
        let handler = listed.handler.clone();
        let value = choice.value;
        menu = menu.child(
            div()
                .id(index)
                .h(px(ROW))
                .flex_none()
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(theme::RADIUS_SM))
                // `--color-menu-hover` and the chosen row's
                // `--color-hover-overlay` are the same colour.
                .when(lit, |row| row.bg(theme.hover_overlay()))
                .whitespace_nowrap()
                .cursor_pointer()
                .when(listed.has_icons, |row| {
                    row.child(
                        glyph(choice.icon, theme.text())
                            .unwrap_or_else(|| div().w(px(20.)).flex_none().into_any_element()),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(choice.label.clone()),
                )
                .on_mouse_move(move |_, _, cx| {
                    hover_state.update(cx, |state, cx| {
                        if state.highlighted != Some(index) {
                            state.highlighted = Some(index);
                            cx.notify();
                        }
                    })
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    let pop_up = click_state.update(cx, |state, cx| {
                        state.open = false;
                        state.opening = false;
                        cx.notify();
                        state.list.take()
                    });
                    if let Some(handler) = &handler {
                        handler(value, window, cx);
                    }
                    if let Some(pop_up) = pop_up {
                        close_list(pop_up, cx);
                    }
                }),
        );
    }
    frame.child(menu).when(scrolls, |frame| {
        frame.child(super::scrollbar::scrollbar(
            ElementId::NamedChild(std::sync::Arc::new(id), "scrollbar".into()),
            &scroll,
            window,
            cx,
        ))
    })
}

fn close_list(list: AnyWindowHandle, cx: &mut App) {
    super::menu::release(list, cx);
    cx.defer(move |cx| {
        let _ = list.update(cx, |_, window, _| window.remove_window());
    });
}

/// A select inside a pop-up menu's window, which cannot draw past its own
/// small window: its list opens as a pop-up window of its own at the button,
/// on the same display, and closes with the menu. A press elsewhere in the
/// menu's window closes it, as a press outside the list does in the window.
fn pop_up(
    button: Stateful<Div>,
    listed: Listed,
    open: bool,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let state = listed.state.clone();
    let (list_window, opening, measured) = {
        let state = state.read(cx);
        (state.list, state.opening, state.button.clone())
    };
    if !open {
        if let Some(list_window) = list_window {
            state.update(cx, |state, _| state.list = None);
            close_list(list_window, cx);
        }
        return div().flex().child(button);
    }
    if list_window.is_none()
        && !opening
        && let Some(display) = window.display(cx)
    {
        state.update(cx, |state, _| state.opening = true);
        // The pop-up's frame is its content: it has no titlebar.
        let frame = window.bounds();
        let content = window.viewport_size();
        let origin = (
            f32::from(frame.origin.x),
            f32::from(frame.origin.y) + f32::from(frame.size.height - content.height),
        );
        let bounds = measured.get();
        let area = display.visible_bounds();
        let (list_rect, window_rect) = pop_up_list(
            [
                origin.0 + f32::from(bounds.origin.x),
                origin.1 + f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ],
            listed.choices.len(),
            [
                f32::from(area.origin.x),
                f32::from(area.origin.y),
                f32::from(area.size.width),
                f32::from(area.size.height),
            ],
        );
        let owner = window.window_handle();
        let display = display.id();
        cx.defer(move |cx| {
            if !listed.state.read(cx).open {
                listed.state.update(cx, |state, _| state.opening = false);
                return;
            }
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(window_rect[0]), px(window_rect[1])),
                    size(px(window_rect[2]), px(window_rect[3])),
                ))),
                display_id: Some(display),
                titlebar: None,
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            };
            let state = listed.state.clone();
            let at = (list_rect[0] - window_rect[0], list_rect[1] - window_rect[1]);
            let width = list_rect[2];
            let opened = cx
                .open_window(options, move |window, cx| {
                    super::menu::without_system_shadow(window);
                    cx.new(|cx| PopUpList::new(listed, at, width, cx))
                })
                .ok()
                .map(AnyWindowHandle::from);
            state.update(cx, |state, cx| {
                state.opening = false;
                match opened {
                    Some(handle) if state.open => {
                        state.list = Some(handle);
                        cx.notify();
                    }
                    Some(handle) => {
                        cx.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, _| window.remove_window());
                        });
                    }
                    // No pop-up windows here: the list stays closed.
                    None => state.open = false,
                }
            });
            if let Some(handle) = opened {
                super::menu::adopt(owner, handle, cx);
            }
        });
    }
    let bounds = measured.get();
    div()
        .flex()
        .child(button)
        .on_mouse_down_out(move |event, _, cx| {
            // A press on the button itself is the button's to toggle.
            if bounds.contains(&event.position) {
                return;
            }
            state.update(cx, |state, cx| {
                state.open = false;
                cx.notify();
            });
        })
}

/// The root view of a select list's pop-up window: the list at its place in
/// the transparent window, following the select's highlight.
struct PopUpList {
    listed: Listed,
    at: (f32, f32),
    width: f32,
    _observe: Subscription,
}

impl PopUpList {
    fn new(listed: Listed, at: (f32, f32), width: f32, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&listed.state, |_, _, cx| cx.notify());
        Self {
            listed,
            at,
            width,
            _observe: observe,
        }
    }
}

impl Render for PopUpList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let highlighted = self.listed.state.read(cx).highlighted;
        div().size_full().child(
            div()
                .absolute()
                .left(px(self.at.0))
                .top(px(self.at.1))
                .child(list(
                    &self.listed,
                    highlighted,
                    px(self.width),
                    "select-pop-up-list".into(),
                    window,
                    cx,
                )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{list_height, pop_up_list};

    #[test]
    fn a_pop_up_list_sits_below_its_button_with_room_for_its_shadow() {
        let display = [0., 25., 1440., 875.];
        // Three choices: three rows, two gaps, the padding and the border.
        assert_eq!(list_height(3), 3. * 26. + 2. * 2. + 8. + 2.);
        let (list, window) = pop_up_list([500., 200., 130., 26.], 3, display);
        assert_eq!(list, [500., 230., 130., list_height(3)]);
        assert_eq!(window, [460., 190., 210., list_height(3) + 80.]);
        // Many choices stop at seven rows.
        assert_eq!(list_height(12), 7. * 26. + 6. * 2. + 12.);
    }

    #[test]
    fn a_pop_up_list_flips_above_and_stays_on_the_display() {
        let display = [0., 25., 1440., 875.];
        // Near the display's bottom it opens above, its bottom 4px over the
        // button, and the window stops at the display's edge.
        let (list, window) = pop_up_list([1380., 860., 130., 26.], 6, display);
        assert_eq!(list[1] + list[3], 856.);
        // Moved left to stay on the display.
        assert_eq!(list[0], 1310.);
        assert_eq!(window[0] + window[2], 1440.);
        assert_eq!(window[1] + window[3], list[1] + list[3] + 40.);
    }
}
