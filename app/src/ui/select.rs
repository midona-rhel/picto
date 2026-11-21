use super::menu::SHADOW_ROOM;
use super::popover::{Dismissal, Flip, Placement, Side, floating, place};
use crate::theme;
use crate::theme::Text as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{cell::Cell, rc::Rc};

type ChangeHandler = Box<dyn Fn(&'static str, &mut Window, &mut App) + 'static>;

/// One choice of a select: the value it stands for and its label. `icon` is
/// the name of an optional 14px glyph, and whether the glyph is drawn turned
/// a quarter turn to the left.
#[derive(Clone)]
pub struct Choice {
    pub value: &'static str,
    pub label: SharedString,
    pub icon: Option<(&'static str, bool)>,
}

/// Returns a choice with this value and label. The label is translated into
/// the interface's language here.
pub fn choice(value: &'static str, label: impl Into<SharedString>) -> Choice {
    Choice {
        value,
        label: crate::i18n::t_shared(label.into()),
        icon: None,
    }
}

impl Choice {
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some((icon, false));
        self
    }
    /// Sets the choice's glyph and draws it turned a quarter turn to the
    /// left. The choice for the Justified layout uses this.
    pub fn rotated_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some((icon, true));
        self
    }
}

/// Holds what a select keeps between frames.
struct State {
    open: bool,
    /// The highlighted row of the open list. It starts at the current choice
    /// and follows the pointer and the arrow keys.
    highlighted: Option<usize>,
    /// The bounds that the button was last painted at. The list is placed
    /// with them.
    button: Rc<Cell<Bounds<Pixels>>>,
    /// For a select inside a pop-up window of a menu: the pop-up window of
    /// the select's list (`list`), and whether that window is being opened
    /// (`opening`).
    list: Option<AnyWindowHandle>,
    opening: bool,
    /// The scroll position of the list, which scrolls when it has more than
    /// seven choices.
    scroll: ScrollHandle,
}

/// A select. It is a 26px button that shows the current choice and a
/// selector glyph. The button is as wide as its widest choice unless it is
/// given a width. A click opens a list of the choices 4px below the button,
/// or above it when there is too little room below. The list is as wide as
/// the button. A press anywhere else closes the list.
#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    value: SharedString,
    choices: Vec<Choice>,
    width: Option<f32>,
    /// The name that assistive technology gives the select. Without a name,
    /// assistive technology has only the choice that the select shows.
    name: Option<SharedString>,
    on_change: Option<ChangeHandler>,
    disabled: bool,
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
        name: None,
        on_change: None,
        disabled: false,
    }
}

