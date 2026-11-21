//! Keyboard focus for the shared controls. It behaves as Chromium's keyboard
//! focus does for production's buttons, check boxes, selects, sliders and
//! fields.
//!
//! Each shared control is a tab stop, which is a control that the Tab key
//! can move keyboard focus to ([`control`]). A dialog, the Settings window
//! and a popover are each a scope ([`scope`]). Inside a scope, Tab moves
//! keyboard focus to the next tab stop and Shift-Tab moves it to the
//! previous one, in the order in which the controls are drawn, which is
//! production's DOM order. After the last tab stop Tab goes to the first,
//! and before the first Shift-Tab goes to the last, so keyboard focus never
//! moves to a control behind a dialog. Enter and Space press the button that
//! has keyboard focus. Space ticks the check box that has keyboard focus.
//! The arrow keys change the select or the slider that has keyboard focus.
//!
//! A control draws its focus ring only when the Tab key moved keyboard focus
//! to it, and not after a click, as the CSS selector `:focus-visible` does.
//! A click on a control does not move keyboard focus, so the window's
//! keyboard shortcuts keep working after the click. The next Tab does start
//! from the control that was clicked, as Chromium's sequential focus
//! navigation starting point does.
use crate::theme;
use gpui::*;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

actions!(
    focus,
    [
        /// Bound to Tab. Moves keyboard focus to the next tab stop in the
        /// scope.
        Next,
        /// Bound to Shift-Tab. Moves keyboard focus to the previous tab stop
        /// in the scope.
        Previous,
        /// Presses the control that has keyboard focus. Bound to Enter and
        /// Space on a button, and to Space on a check box or a select.
        Press,
        /// Bound to Enter on a select. Opens the select's list, or, when the
        /// list is open, closes it and chooses the highlighted choice.
        Choose,
        Up,
        Down,
        Left,
        Right,
        Home,
        End,
        PageUp,
        PageDown,
        /// Bound to Escape on a control that can have something open, such
        /// as a select with its list. Closes what is open.
        Cancel,
    ]
);

/// The key contexts in which Tab and Shift-Tab are bound to [`Next`] and
/// [`Previous`]. They are the key contexts of a dialog (`ui::modal`), of the
/// Settings window and of a popover.
// i18n: a key context
const SCOPES: [&str; 3] = ["Modal", "Settings", "Popover"];

pub fn bind(cx: &mut App) {
    let mut bindings = Vec::new();
    for scope in SCOPES {
        bindings.push(KeyBinding::new("tab", Next, Some(scope)));
        bindings.push(KeyBinding::new("shift-tab", Previous, Some(scope)));
    }
    let button = Some(Kind::Button.context());
    let check = Some(Kind::Check.context());
    let select = Some(Kind::Select.context());
    let slider = Some(Kind::Slider.context());
    bindings.extend([
        // Enter and Space press a button, as they press an HTML `<button>`.
        KeyBinding::new("enter", Press, button),
        KeyBinding::new("space", Press, button),
        // Only Space presses a check box, as with an HTML
        // `<input type="checkbox">`.
        KeyBinding::new("space", Press, check),
        // The keys of production's `CmSelect`. Space is bound because
        // `CmSelect` is a `<button>`, and Space clicks a `<button>`.
        KeyBinding::new("enter", Choose, select),
        KeyBinding::new("space", Press, select),
        KeyBinding::new("up", Up, select),
        KeyBinding::new("down", Down, select),
        KeyBinding::new("home", Home, select),
        KeyBinding::new("end", End, select),
        KeyBinding::new("escape", Cancel, select),
        // The keys of an HTML `<input type="range">`.
        KeyBinding::new("left", Left, slider),
        KeyBinding::new("down", Down, slider),
        KeyBinding::new("right", Right, slider),
        KeyBinding::new("up", Up, slider),
        KeyBinding::new("home", Home, slider),
        KeyBinding::new("end", End, slider),
        KeyBinding::new("pageup", PageUp, slider),
        KeyBinding::new("pagedown", PageDown, slider),
    ]);
    cx.bind_keys(bindings);
}

