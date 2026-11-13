use super::field::{Look, field};
use super::text_input::TextInput;
use crate::{motion, theme};
use gpui::*;
use std::{cell::RefCell, rc::Rc};

type CommitHandler = Rc<dyn Fn(i64, &mut App) + 'static>;

/// What a number field keeps between frames: its text input, the value it
/// was last given, and the handler it reports to.
struct State {
    input: Entity<TextInput>,
    value: i64,
    min: i64,
    max: i64,
    seen: String,
    handler: Rc<RefCell<Option<CommitHandler>>>,
    _typed: Subscription,
}

/// `text` as a whole number within the bounds (`CompactNumberInput`'s
/// `parse`), or `None`.
pub fn parse(text: &str, min: i64, max: i64) -> Option<i64> {
    let digits = text.trim();
    let end = digits
        .char_indices()
        .find(|(at, c)| !(c.is_ascii_digit() || (*at == 0 && (*c == '-' || *c == '+'))))
        .map_or(digits.len(), |(at, _)| at);
    digits[..end]
        .parse::<i64>()
        .ok()
        .filter(|value| (min..=max).contains(value))
}

/// `CompactNumberInput.tsx`: a 100px field with up and down steppers. A
/// valid number reports as it is typed; leaving the field puts back the
/// value if the text is not one, and Escape restores it. The steppers step
/// by one within the bounds.
#[derive(IntoElement)]
pub struct NumberInput {
    id: ElementId,
    value: i64,
    min: i64,
    max: i64,
    disabled: bool,
    on_commit: Option<CommitHandler>,
}

pub fn number_input(id: impl Into<ElementId>, value: i64, min: i64, max: i64) -> NumberInput {
    NumberInput {
        id: id.into(),
        value,
        min,
        max,
        disabled: false,
        on_commit: None,
    }
}

impl NumberInput {
    /// Dims the field to half and its steppers to a fifth, and takes no
    /// input (`GlassInput:disabled`, `.stepper button:disabled`).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_commit(mut self, handler: impl Fn(i64, &mut App) + 'static) -> Self {
        self.on_commit = Some(Rc::new(handler));
        self
    }
}

fn stepper(
    id: &'static str,
    icon: &'static str,
    delta: i64,
    state: Entity<State>,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = theme::theme(cx);
    let hover = window.use_keyed_state(ElementId::from((id, state.entity_id())), cx, |_, _| {
        motion::Fade::default()
    });
    if hover.read(cx).animating() {
        window.request_animation_frame();
    }
    let arrow = div()
        .id(id)
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.))
        .child(super::icon(icon, 11., theme.text()));
    if disabled {
        return arrow.opacity(0.2).into_any_element();
    }
    let opacity = 0.5 + 0.5 * hover.read(cx).value_with(motion::EASE);
    arrow
        .opacity(opacity)
        .cursor_pointer()
        .on_hover(move |hovered, _, cx| {
            hover.update(cx, |hover, cx| {
                hover.set(*hovered, motion::STEPPER);
                cx.notify();
            })
        })
        // The field keeps the keyboard; the arrow only steps.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            let (input, value, min, max, handler) = {
                let state = state.read(cx);
                (
                    state.input.clone(),
                    state.value,
                    state.min,
                    state.max,
                    state.handler.borrow().clone(),
                )
            };
            let typed = parse(&input.read(cx).value(), min, max).unwrap_or(value);
            let next = (typed + delta).clamp(min, max);
            input.update(cx, |input, cx| input.set_text(next.to_string(), cx));
            state.update(cx, |state, _| state.seen = next.to_string());
            if next != value
                && let Some(handler) = handler
            {
                handler(next, cx);
            }
        })
        .into_any_element()
}

impl RenderOnce for NumberInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (value, min, max) = (self.value, self.min, self.max);
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| {
            let input = cx.new(|cx| {
                let mut input = TextInput::number(cx);
                input.set_text(value.to_string(), cx);
                input
            });
            // Each edit that leaves a valid, different number reports it.
            let typed = cx.observe(&input, |state: &mut State, input, cx| {
                let text = input.read(cx).value();
                if text == state.seen {
                    return;
                }
                state.seen = text.clone();
                if let Some(next) = parse(&text, state.min, state.max)
                    && next != state.value
                {
                    state.value = next;
                    if let Some(handler) = state.handler.borrow().clone() {
                        handler(next, cx);
                    }
                }
            });
            State {
                input,
                value,
                min,
                max,
                seen: value.to_string(),
                handler: Rc::new(RefCell::new(None)),
                _typed: typed,
            }
        });
        let input = state.read(cx).input.clone();
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        // Outside editing, the field shows the value it was given.
        if !focused && input.read(cx).value() != value.to_string() {
            input.update(cx, |input, cx| input.set_text(value.to_string(), cx));
        }
        state.update(cx, |state, _| {
            state.value = value;
            state.min = min;
            state.max = max;
            if !focused {
                state.seen = value.to_string();
            }
            *state.handler.borrow_mut() = self.on_commit.clone();
        });
        div()
            .relative()
            .w(px(100.))
            .flex_none()
            .flex()
            .items_center()
            .child(
                div()
                    .relative()
                    .w_full()
                    .opacity(if self.disabled { 0.5 } else { 1. })
                    .child(field(self.id.clone(), &input, Look::Number))
                    // Disabled, a cover takes the presses the field would.
                    .children(self.disabled.then(|| {
                        div()
                            .id("disabled")
                            .absolute()
                            .inset_0()
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .right(px(4.))
                    .bottom(px(2.))
                    .w(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(stepper(
                        "up",
                        "chevron-up-2",
                        1,
                        state.clone(),
                        self.disabled,
                        window,
                        cx,
                    ))
                    .child(stepper(
                        "down",
                        "chevron-down-2",
                        -1,
                        state,
                        self.disabled,
                        window,
                        cx,
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn only_whole_numbers_within_bounds_parse() {
        assert_eq!(parse("42", 0, 100), Some(42));
        assert_eq!(parse(" 7 ", 0, 100), Some(7));
        // `parseInt` stops at the first character that is not a digit.
        assert_eq!(parse("12abc", 0, 100), Some(12));
        assert_eq!(parse("101", 0, 100), None);
        assert_eq!(parse("", 0, 100), None);
        assert_eq!(parse("-1", 0, 100), None);
        assert_eq!(parse("abc", 0, 100), None);
    }
}