impl Select {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    /// Sets the name that assistive technology gives the select and its
    /// list.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }
    pub fn on_change(
        mut self,
        handler: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
    /// Disables the select. A disabled select is drawn at 35% opacity and
    /// does not react to the pointer or to keys, and a list that it had open
    /// is closed.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// The layer that the open list is drawn on. It is above whatever holds the
/// select, also above a dialog.
const LAYER: usize = super::layer::MENU;

/// The height of the button and of each choice.
const ROW: f32 = theme::CONTROL_SM;
/// The measures of the list: 4px of padding and 2px between rows. `LIST_MAX`
/// is the greatest height of the list with its 1px border: seven rows, the
/// six gaps between them, and 12px.
const LIST_PADDING: f32 = 4.;
const LIST_GAP: f32 = 2.;
const LIST_MAX: f32 = VISIBLE as f32 * ROW + 6. * LIST_GAP + 12.;
/// The distance between the button and the list.
const LIST_OFFSET: f32 = 4.;
/// Returns the placement of the list for this number of choices: 4px below
/// the button. The list goes above the button when its height does not fit
/// between the button and a margin of 8px at the bottom of the window, and
/// there is more room above than below. The height used in this test is 26px
/// for each choice plus 8px, and at most 164px. The list is never moved
/// sideways.
fn placement(choices: usize) -> Placement {
    Placement::new(Side::Below, LIST_OFFSET, 8.)
        .flip(Flip::ToRoomierSide {
            needs: (choices as f32 * ROW + 8.).min(164.),
        })
        .unclamped()
}

/// The number of choices that the list shows without scrolling.
const VISIBLE: usize = 7;

/// Returns the height of the list for this number of choices.
fn list_height(choices: usize) -> f32 {
    let rows = choices as f32 * ROW + choices.saturating_sub(1) as f32 * LIST_GAP;
    (rows + 2. * LIST_PADDING + 2.).min(LIST_MAX)
}

/// A rectangle as `[x, y, width, height]`.
type Area = [f32; 4];

/// Returns where a select's list goes when it opens in a pop-up window of
/// its own, which it does when the select is inside a pop-up window of a
/// menu. `button` is the button's rectangle and `area` is the usable part of
/// the display, both in display coordinates. The list is placed as it is
/// inside a window, with the display's area in place of the window, and is
/// moved sideways to stay on the display. The first rectangle returned is
/// the list's. The second is the window's, which has room around the list
/// for its shadow but does not reach past the display's area.
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

        // The label column also holds the label of every choice at a height
        // of 0, which makes the button as wide as the widest label.
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
        let shown = current
            .map(|at| self.choices[at].label.clone())
            .unwrap_or_else(|| self.value.clone());
        // Assistive technology gets the button as a combo box whose value is
        // the choice it shows, and is told whether its list is open.
        let mut button = div()
            .id(self.id.clone())
            .role(Role::ComboBox)
            .aria_expanded(open)
            .aria_value(shown)
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
            // The canvas records where the button is, for placing the list. It
            // fills the button inside the button's 1px border, so the bounds
            // are made 1px larger on each side.
            .child({
                let measured = measured.clone();
                let open = open;
                canvas(
                    move |bounds, window, _| {
                        let bounds = bounds.dilate(px(1.));
                        // When the button has moved while the list is open,
                        // for example because the page scrolled, another frame
                        // is requested, so that the list is drawn at the new
                        // place.
                        if measured.replace(bounds) != bounds && open {
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
            });
        if let Some(width) = self.width {
            button = button.w(px(width));
        }
        if let Some(name) = self.name.clone() {
            button = button.aria_label(name);
        }
        if self.disabled {
            if open {
                state.update(cx, |state, cx| {
                    state.open = false;
                    cx.notify();
                });
            }
            return div()
                .flex()
                .child(button.aria_disabled(true).opacity(0.35));
        }
        button = button.cursor_pointer().on_click(move |_, _, cx| {
            cx.stop_propagation();
            toggle_state.update(cx, |state, cx| {
                state.open = !state.open;
                state.highlighted = current;
                cx.notify();
            });
        });
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
                    name: self.name,
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
                name: self.name,
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
            // A press on the button does not close the list here, because the
            // button's own click handler opens and closes it.
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

/// Makes the button a tab stop and gives it the select's keys. Space opens
/// or closes the list, as a click does. Enter opens the list, or, when the
/// list is open, takes the highlighted choice and closes it. Up and Down
/// open the list at the current choice, and move the highlight when the list
/// is open. Home and End open the list at the first or the last choice.
/// Escape closes the list; when the list is closed, Escape is passed on, for
/// example to the dialog that holds the select. Tab closes the list and
/// moves on to the next tab stop. The button draws no focus ring.
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

/// Holds what the list shows, and the handler that is called when a choice
/// is taken.
#[derive(Clone)]
struct Listed {
    choices: Vec<Choice>,
    current: Option<usize>,
    has_icons: bool,
    state: Entity<State>,
    handler: Option<Rc<ChangeHandler>>,
    name: Option<SharedString>,
}

/// Returns the list of choices with its frame. The current choice and the
/// highlighted row have a background. A click on a row takes that choice,
/// closes the list, and closes the list's pop-up window when it has one.
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
    // A list of more than seven choices scrolls. Its rows are then narrower
    // by the scrollbar's gutter, which is to their right.
    let scrolls = listed.choices.len() > VISIBLE;
    let mut menu = div()
        .id(ElementId::NamedChild(
            std::sync::Arc::new(id.clone()),
            "list".into(),
        ))
        .role(Role::ListBox)
        .when_some(listed.name.clone(), |list, name| list.aria_label(name))
        .w_full()
        // The greatest height of the list inside its 1px border.
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
                .role(Role::ListBoxOption)
                .aria_label(choice.label.clone())
                .aria_selected(listed.current == Some(index))
                .aria_position_in_set(index + 1)
                .aria_size_of_set(listed.choices.len())
                .h(px(ROW))
                .flex_none()
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(theme::RADIUS_SM))
                // The highlighted row and the current choice have the same
                // background colour.
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

/// Returns the select for a pop-up window of a menu. Nothing can be drawn
/// past the edge of that small window, so the select's list opens in a
/// pop-up window of its own, at the button and on the same display. That
/// window closes when the menu closes. A press anywhere else in the menu's
/// window closes the list.
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
        // The pop-up window has no titlebar, so its frame is its content. The
        // list is placed in the points of the interface zoom, and the
        // window's frame is then converted to the points of the screen.
        let origin = crate::zoom::content_origin_in_ui(window);
        let bounds = measured.get();
        let area = crate::zoom::screen_to_ui(display.visible_bounds(), window.ui_zoom());
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
                window_bounds: Some(WindowBounds::Windowed(crate::zoom::ui_to_screen(
                    Bounds::new(
                        point(px(window_rect[0]), px(window_rect[1])),
                        size(px(window_rect[2]), px(window_rect[3])),
                    ),
                    cx.ui_zoom(),
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
                // A pop-up window is never the key window. With GPUI's
                // default interval for inactive windows, its hover
                // highlights would be drawn at 30 frames a second.
                inactive_frame_interval: None,
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
                    // The pop-up window could not be opened, so the list
                    // stays closed.
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
            // A press on the button does not close the list here, because
            // the button's own click handler opens and closes it.
            if bounds.contains(&event.position) {
                return;
            }
            state.update(cx, |state, cx| {
                state.open = false;
                cx.notify();
            });
        })
}

/// The root view of the pop-up window of a select's list. It draws the list
/// at its place in the transparent window, and draws it again whenever the
/// select's state changes, for example when the highlight moves.
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
        // Three choices are three rows, two gaps, the padding and the border.
        assert_eq!(list_height(3), 3. * 26. + 2. * 2. + 8. + 2.);
        let (list, window) = pop_up_list([500., 200., 130., 26.], 3, display);
        assert_eq!(list, [500., 230., 130., list_height(3)]);
        assert_eq!(window, [460., 190., 210., list_height(3) + 80.]);
        // A list of many choices is as tall as a list of seven.
        assert_eq!(list_height(12), 7. * 26. + 6. * 2. + 12.);
    }

    #[test]
    fn a_pop_up_list_flips_above_and_stays_on_the_display() {
        let display = [0., 25., 1440., 875.];
        // Near the display's bottom the list opens above the button, with its
        // bottom 4px over the button, and the window ends at the display's
        // edge.
        let (list, window) = pop_up_list([1380., 860., 130., 26.], 6, display);
        assert_eq!(list[1] + list[3], 856.);
        // The list is moved left to stay on the display.
        assert_eq!(list[0], 1310.);
        assert_eq!(window[0] + window[2], 1440.);
        assert_eq!(window[1] + window[3], list[1] + list[3] + 40.);
    }
}