/// The kind of a control that can have keyboard focus. The kind decides
/// which keys the control responds to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A button. Enter and Space press it.
    Button,
    /// A check box or a radio button. Space ticks it.
    Check,
    /// A select. Enter and Space open it. The Up and Down arrow keys, Home
    /// and End move through its choices. Escape closes it.
    Select,
    /// A slider. An arrow key changes its value by one step. Page Up and
    /// Page Down change it by a tenth of its range. Home sets it to its
    /// lowest value and End sets it to its highest value.
    Slider,
}

impl Kind {
    fn context(self) -> &'static str {
        match self {
            Kind::Button => "FocusButton",
            Kind::Check => "FocusCheck",
            Kind::Select => "FocusSelect",
            Kind::Slider => "FocusSlider",
        }
    }
}

/// The focus ring: a line drawn around a control that has keyboard focus, as
/// a CSS `outline` is. The line is `width` pixels thick. Its inner edge is
/// `offset` pixels outside the control's edge, or inside the control's edge
/// when `offset` is negative. Its corners are rounded to follow the
/// control's corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub width: f32,
    pub offset: f32,
    pub color: Rgba,
}

impl Ring {
    /// The ring that `.btn:focus-visible` in `actionButton.module.css`
    /// draws. It is `2px solid var(--color-primary-60)` and it starts 1px
    /// outside the button's edge.
    pub fn button(theme: &theme::Theme) -> Self {
        Self {
            width: 2.,
            offset: 1.,
            color: theme.border_focus(),
        }
    }

    /// Returns two numbers for a control whose corners have the radius
    /// `radius`. The first is how far, in pixels, the ring's outer edge is
    /// outside the control's edge. The second is the radius of the ring's
    /// outer corners. Both match how Chromium draws an outline around a box
    /// with rounded corners. A control with square corners gets a ring with
    /// square corners.
    pub fn outer(&self, radius: f32) -> (f32, f32) {
        let out = self.offset + self.width;
        let corner = if radius > 0. {
            (radius + out).max(0.)
        } else {
            0.
        };
        (out, corner)
    }

    /// Returns the ring as an element to add as a child of the control.
    /// `border` is the thickness of the control's border. A child element
    /// is positioned from the inner edge of its parent's border, but a CSS
    /// outline is measured from the outer edge, so the ring is moved out by
    /// `border` as well.
    fn element(self, radius: f32, border: f32) -> Div {
        let (out, corner) = self.outer(radius);
        let out = out + border;
        div()
            .absolute()
            .top(px(-out))
            .left(px(-out))
            .right(px(-out))
            .bottom(px(-out))
            .rounded(px(corner))
            .border(px(self.width))
            .border_color(self.color)
    }
}

/// What this module remembers about keyboard focus in each window. `by_tab`
/// is the control that the Tab key last moved keyboard focus to. `pressed`
/// is the control that was last clicked, which is where the next Tab starts
/// from, together with whatever had keyboard focus at the time of the click.
#[derive(Default)]
struct Keyboard {
    by_tab: HashMap<WindowId, WeakFocusHandle>,
    pressed: HashMap<WindowId, (WeakFocusHandle, Option<WeakFocusHandle>)>,
    /// The bounds of the control that the Tab key has just moved keyboard
    /// focus to, so that a scroller can scroll the control into view. The
    /// value is `None` until the control has been drawn and its bounds are
    /// known.
    reveal: HashMap<WindowId, Option<Bounds<Pixels>>>,
}

impl Global for Keyboard {}

fn keyboard(cx: &mut App) -> &mut Keyboard {
    cx.default_global::<Keyboard>()
}

fn same(weak: &WeakFocusHandle, handle: &FocusHandle) -> bool {
    weak.upgrade().is_some_and(|stored| &stored == handle)
}

/// Returns true when `handle` has keyboard focus in this window and is the
/// control that the Tab key last moved keyboard focus to.
fn tabbed_to(handle: &FocusHandle, window: &Window, cx: &App) -> bool {
    handle.is_focused(window)
        && cx.try_global::<Keyboard>().is_some_and(|keyboard| {
            keyboard
                .by_tab
                .get(&window.window_handle().window_id())
                .is_some_and(|weak| same(weak, handle))
        })
}

