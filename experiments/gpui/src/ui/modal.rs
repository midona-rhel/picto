//! The centred dialog (`GlassModal.tsx`): a dimmed backdrop over the whole
//! window and a glass panel with a title, a body and an optional footer.
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::{rc::Rc, time::Instant};

mod host;
pub use host::{Dialog, Host};

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
    /// The width the body's contents get: the panel's inside its border,
    /// less the body's padding and its scrollbar gutter.
    pub fn content_width(self) -> f32 {
        self.width() - 2. * BORDER - 2. * BODY_PADDING - super::scrollbar::GUTTER
    }
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
/// `.panel`'s 1px glass border, which the contents sit inside.
const BORDER: f32 = 1.;
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
    /// `flush`: the body has no padding, only the scrollbar's gutter.
    flush: bool,
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
        priority: super::layer::DIALOG,
        flush: false,
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
    /// `flush`: content that reaches the body's edges, such as a list.
    pub fn flush(mut self) -> Self {
        self.flush = true;
        self
    }
    /// Dialogs opened over another one draw above it.
    pub fn layer(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }
}

impl RenderOnce for Modal {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // A dialog that has faded out draws nothing, so that it cannot take
        // a click in the frames before its owner drops it.
        if self.presence.finished() {
            return gpui::Empty.into_any_element();
        }
        let theme = theme::theme(cx);
        if self.presence.animating() {
            window.request_animation_frame();
        }
        let shown = self.presence.panel();
        let max_height = f32::from(window.viewport_size().height) * self.size.max_height();
        let has_footer = !self.footer.is_empty();
        let padding = if self.flush { 0. } else { BODY_PADDING };
        let body_height = max_height - 2. * BORDER - HEADER - if has_footer { FOOTER } else { 0. };
        let scroll = window
            .use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "body".into()),
                cx,
                |_, _| ScrollHandle::new(),
            )
            .read(cx)
            .clone();
        // The body's own focus, so Tab can tell what it holds.
        let body_focus = window
            .use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "body-focus".into()),
                cx,
                |_, cx| cx.focus_handle(),
            )
            .read(cx)
            .clone();
        super::focus::follow(&scroll, &body_focus, window, cx);

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
            .map(|height| motion::scaled_frame(width, height + 2. * BORDER, scale, (0.5, 0.5)));
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
            .w(px(width - 2. * BORDER))
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
                    .child({
                        // `WindowCloseButton`, which closes the dialog.
                        let close = self.on_close.clone();
                        super::window_chrome::close_button("window-close", move |_, window, cx| {
                            if let Some(close) = &close {
                                close(window, cx);
                            }
                        })
                    })
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
                            .track_focus(&body_focus)
                            .pt(px(padding))
                            .pb(px(padding))
                            .pl(px(padding))
                            // `scrollbar-gutter: stable` keeps the gutter.
                            .pr(px(padding + super::scrollbar::GUTTER))
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
        // `.panel`: the glass, with its edges.
        let mut panel = super::glass_panel(self.id.clone(), theme::RADIUS_LG, cx)
            .relative()
            .flex_none()
            .overflow_hidden()
            .top(px(drop))
            .opacity(shown)
            // A press inside the panel is not a press on the backdrop.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        panel = match frame {
            // Centred by the backdrop, so the smaller frame stays centred.
            Some((left, top, scaled_width, scaled_height)) => {
                panel.w(px(scaled_width)).h(px(scaled_height)).child(
                    content
                        .absolute()
                        .left(px(BORDER - left))
                        .top(px(BORDER - top)),
                )
            }
            None => panel
                .w(px(width))
                .max_h(px(max_height))
                .p(px(BORDER))
                .flex()
                .flex_col()
                .child(content.relative().max_h(px(max_height - 2. * BORDER))),
        };

        let mut layer = backdrop(
            ElementId::NamedChild(std::sync::Arc::new(self.id), "backdrop".into()),
            self.presence.backdrop(),
            self.focus.as_ref(),
            window,
        );
        if let Some(handler) = self.on_close.filter(|_| !self.presence.closing()) {
            let escape = handler.clone();
            layer = layer
                .on_action(move |_: &Close, window, cx| escape(window, cx))
                .on_click(move |_, window, cx| handler(window, cx));
        }
        over_window(layer.child(panel), self.priority).into_any_element()
    }
}

/// `.backdrop`: the dimmed layer over the whole window that a dialog's
/// panel sits centred on, at `strength` (0 to 1) of
/// `--modal-backdrop`. It takes every press and scroll beneath it and
/// holds the dialog's keys (`Modal`: Escape is [`Close`]); with `focus` it
/// holds the keyboard while no field in the dialog has it and keeps Tab
/// and Shift-Tab among the dialog's controls. The dialog adds what Escape
/// and a click on it do.
pub fn backdrop(
    id: impl Into<ElementId>,
    strength: f32,
    focus: Option<&FocusHandle>,
    window: &Window,
) -> Stateful<Div> {
    let viewport = window.viewport_size();
    let backdrop = rgba(theme::MODAL_BACKDROP);
    let layer = div()
        .id(id)
        .occlude()
        .w(viewport.width)
        .h(viewport.height)
        .flex()
        .items_center()
        .justify_center()
        .bg(Rgba {
            a: backdrop.a * strength,
            ..backdrop
        })
        .key_context("Modal")
        // Nothing beneath the dialog scrolls while it is up.
        .on_scroll(|_, _, cx| cx.stop_propagation());
    match focus {
        Some(focus) => super::focus::scope(layer.track_focus(focus), focus),
        None => layer,
    }
}

/// A dialog's [`backdrop`], with its panel, over the whole window on
/// `priority` (`ui::layer::DIALOG`, more for one opened over another).
pub fn over_window(layer: Stateful<Div>, priority: usize) -> impl IntoElement {
    deferred(anchored().position(point(px(0.), px(0.))).child(layer)).with_priority(priority)
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
