//! The centred dialog (`GlassModal.tsx`): a dimmed backdrop over the whole
//! window and a glass panel with a title, a body and an optional footer.
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::{rc::Rc, time::Instant};

actions!(modal, [Close]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some("Modal"))]);
}

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// A dialog's time on screen: when it opened and, once dismissed, when it
/// began to leave. The owner keeps one per dialog and drops the dialog once
/// [`Presence::finished`].
#[derive(Clone, Copy)]
pub struct Presence {
    opened: Instant,
    closing: Option<Instant>,
}

impl Default for Presence {
    fn default() -> Self {
        Self::new()
    }
}

impl Presence {
    pub fn new() -> Self {
        Self {
            opened: Instant::now(),
            closing: None,
        }
    }

    /// Starts the 120 ms exit. Closing twice changes nothing.
    pub fn close(&mut self) {
        self.closing.get_or_insert_with(Instant::now);
    }

    pub fn closing(&self) -> bool {
        self.closing.is_some()
    }

    /// The exit is over and the dialog can go.
    pub fn finished(&self) -> bool {
        self.closing
            .is_some_and(|since| since.elapsed() >= motion::MODAL_OUT)
    }

    pub fn animating(&self) -> bool {
        match self.closing {
            Some(since) => since.elapsed() < motion::MODAL_OUT,
            None => self.opened.elapsed() < motion::MODAL_IN,
        }
    }

    /// How far in the panel is, 0 to 1: `modalIn` on the default curve,
    /// `modalOut` on `ease-in`.
    fn panel(&self) -> f32 {
        match self.closing {
            Some(since) => {
                1. - motion::EASE_IN.ease(motion::progress(since.elapsed(), motion::MODAL_OUT))
            }
            None => {
                motion::EASE_DEFAULT.ease(motion::progress(self.opened.elapsed(), motion::MODAL_IN))
            }
        }
    }

    /// The backdrop's opacity: `backdropIn` on `ease-out`, `backdropOut` on
    /// `ease-in`.
    fn backdrop(&self) -> f32 {
        match self.closing {
            Some(since) => {
                1. - motion::EASE_IN.ease(motion::progress(since.elapsed(), motion::MODAL_OUT))
            }
            None => {
                motion::EASE_OUT.ease(motion::progress(self.opened.elapsed(), motion::MODAL_IN))
            }
        }
    }
}

/// `.sm`, `.md` and `.lg`: the panel's width and the share of the window's
/// height it may take.
#[derive(Clone, Copy)]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    fn width(self) -> f32 {
        match self {
            Size::Small => 400.,
            Size::Medium => 560.,
            Size::Large => 680.,
        }
    }
    fn max_height(self) -> f32 {
        match self {
            Size::Small | Size::Medium => 0.8,
            Size::Large => 0.85,
        }
    }
}

const HEADER: f32 = 48.;
const FOOTER: f32 = 44.;
/// The body's padding, and the scrollbar gutter it keeps on the right.
const BODY_PADDING: f32 = 16.;

#[derive(IntoElement)]
pub struct Modal {
    id: ElementId,
    title: SharedString,
    size: Size,
    /// A width other than the size's (`panelClassName` widths).
    width: Option<f32>,
    presence: Presence,
    body: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    on_close: Option<Handler>,
    focus: Option<FocusHandle>,
    priority: usize,
}

/// A dialog titled `title`. Draw it from the view that owns it; it covers
/// the whole window whatever that view's bounds.
pub fn modal(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    presence: Presence,
) -> Modal {
    Modal {
        id: id.into(),
        title: title.into(),
        size: Size::Medium,
        width: None,
        presence,
        body: Vec::new(),
        footer: Vec::new(),
        on_close: None,
        focus: None,
        priority: 10,
    }
}

impl Modal {
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
    /// A width other than the size's, for a dialog that sets its own.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.body.push(child.into_any_element());
        self
    }
    /// A button in the footer, which shows once it has one.
    pub fn footer(mut self, button: impl IntoElement) -> Self {
        self.footer.push(button.into_any_element());
        self
    }
    /// What the close button, Escape and a click on the backdrop do.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
    /// The focus the dialog holds while no field in it has the keyboard, so
    /// Escape reaches it.
    pub fn focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }
    /// Dialogs opened over another one draw above it.
    pub fn layer(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }
}

/// `WindowCloseButton`: 28px with a one-pixel cross.
fn close_button(on_close: Option<Handler>, cx: &App) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let mut button = div()
        .id("modal-close")
        .size(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_CONTROL))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.subtle_overlay()))
        .active(move |style| style.bg(theme.pressed_overlay()))
        .child(super::icon("modal-close", 16., theme.text()));
    if let Some(handler) = on_close {
        button = button.on_click(move |_, window, cx| handler(window, cx));
    }
    button
}

