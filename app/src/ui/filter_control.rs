//! The controls of the filter bar: a filter field ([`FilterControl`]) and the
//! bar's glyph buttons ([`FilterAction`]).
use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::{cell::Cell, rc::Rc};

type OpenHandler = Rc<dyn Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static>;
type ClearHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// What a filter field shows before its label.
pub enum Glyph {
    Icon(&'static str),
    /// A dot in the chosen colour.
    Color(Rgba),
}

/// What a filter field keeps between frames.
struct State {
    /// The fade of the field's background when the pointer comes onto the
    /// field or leaves it. It takes `motion::NORMAL` and follows
    /// `motion::EASE_DEFAULT`.
    hover: motion::Fade,
    clear_hovered: bool,
    /// Where the field was last painted. Its editor is placed from these
    /// bounds.
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

/// One filter of the bar: a field 24px high with its glyph and its label. The
/// field has a background while the filter has a value or the pointer is over
/// the field. A click opens the filter's editor below the field. A right click
/// clears the filter, and so does the round button that is shown at the
/// field's top right corner while the filter has a value and the pointer is
/// over the field.
#[derive(IntoElement)]
pub struct FilterControl {
    id: ElementId,
    glyph: Glyph,
    label: SharedString,
    active: bool,
    on_open: Option<OpenHandler>,
    on_clear: Option<ClearHandler>,
}

pub fn filter_control(
    id: impl Into<ElementId>,
    glyph: Glyph,
    label: impl Into<SharedString>,
    active: bool,
) -> FilterControl {
    FilterControl {
        id: id.into(),
        glyph,
        label: label.into(),
        active,
        on_open: None,
        on_clear: None,
    }
}

impl FilterControl {
    /// Sets the handler that is called with the field's bounds on a click. The
    /// caller opens the filter's editor below those bounds.
    pub fn on_open(
        mut self,
        handler: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open = Some(Rc::new(handler));
        self
    }
    pub fn on_clear(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear = Some(Rc::new(handler));
        self
    }
}

/// The largest width of the field's label.
const LABEL_MAX: f32 = 180.;
/// The diameter of the round clear button. The button reaches 6px beyond the
/// field's top edge and 6px beyond its right edge.
const CLEAR: f32 = 16.;

impl RenderOnce for FilterControl {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            hover: motion::Fade::default(),
            clear_hovered: false,
            bounds: Rc::new(Cell::new(Bounds::default())),
        });
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        let (hover, hovered, clear_hovered, animating, bounds) = {
            let state = state.read(cx);
            (
                state.hover.value_with(motion::EASE_DEFAULT),
                state.hover.on(),
                state.clear_hovered,
                state.hover.animating(),
                state.bounds.clone(),
            )
        };
        if animating || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        // Without the pointer, the background is `Theme::contrast` at 0%, or
        // at 10% while the filter has a value. Under the pointer it is at 10%,
        // or at 15% while the filter has a value.
        let (rest, lifted) = if self.active { (10., 15.) } else { (0., 10.) };
        let fill = theme.contrast(rest + (lifted - rest) * hover);
        let glyph = match self.glyph {
            Glyph::Icon(name) => super::icon(name, 16., theme.text()).into_any_element(),
            Glyph::Color(color) => {
                // A 1px ring in the text colour at half that colour's opacity,
                // around the 12px dot.
                let mut ring = theme.text();
                ring.a *= 0.5;
                div()
                    .size(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(ring)
                    .child(div().size(px(12.)).rounded_full().bg(color))
                    .into_any_element()
            }
        };
        let tip = if self.active {
            crate::tf!("{value0} · Right-click to clear", self.label)
        } else {
            self.label.to_string()
        };
        // Assistive technology is told that the field is a button, named by
        // its label.
        let mut control = div()
            .id(self.id.clone())
            .role(Role::Button)
            .aria_label(self.label.clone())
            .occlude()
            .relative()
            .h(px(24.))
            .flex_none()
            .mr(px(2.))
            .pl(px(5.))
            .pr(px(7.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(px(theme::RADIUS_MD))
            .bg(fill)
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .whitespace_nowrap()
            .cursor_pointer()
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(glyph),
            )
            .child(
                div()
                    .max_w(px(LABEL_MAX))
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(self.label.clone()),
            )
            .child({
                let bounds = bounds.clone();
                canvas(move |painted, _, _| bounds.set(painted), |_, _, _, _| {})
                    .absolute()
                    .inset_0()
            });
        if self.active && hovered {
            let clear_state = state.clone();
            let clear = self.on_clear.clone();
            control = control.child(
                div()
                    .id("filter-clear")
                    .role(Role::Button)
                    .aria_label(crate::tf!("Clear {value0} filter", self.label))
                    .absolute()
                    .top(px(-6.))
                    .right(px(-6.))
                    .size(px(CLEAR))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme.text())
                    .opacity(if clear_hovered { 1. } else { 0.9 })
                    .cursor_pointer()
                    .child(super::icon("x", 12., theme.background()))
                    .on_hover(move |hovered, _, cx| {
                        clear_state.update(cx, |state, cx| {
                            state.clear_hovered = *hovered;
                            cx.notify();
                        })
                    })
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(clear) = &clear {
                            clear(window, cx);
                        }
                    }),
            );
        }
        control = control.children(hint.read(cx).layer(&Tip::new(tip), 24., cx));
        let entered = state.clone();
        control = control.on_hover(move |hovered, _, cx| {
            entered.update(cx, |state, cx| {
                state.hover.set(*hovered, motion::NORMAL);
                if !*hovered {
                    state.clear_hovered = false;
                }
                cx.notify();
            });
            hint.update(cx, |hint, cx| hint.set(*hovered, cx));
        });
        if let Some(clear) = self.on_clear {
            control = control.on_mouse_down(MouseButton::Right, move |_, window, cx| {
                cx.stop_propagation();
                clear(window, cx);
            });
        }
        if let Some(open) = self.on_open {
            control = control.on_click(move |_, window, cx| open(bounds.get(), window, cx));
        }
        control
    }
}

