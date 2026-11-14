//! The filter bar's controls (`GridFilterMenu.module.css`): a filter field
//! (`FilterControl`, `.filterItem`) and the bar's glyph buttons
//! (`.filterAction`, `.addButton`).
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
    /// The chosen colour, as `.colorDot`.
    Color(Rgba),
}

/// What a filter field keeps between frames.
struct State {
    /// `transition: background-color 200ms var(--ease-default)`.
    hover: motion::Fade,
    clear_hovered: bool,
    /// Where it was last painted, which places its menu.
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

/// One filter of the bar: a 24px pill with its glyph and label, on the
/// subtle overlay while it holds a value or is hovered. A click opens its
/// editor below it; a right click, or the round clear button that shows at
/// its corner while it is set and hovered, clears it.
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
    /// Called with the field's bounds, to open its editor below it.
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

/// `.filterLabel`'s `max-width`.
const LABEL_MAX: f32 = 180.;
/// `.clearButton`: 16px, 6px out from the top-right corner.
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
        // At rest nothing, or the subtle overlay while set; hovered, the
        // subtle overlay, or 15% of the text colour while set.
        let (rest, lifted) = if self.active { (10., 15.) } else { (0., 10.) };
        let fill = theme.contrast(rest + (lifted - rest) * hover);
        let glyph = match self.glyph {
            Glyph::Icon(name) => super::icon(name, 16., theme.text()).into_any_element(),
            Glyph::Color(color) => {
                // `outline: 1px solid` of half the text colour, outside the
                // 12px dot.
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
        // `role="button"` named by its label.
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

/// `.filterAction` and `.addButton`: the shared icon button's filter look,
/// which tells its click where it is, for a menu that opens below it.
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
    /// What assistive technology calls the button when its tooltip says
    /// something else (production's `aria-label`).
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }
    /// Reports the button as a toggle (`aria-pressed`).
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
    /// Called with the button's bounds, for a menu that opens below it.
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