/// The keyboard focus of one control. Make it with [`control`] in the
/// control's `render`, add its [`Control::ring`] to the control's element,
/// and then pass the element to [`Control::attach`].
pub struct Control {
    handle: FocusHandle,
    shown: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

/// Returns the keyboard focus of the control with the id `id`. Its focus
/// handle is stored under that id, so it is the same one in every frame.
pub fn control(id: &ElementId, window: &mut Window, cx: &mut App) -> Control {
    let key = ElementId::NamedChild(std::sync::Arc::new(id.clone()), "focus".into());
    let state = window.use_keyed_state(key, cx, |_, cx| {
        (
            cx.focus_handle().tab_stop(true),
            Rc::new(Cell::new(Bounds::default())),
        )
    });
    let (handle, bounds) = state.read(cx).clone();
    #[cfg(test)]
    names::name(&handle, id, cx);
    Control {
        shown: tabbed_to(&handle, window, cx),
        handle,
        bounds,
    }
}

impl Control {
    /// Returns true when the control has keyboard focus, whether or not it
    /// shows its focus ring.
    pub fn focused(&self, window: &Window) -> bool {
        self.handle.is_focused(window)
    }

    /// Returns the focus ring when the control shows it, and `None` when it
    /// does not. `radius` is the radius of the control's corners and
    /// `border` is the thickness of its border. Add the ring as a child of
    /// the control's element, which must be positioned (`relative`).
    pub fn ring(&self, ring: Ring, radius: f32, border: f32) -> Option<Div> {
        self.shown.then(|| ring.element(radius, border))
    }