impl RenderOnce for Modal {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        if self.presence.animating() {
            window.request_animation_frame();
        }
        let shown = self.presence.panel();
        let viewport = window.viewport_size();
        let max_height = f32::from(viewport.height) * self.size.max_height();
        let has_footer = !self.footer.is_empty();
        let body_height = max_height - HEADER - if has_footer { FOOTER } else { 0. };
        let scroll = window
            .use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "body".into()),
                cx,
                |_, _| ScrollHandle::new(),
            )
            .read(cx)
            .clone();

        // `.panel`'s edges: 25% white on top, 15% at the sides and 10% at
        // the bottom. One border has one colour, so the bottom's goes all
        // the way round and the others are laid over it.
        let over = |alpha: f32| Rgba {
            r: 1.,
            g: 1.,
            b: 1.,
            a: alpha,
        };
        let edge = || div().absolute().inset_0().rounded(px(theme::RADIUS_LG));
        // `modalIn` / `modalOut`: `scale(0.97) translateY(4px)` about the
        // centre, with the opacity. GPUI cannot scale the contents, so while
        // the dialog moves the panel's surface is drawn at the scale, the
        // contents stay at full size and in place, clipped to it. That needs
        // the contents' height, measured on an earlier frame; until there is
        // one, and whenever the dialog is at rest, the contents size the
        // panel as usual.
        let width = self.width.unwrap_or(self.size.width());
        let measured = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "height".into()),
            cx,
            |_, _| None::<f32>,
        );
        let (scale, drop) = motion::modal_pop(shown);
        let frame = (*measured.read(cx))
            .filter(|_| self.presence.animating())
            .map(|height| motion::scaled_frame(width, height, scale, (0.5, 0.5)));
        let measure = canvas(
            move |bounds, _, cx| {
                measured.update(cx, |height, _| {
                    *height = Some(f32::from(bounds.size.height))
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        let mut content = div()
            .w(px(width))
            .flex()
            .flex_col()
            .child(measure)
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .line_height(px(theme::LINE_HEIGHT_MD))
            .text_color(theme.text_secondary())
            .child(
                div()
                    .relative()
                    .h(px(HEADER))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text(theme::TEXT_LG)
                            .line_height(px(theme::LINE_HEIGHT_MD))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text())
                            .child(self.title),
                    )
                    .child(close_button(self.on_close.clone(), cx))
                    // `box-shadow: 0 -1px 0 var(--color-border-secondary) inset`
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(1.))
                            .bg(theme.border_secondary()),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .max_h(px(body_height.max(0.)))
                    .child(
                        div()
                            .id("modal-body")
                            .max_h(px(body_height.max(0.)))
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .pt(px(BODY_PADDING))
                            .pb(px(BODY_PADDING))
                            .pl(px(BODY_PADDING))
                            // `scrollbar-gutter: stable` keeps the gutter.
                            .pr(px(BODY_PADDING + super::scrollbar::GUTTER))
                            .children(self.body),
                    )
                    .child(super::scrollbar("modal-scrollbar", &scroll, window, cx)),
            );
        if has_footer {
            content = content.child(
                div()
                    .h(px(FOOTER))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .bg(theme.footer_surface())
                    .border_t_1()
                    .border_color(theme.border_secondary())
                    .children(self.footer),
            );
        }
        let mut panel = div()
            .id(self.id.clone())
            .relative()
            .flex_none()
            .overflow_hidden()
            .top(px(drop))
            .opacity(shown)
            .rounded(px(theme::RADIUS_LG))
            .bg(theme.glass())
            .shadow(theme::panel_shadow())
            // A press inside the panel is not a press on the backdrop.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        panel = match frame {
            // Centred by the backdrop, so the smaller frame stays centred.
            Some((left, top, scaled_width, scaled_height)) => panel
                .w(px(scaled_width))
                .h(px(scaled_height))
                .child(content.absolute().left(px(-left)).top(px(-top))),
            None => panel
                .w(px(width))
                .max_h(px(max_height))
                .flex()
                .flex_col()
                .child(content.relative().max_h(px(max_height))),
        };
        panel = panel
            .child(edge().border_1().border_color(over(0.10)))
            .child(edge().border_l_1().border_r_1().border_color(over(0.0556)))
            .child(edge().border_t_1().border_color(over(0.1667)));

        let backdrop = rgba(theme::MODAL_BACKDROP);
        let mut layer = div()
            .id(ElementId::NamedChild(
                std::sync::Arc::new(self.id),
                "backdrop".into(),
            ))
            .occlude()
            .w(viewport.width)
            .h(viewport.height)
            .flex()
            .items_center()
            .justify_center()
            .bg(Rgba {
                a: backdrop.a * self.presence.backdrop(),
                ..backdrop
            })
            .key_context("Modal")
            // Nothing beneath the dialog scrolls while it is up.
            .on_scroll(|_, _, cx| cx.stop_propagation());
        if let Some(focus) = &self.focus {
            layer = layer.track_focus(focus);
        }
        if let Some(handler) = self.on_close.filter(|_| !self.presence.closing()) {
            let escape = handler.clone();
            layer = layer
                .on_action(move |_: &Close, window, cx| escape(window, cx))
                .on_click(move |_, window, cx| handler(window, cx));
        }
        deferred(
            anchored()
                .position(point(px(0.), px(0.)))
                .child(layer.child(panel)),
        )
        .with_priority(self.priority)
    }
}

#[cfg(test)]
mod tests {
    use super::Presence;
    use std::time::{Duration, Instant};

    #[test]
    fn a_dialog_fades_in_then_out_and_is_finished_only_after_its_exit() {
        let mut presence = Presence::new();
        assert!(presence.animating() && !presence.closing());
        presence.opened = Instant::now() - Duration::from_millis(200);
        assert_eq!(presence.panel(), 1.);
        assert_eq!(presence.backdrop(), 1.);
        assert!(!presence.animating() && !presence.finished());
        presence.close();
        assert!(presence.closing() && !presence.finished());
        let started = presence.closing;
        presence.close();
        assert_eq!(presence.closing, started);
        presence.closing = Some(Instant::now() - Duration::from_millis(130));
        assert!(presence.finished());
        assert_eq!(presence.panel(), 0.);
    }
}