type ClickHandler = Rc<dyn Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static>;

/// A glyph button of the filter bar: the shared icon button with the look
/// `icon_button::Look::Filter`. Its click handler is called with the button's
/// bounds, so that a menu can be opened below the button.
#[derive(IntoElement)]
pub struct FilterAction {
    id: ElementId,
    icon: &'static str,
    active: bool,
    disabled: bool,
    tip: Option<Tip>,
    name: Option<SharedString>,
    pressed: Option<bool>,
    on_click: Option<ClickHandler>,
}

pub fn filter_action(id: impl Into<ElementId>, icon: &'static str) -> FilterAction {
    FilterAction {
        id: id.into(),
        icon,
        active: false,
        disabled: false,
        tip: None,
        name: None,
        pressed: None,
        on_click: None,
    }
}

impl FilterAction {
    /// Sets what assistive technology calls the button, when that is not the
    /// text of its tooltip.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }
    /// Tells assistive technology that the button is a toggle, and whether it
    /// is on.
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = Some(pressed);
        self
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn tip(mut self, tip: Tip) -> Self {
        self.tip = Some(tip);
        self
    }
    /// Sets the handler that is called with the button's bounds on a click, so
    /// that a menu can be opened below the button.
    pub fn on_click(
        mut self,
        handler: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for FilterAction {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let bounds = window
            .use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "bounds".into()),
                cx,
                |_, _| Rc::new(Cell::new(Bounds::<Pixels>::default())),
            )
            .read(cx)
            .clone();
        let probe = {
            let bounds = bounds.clone();
            canvas(move |painted, _, _| bounds.set(painted), |_, _, _, _| {})
                .absolute()
                .inset_0()
        };
        // a11y: the button is named by its tooltip or `name`, both set below.
        let mut button = super::icon_button(self.id, self.icon)
            .look(super::icon_button::Look::Filter)
            .toggled(self.active)
            .disabled(self.disabled)
            .child(probe);
        if let Some(tip) = self.tip {
            button = button.tip(tip);
        }
        if let Some(name) = self.name {
            button = button.name(name);
        }
        if let Some(pressed) = self.pressed {
            button = button.pressed(pressed);
        }
        if let Some(handler) = self.on_click {
            button = button.on_click(move |_, window, cx| handler(bounds.get(), window, cx));
        }
        button
    }
}
