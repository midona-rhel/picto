//! Keyboard focus for the shared controls, as Chromium gives it to
//! production's buttons, check boxes, selects, sliders and fields.
//!
//! Each shared control is a tab stop ([`control`]). A dialog, the Settings
//! window and a popover are scopes ([`scope`]): Tab and Shift-Tab move
//! through the stops inside one, in the order they are drawn (production's
//! DOM order), wrapping at its ends, so the keyboard never reaches what is
//! behind a dialog. Enter and Space press a focused button, Space ticks a
//! focused check box, the arrow keys move a focused select or slider.
//!
//! A control shows its ring only while Tab put the keyboard on it
//! (`:focus-visible`), not after a click. A click leaves the keyboard where
//! it was, so the window's shortcuts keep working, but the next Tab starts
//! from the control pressed, as Chromium's sequential focus starting point
//! does.
use crate::theme;
use gpui::*;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

actions!(
    focus,
    [
        /// Tab: the next stop in the scope.
        Next,
        /// Shift-Tab: the stop before.
        Previous,
        /// Enter or Space on a button; Space on a check box or a select.
        Press,
        /// Enter on a select: opens it, or takes the highlighted choice.
        Choose,
        Up,
        Down,
        Left,
        Right,
        Home,
        End,
        PageUp,
        PageDown,
        /// Escape on a control that has something open (a select's list).
        Cancel,
    ]
);

/// The key contexts that hold a scope's Tab and Shift-Tab: a dialog
/// (`ui::modal`), the Settings window and a popover.
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
        // A `<button>`: Enter and Space.
        KeyBinding::new("enter", Press, button),
        KeyBinding::new("space", Press, button),
        // An `<input type="checkbox">`: Space alone.
        KeyBinding::new("space", Press, check),
        // `CmSelect`'s keys; Space is the `<button>`'s click.
        KeyBinding::new("enter", Choose, select),
        KeyBinding::new("space", Press, select),
        KeyBinding::new("up", Up, select),
        KeyBinding::new("down", Down, select),
        KeyBinding::new("home", Home, select),
        KeyBinding::new("end", End, select),
        KeyBinding::new("escape", Cancel, select),
        // An `<input type="range">`.
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

/// What kind of control has the keyboard, which decides its keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A button: Enter and Space press it.
    Button,
    /// A check box or radio button: Space ticks it.
    Check,
    /// A select: Enter and Space open it, the arrows, Home and End move
    /// through its choices, Escape closes it.
    Select,
    /// A slider: the arrows step it, Page Up and Page Down by a tenth,
    /// Home and End to its ends.
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

/// An `outline` drawn around a control with the keyboard: `width` thick,
/// `offset` outside its edge (inside, when negative), following its
/// corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub width: f32,
    pub offset: f32,
    pub color: Rgba,
}

impl Ring {
    /// `actionButton.module.css` `.btn:focus-visible`: `2px solid
    /// var(--color-primary-60)`, 1px out.
    pub fn button(theme: &theme::Theme) -> Self {
        Self {
            width: 2.,
            offset: 1.,
            color: theme.border_focus(),
        }
    }

    /// How far the ring's outer edge lies outside the control's, and the
    /// radius of its outer corners, as Chromium draws an outline round a
    /// box with rounded corners.
    pub fn outer(&self, radius: f32) -> (f32, f32) {
        let out = self.offset + self.width;
        let corner = if radius > 0. {
            (radius + out).max(0.)
        } else {
            0.
        };
        (out, corner)
    }

    /// The ring as a child of a control whose border is `border` thick: a
    /// child is placed from inside the border, an outline from outside it.
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

/// Where the keyboard is, per window: the control Tab last put it on, and
/// the control a click last pressed (where the next Tab starts).
#[derive(Default)]
struct Keyboard {
    by_tab: HashMap<WindowId, WeakFocusHandle>,
    pressed: HashMap<WindowId, (WeakFocusHandle, Option<WeakFocusHandle>)>,
    /// A control Tab just reached, to scroll into view: `None` until it has
    /// been drawn and its bounds are known.
    reveal: HashMap<WindowId, Option<Bounds<Pixels>>>,
}

impl Global for Keyboard {}

fn keyboard(cx: &mut App) -> &mut Keyboard {
    cx.default_global::<Keyboard>()
}

fn same(weak: &WeakFocusHandle, handle: &FocusHandle) -> bool {
    weak.upgrade().is_some_and(|stored| &stored == handle)
}

/// The control Tab put the keyboard on in this window, while it still has
/// it.
fn tabbed_to(handle: &FocusHandle, window: &Window, cx: &App) -> bool {
    handle.is_focused(window)
        && cx.try_global::<Keyboard>().is_some_and(|keyboard| {
            keyboard
                .by_tab
                .get(&window.window_handle().window_id())
                .is_some_and(|weak| same(weak, handle))
        })
}

/// One control's keyboard focus. Make it with [`control`] in the control's
/// `render`, draw its [`Control::ring`] and [`Control::attach`] it to the
/// control's element.
pub struct Control {
    handle: FocusHandle,
    shown: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

/// The keyboard focus of the control `id`, kept between frames.
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
    /// The ring, while the control shows it, for a control with corners of
    /// `radius` and a border `border` thick. Add it as a child of the
    /// control's (relative) element.
    pub fn ring(&self, ring: Ring, radius: f32, border: f32) -> Option<Div> {
        self.shown.then(|| ring.element(radius, border))
    }