    /// Makes `element` a tab stop that responds to the keys of `kind`.
    /// `press` is called when the control has keyboard focus and the user
    /// presses a key that is bound to [`Press`] for `kind` (Enter or Space
    /// on a button). It receives a click event that comes from the keyboard
    /// and not from the pointer. A disabled control cannot be reached with
    /// the Tab key, so do not call `attach` for it.
    ///
    /// When `element` has a role, its node in the accessibility tree
    /// represents the tab stop. Assistive technology reports the node as
    /// focused while the tab stop has keyboard focus, and when assistive
    /// technology focuses the node, keyboard focus moves to the tab stop.
    ///
    /// The tab stop itself is a child element with no size, so a press of
    /// the pointer never lands on it. That is why a press on the control
    /// does not move keyboard focus (see the module documentation). The
    /// press is recorded as the place where the next Tab starts from.
    pub fn attach<E>(
        self,
        element: E,
        kind: Kind,
        press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> E
    where
        E: StatefulInteractiveElement + ParentElement + Sized,
    {
        let Control {
            handle,
            shown,
            bounds,
        } = self;
        let pressed = handle.clone();
        let at = bounds.clone();
        let mut element = element
            .a11y_focus(&handle)
            .key_context(kind.context())
            .on_action(move |_: &Press, window, cx| {
                let event = ClickEvent::Keyboard(KeyboardClickEvent {
                    button: KeyboardButton::Enter,
                    bounds: at.get(),
                });
                press(&event, window, cx);
            })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let before = window.focused(cx).map(|focus| focus.downgrade());
                keyboard(cx).pressed.insert(
                    window.window_handle().window_id(),
                    (pressed.downgrade(), before),
                );
            });
        element = element.child(div().absolute().size_0().track_focus(&handle));
        if shown {
            // This canvas records the control's bounds for two uses. A press
            // with the keyboard puts them in its click event, so that a
            // handler can position something next to the control. When the
            // Tab key has just moved keyboard focus here, they are also
            // stored in `Keyboard::reveal`, so that a scroller can scroll
            // the control into view.
            element = element.child(
                canvas(
                    move |area, window, cx| {
                        bounds.set(area);
                        let id = window.window_handle().window_id();
                        if let Some(reveal @ None) = keyboard(cx).reveal.get_mut(&id) {
                            *reveal = Some(area);
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            );
        }
        element
    }
}

/// Makes `element` a tab stop that Enter and Space press and that draws no
/// focus ring. Use it for a control that production draws as a plain
/// `<button>` without a focus ring.
pub fn button<E>(
    element: E,
    id: &ElementId,
    window: &mut Window,
    cx: &mut App,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> E
where
    E: StatefulInteractiveElement + ParentElement + Sized,
{
    control(id, window, cx).attach(element, Kind::Button, press)
}

/// Wraps a view's own `<button>` so that it becomes a tab stop. Use it where
/// the view builds the element in code that has no `Window` to pass to
/// [`button`]. The returned [`Stop`] does what [`button`] does at the time
/// it is rendered, when it is given the window. `id` must be the id of
/// `element`.
pub fn stop(
    id: impl Into<ElementId>,
    element: Stateful<Div>,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stop {
    Stop {
        id: id.into(),
        element,
        press: Box::new(press),
        ring: None,
    }
}

#[derive(IntoElement)]
pub struct Stop {
    id: ElementId,
    element: Stateful<Div>,
    press: Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
    ring: Option<(Ring, f32, f32)>,
}

impl Stop {
    /// Sets the focus ring that the button draws when the Tab key has moved
    /// keyboard focus to it. `radius` is the radius of the button's corners
    /// and `border` is the thickness of its border.
    pub fn ring(mut self, ring: Ring, radius: f32, border: f32) -> Self {
        self.ring = Some((ring, radius, border));
        self
    }
}

impl RenderOnce for Stop {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = control(&self.id, window, cx);
        let mut element = self.element;
        if let Some((ring, radius, border)) = self.ring {
            element = element.children(focus.ring(ring, radius, border));
        }
        focus.attach(element, Kind::Button, self.press)
    }
}

/// Makes `element` a scope. `element` must already track the focus handle
/// `handle`. Tab and Shift-Tab then move keyboard focus through the tab
/// stops inside `element`, going from the last one to the first and from the
/// first one to the last. The key context of `element` must be one of
/// [`SCOPES`].
pub fn scope<E: InteractiveElement>(element: E, handle: &FocusHandle) -> E {
    let (next, previous) = (handle.clone(), handle.clone());
    element
        .on_action(move |_: &Next, window, cx| step(&next, true, window, cx))
        .on_action(move |_: &Previous, window, cx| step(&previous, false, window, cx))
}

/// Moves keyboard focus to the next tab stop inside `scope` when `forward`
/// is true, and to the previous one when it is false. After the last tab
/// stop it goes to the first, and before the first it goes to the last. The
/// tab stop that gets keyboard focus shows its focus ring. When `scope`
/// contains no tab stop, keyboard focus stays where it was.
pub fn step(scope: &FocusHandle, forward: bool, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let start = window.focused(cx);
    // When a control has been clicked since the last Tab, Tab starts from
    // that control. This does not apply when keyboard focus has moved to a
    // different tab stop since the click. It also does not apply when the
    // clicked control is outside `scope`: keyboard focus is then put back
    // where it was before Tab was pressed.
    if let Some((pressed, before)) = keyboard(cx).pressed.remove(&id)
        && let Some(pressed) = pressed.upgrade()
    {
        let unmoved = match (&start, before.and_then(|before| before.upgrade())) {
            (Some(now), Some(before)) => *now == before || !now.tab_stop,
            (Some(now), None) => !now.tab_stop,
            (None, _) => true,
        };
        if unmoved {
            window.focus(&pressed, cx);
            if !scope.contains_focused(window, cx) {
                match &start {
                    Some(start) => window.focus(start, cx),
                    None => window.blur(cx),
                }
            }
        }
    }
    let mut seen = Vec::new();
    loop {
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
        let Some(now) = window.focused(cx) else {
            break;
        };
        if now != *scope && scope.contains_focused(window, cx) {
            let keyboard = keyboard(cx);
            keyboard.by_tab.insert(id, now.downgrade());
            keyboard.reveal.insert(id, None);
            window.refresh();
            return;
        }
        // Keyboard focus has returned to a tab stop that it was on before,
        // so every tab stop in the window has been tried and none of them
        // is inside `scope`.
        if seen.contains(&now) {
            break;
        }
        seen.push(now);
    }
    match start {
        Some(start) => window.focus(&start, cx),
        None => window.blur(cx),
    }
}

/// Scrolls `scroll` by the smallest amount that brings the control that the
/// Tab key has just moved keyboard focus to into view, as a browser scrolls
/// a focused element into view. It does this only when that control is
/// inside `area`, which is the focus handle that the scroller's element
/// tracks. A scroller calls this each time it draws.
pub fn follow(scroll: &ScrollHandle, area: &FocusHandle, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let Some(Some(control)) = cx
        .try_global::<Keyboard>()
        .and_then(|keyboard| keyboard.reveal.get(&id).copied())
    else {
        return;
    };
    if !area.contains_focused(window, cx) {
        return;
    }
    keyboard(cx).reveal.remove(&id);
    let view = scroll.bounds();
    let offset = scroll.offset();
    let moved = revealed(
        (f32::from(view.top()), f32::from(view.bottom())),
        (f32::from(control.top()), f32::from(control.bottom())),
        f32::from(offset.y),
        f32::from(scroll.max_offset().y),
    );
    if moved != f32::from(offset.y) {
        scroll.set_offset(point(offset.x, px(moved)));
        window.refresh();
    }
}

/// Returns the vertical scroll offset that brings a control into view.
/// `height` is the top and the bottom of the scroller in the window, and
/// `down` is the top and the bottom of the control. `offset` is the current
/// scroll offset: 0 when the scroller is at its top, negative when it is
/// scrolled down, and never below `-max`. A control whose top is above the
/// scroller's top is scrolled until its top is at the scroller's top. A
/// control whose bottom is below the scroller's bottom is scrolled until its
/// bottom is at the scroller's bottom. A control that is already in view
/// leaves the offset unchanged.
fn revealed(height: (f32, f32), down: (f32, f32), offset: f32, max: f32) -> f32 {
    let moved = if down.0 < height.0 {
        offset + (height.0 - down.0)
    } else if down.1 > height.1 {
        offset - (down.1 - height.1)
    } else {
        offset
    };
    moved.clamp(-max, 0.)
}

/// Returns the value that a slider has after `key` is pressed, as an HTML
/// `<input type="range">` behaves. An arrow key changes `value` by `step`.
/// Page Up and Page Down change it by a tenth of the range from `min` to
/// `max`, or by `step` when `step` is larger than that. Home sets it to
/// `min` and End sets it to `max`. The result is rounded to a whole number
/// of steps from `min` and is kept between `min` and `max`. A `step` of
/// zero or less counts as 1.
pub fn slider_key(key: SliderKey, value: f64, min: f64, max: f64, step: f64) -> f64 {
    let step = if step > 0. { step } else { 1. };
    let page = ((max - min) / 10.).max(step);
    let moved = match key {
        SliderKey::Down => value - step,
        SliderKey::Up => value + step,
        SliderKey::PageDown => value - page,
        SliderKey::PageUp => value + page,
        SliderKey::Home => min,
        SliderKey::End => max,
    };
    let snapped = min + ((moved - min) / step).round() * step;
    snapped.clamp(min, max)
}

/// A key that changes a slider's value. The Left and Down arrow keys lower
/// the value by one step (`Down`), and the Right and Up arrow keys raise it
/// by one step (`Up`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliderKey {
    Down,
    Up,
    PageDown,
    PageUp,
    Home,
    End,
}

/// A name for each focus handle. The off-screen tool uses the names for its
/// `focused` line, which prints the control that has keyboard focus.
#[cfg(test)]
pub mod names {
    use gpui::{App, ElementId, FocusHandle, Global, SharedString, Window};
    use std::collections::HashMap;

    #[derive(Default)]
    struct Names(HashMap<String, SharedString>);
    impl Global for Names {}

    fn key(handle: &FocusHandle) -> String {
        format!("{handle:?}")
    }

    pub fn name(handle: &FocusHandle, id: &ElementId, cx: &mut App) {
        let names = &mut cx.default_global::<Names>().0;
        let key = key(handle);
        if !names.contains_key(&key) {
            names.insert(key, format!("{id}").into());
        }
    }

    pub fn name_text(handle: &FocusHandle, name: impl Into<SharedString>, cx: &mut App) {
        cx.default_global::<Names>()
            .0
            .insert(key(handle), name.into());
    }

    /// Returns the name of the control that has keyboard focus, followed by
    /// ` (ring)` when it shows its focus ring. Returns `nothing` when
    /// nothing has keyboard focus.
    pub fn focused(window: &Window, cx: &App) -> String {
        let Some(handle) = window.focused(cx) else {
            return "nothing".into();
        };
        let name = cx
            .try_global::<Names>()
            .and_then(|names| names.0.get(&key(&handle)).cloned())
            .unwrap_or_else(|| format!("unnamed {handle:?}").into());
        let ring = if super::tabbed_to(&handle, window, cx) {
            " (ring)"
        } else {
            ""
        };
        format!("{name}{ring}")
    }
}

#[cfg(test)]
mod tests {
    use super::{Ring, SliderKey, revealed, slider_key};
    use gpui::rgba;

    #[test]
    fn a_ring_follows_the_corners_outside_the_control() {
        let ring = Ring {
            width: 2.,
            offset: 1.,
            color: rgba(0x3297ff99),
        };
        // For the 6px corners of `.btn`, the outline's outer edge is 3px
        // outside the button and its outer corners have a radius of 9px.
        assert_eq!(ring.outer(6.), (3., 9.));
        // A control with square corners gets a ring with square corners.
        assert_eq!(ring.outer(0.), (3., 0.));
        // A ring that is drawn inside the control's edge (a negative offset)
        // never gets a negative corner radius.
        let inside = Ring {
            width: 1.,
            offset: -1.,
            color: rgba(0xffffffff),
        };
        assert_eq!(inside.outer(2.), (0., 2.));
        assert_eq!(inside.outer(0.), (0., 0.));
    }

    #[test]
    fn tab_scrolls_a_control_into_view_the_least_it_can() {
        // The scroller's body is in the window from y 100 to y 400 and can
        // scroll 300px further.
        let height = (100., 400.);
        // A control that is already in view does not change the offset.
        assert_eq!(revealed(height, (150., 180.), 0., 300.), 0.);
        // A control below the body's bottom edge is scrolled until its
        // bottom is at the body's bottom.
        assert_eq!(revealed(height, (420., 450.), 0., 300.), -50.);
        // With the body scrolled down by 200px, a control above the body's
        // top edge is scrolled until its top is at the body's top.
        assert_eq!(revealed(height, (50., 80.), -200., 300.), -150.);
        // The offset never goes past the start or the end of the scroll
        // range.
        assert_eq!(revealed(height, (900., 950.), 0., 300.), -300.);
        assert_eq!(revealed(height, (-90., -60.), -100., 300.), 0.);
    }

    #[test]
    fn a_slider_steps_as_a_range_input_does() {
        // The thumbnail size slider goes from 100 to 600 in steps of 50.
        assert_eq!(slider_key(SliderKey::Up, 200., 100., 600., 50.), 250.);
        assert_eq!(slider_key(SliderKey::Down, 100., 100., 600., 50.), 100.);
        assert_eq!(slider_key(SliderKey::PageUp, 200., 100., 600., 50.), 250.);
        assert_eq!(slider_key(SliderKey::Home, 350., 100., 600., 50.), 100.);
        assert_eq!(slider_key(SliderKey::End, 350., 100., 600., 50.), 600.);
        // The quality slider goes from 1 to 100 in steps of 1. Page Up and
        // Page Down change it by a tenth of the range.
        assert_eq!(slider_key(SliderKey::PageUp, 80., 1., 100., 1.), 90.);
        assert_eq!(slider_key(SliderKey::PageDown, 5., 1., 100., 1.), 1.);
        assert_eq!(slider_key(SliderKey::Up, 100., 1., 100., 1.), 100.);
    }
}