    /// Makes `element` the control: a tab stop whose keys are those of
    /// `kind`, `press` running for Enter or Space (a click with no
    /// pointer). A disabled control is no stop: leave it unattached.
    ///
    /// The tab stop is a child of no size, which a pointer never reaches,
    /// so a press on the control leaves the keyboard where it was (see the
    /// module's note); the press is remembered as where the next Tab
    /// starts.
    pub fn attach<E>(
        self,
        element: E,
        kind: Kind,
        press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> E
    where
        E: InteractiveElement + ParentElement + Sized,
    {
        let Control {
            handle,
            shown,
            bounds,
        } = self;
        let pressed = handle.clone();
        let at = bounds.clone();
        let mut element = element
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
            // Where a press by the keyboard happens, for a handler that
            // places something by it, and what a scroller brings into view
            // when Tab has just come here.
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

/// A plain `<button>` that draws no ring: a stop that Enter and Space press.
pub fn button<E>(
    element: E,
    id: &ElementId,
    window: &mut Window,
    cx: &mut App,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> E
where
    E: InteractiveElement + ParentElement + Sized,
{
    control(id, window, cx).attach(element, Kind::Button, press)
}

/// A view's own `<button>`, made a stop where its view draws it without the
/// window at hand: [`button`] once it renders. Give it the element's id.
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
    /// The ring it draws while it has the keyboard, for corners of `radius`
    /// and a border `border` thick.
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

/// Makes `element`, which tracks `handle`, a scope: Tab and Shift-Tab move
/// through the stops inside it and wrap at its ends. Its key context must
/// be one of [`SCOPES`].
pub fn scope<E: InteractiveElement>(element: E, handle: &FocusHandle) -> E {
    let (next, previous) = (handle.clone(), handle.clone());
    element
        .on_action(move |_: &Next, window, cx| step(&next, true, window, cx))
        .on_action(move |_: &Previous, window, cx| step(&previous, false, window, cx))
}

/// Moves the keyboard to the next (or previous) stop inside `scope`,
/// wrapping at its ends, and has that stop show its ring. With no stop
/// inside, the keyboard stays where it is.
pub fn step(scope: &FocusHandle, forward: bool, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let start = window.focused(cx);
    // A control pressed since the keyboard last moved is where Tab starts
    // from, unless the keyboard has gone to another stop since.
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
        // Every stop has come round once, none of them inside.
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

/// Scrolls `scroll` the least that shows the control Tab has just reached,
/// when that control is inside `area` (the scroller, which tracks it), as the
/// browser scrolls a focused element into view. A scroller calls it as it
/// draws.
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

/// The vertical offset (0 at the top, negative further down, no further
/// than `-max`) that shows a control spanning `down` in a scroller spanning
/// `height` scrolled to `offset`: its top brought to the scroller's top, or
/// its bottom to the scroller's bottom, whichever is nearer.
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

/// The value a slider's key moves it to: an arrow steps it by `step`, Page
/// Up and Page Down by a tenth of its range (at least a step), Home and End
/// to its ends, as an `<input type="range">` does.
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

/// A key that moves a slider: Left and Down step it down, Right and Up up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliderKey {
    Down,
    Up,
    PageDown,
    PageUp,
    Home,
    End,
}

/// The names of the focus handles, for the off-screen tool's `focused`
/// line, which prints the control that has the keyboard.
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

    /// The control with the keyboard, and whether it shows its ring.
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
        // `.btn`'s 6px corners: the outline's outer edge is 3px out, its
        // corners 9px round.
        assert_eq!(ring.outer(6.), (3., 9.));
        // A square control keeps square corners.
        assert_eq!(ring.outer(0.), (3., 0.));
        // Drawn inside the edge it never turns its corners inside out.
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
        // A body from y 100 to 400, with 300 more to scroll.
        let height = (100., 400.);
        // A control already showing stays put.
        assert_eq!(revealed(height, (150., 180.), 0., 300.), 0.);
        // One below the fold brings its bottom to the body's bottom.
        assert_eq!(revealed(height, (420., 450.), 0., 300.), -50.);
        // Scrolled down 200, one above the top brings its top to the top.
        assert_eq!(revealed(height, (50., 80.), -200., 300.), -150.);
        // Never past either end.
        assert_eq!(revealed(height, (900., 950.), 0., 300.), -300.);
        assert_eq!(revealed(height, (-90., -60.), -100., 300.), 0.);
    }

    #[test]
    fn a_slider_steps_as_a_range_input_does() {
        // The thumbnail size: 100 to 600 by 50.
        assert_eq!(slider_key(SliderKey::Up, 200., 100., 600., 50.), 250.);
        assert_eq!(slider_key(SliderKey::Down, 100., 100., 600., 50.), 100.);
        assert_eq!(slider_key(SliderKey::PageUp, 200., 100., 600., 50.), 250.);
        assert_eq!(slider_key(SliderKey::Home, 350., 100., 600., 50.), 100.);
        assert_eq!(slider_key(SliderKey::End, 350., 100., 600., 50.), 600.);
        // Quality 1 to 100 by 1: a page is a tenth of the range.
        assert_eq!(slider_key(SliderKey::PageUp, 80., 1., 100., 1.), 90.);
        assert_eq!(slider_key(SliderKey::PageDown, 5., 1., 100., 1.), 1.);
        assert_eq!(slider_key(SliderKey::Up, 100., 1., 100., 1.), 100.);
    }
}
