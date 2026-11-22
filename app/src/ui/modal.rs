//! Draws the centred dialog: a dimmed backdrop over the whole window and, on
//! it, a glass panel with a title, a body and, when the dialog has buttons, a
//! footer.
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{rc::Rc, time::Instant};

mod host;
pub use host::{Dialog, Host};

use super::keys::{self, Dismiss, Key};

pub fn bind(cx: &mut App) {
    keys::bind(cx, "Modal", &[Key::Escape]);
}

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// Holds when a dialog opened and, once it was dismissed, when it began to fade
/// out. The view that owns the dialog keeps one `Presence` per dialog, and
/// removes the dialog when [`Presence::finished`] returns true.
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
        Self::since(Instant::now())
    }

    /// Returns the presence of a dialog that opened at `opened`.
    pub fn since(opened: Instant) -> Self {
        Self {
            opened,
            closing: None,
        }
    }

    /// Starts the fade-out, which takes `motion::MODAL_OUT` (120 ms). A second
    /// call changes nothing.
    pub fn close(&mut self) {
        self.closing.get_or_insert_with(Instant::now);
    }

    pub fn closing(&self) -> bool {
        self.closing.is_some()
    }

    /// Returns whether the fade-out has ended, so that the dialog can be
    /// removed.
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

    /// Returns how far the panel has come in, from 0 (hidden) to 1 (in place).
    /// On the way in it follows `motion::EASE_DEFAULT`, and on the way out it
    /// follows `motion::EASE_IN`.
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

    /// Returns the backdrop's opacity, from 0 to 1. It rises along
    /// `motion::EASE_OUT` and falls along `motion::EASE_IN`.
    pub(super) fn backdrop(&self) -> f32 {
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

/// The four sizes of a dialog. A size sets the panel's width and the largest
/// share of the window's height that the panel may take.
#[derive(Clone, Copy)]
pub enum Size {
    Small,
    Medium,
    Large,
    /// 900px wide, or the window's width less 48px if that is smaller. Exactly
    /// 600px tall, or the window's height less 48px if that is smaller,
    /// whatever the dialog holds.
    ExtraLarge,
}

impl Size {
    /// Returns the width that the body's contents get: the panel's width, less
    /// its border on both sides, the body's padding on both sides and the
    /// scrollbar's gutter.
    pub fn content_width(self) -> f32 {
        self.width() - 2. * BORDER - 2. * BODY_PADDING - super::scrollbar::GUTTER
    }
    fn width(self) -> f32 {
        match self {
            Size::Small => 400.,
            Size::Medium => 560.,
            Size::Large => 680.,
            Size::ExtraLarge => 900.,
        }
    }
    fn max_height(self) -> f32 {
        match self {
            Size::Small | Size::Medium => 0.8,
            Size::Large | Size::ExtraLarge => 0.85,
        }
    }
    /// Returns the panel's width in a window that is `viewport` wide.
    fn width_in(self, viewport: f32) -> f32 {
        match self {
            Size::ExtraLarge => self.width().min(viewport - 48.),
            _ => self.width(),
        }
    }
    /// Returns the panel's height in a window that is `viewport` tall, for the
    /// one size that has a fixed height (`ExtraLarge`). Returns `None` for the
    /// other sizes.
    fn fixed_height(self, viewport: f32) -> Option<f32> {
        matches!(self, Size::ExtraLarge).then(|| 600_f32.min(viewport - 48.))
    }
}

const HEADER: f32 = 48.;
const FOOTER: f32 = 44.;
/// The width of the panel's glass border. The contents are laid out inside the
/// border.
const BORDER: f32 = 1.;
/// The padding on each side of the body. On the right, the width of the
/// scrollbar's gutter is added to it.
const BODY_PADDING: f32 = 16.;

#[derive(IntoElement)]
pub struct Modal {
    id: ElementId,
    title: SharedString,
    size: Size,
    /// A width that replaces the width of `size`, for a dialog that sets its
    /// own width.
    width: Option<f32>,
    presence: Presence,
    body: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    on_close: Option<Handler>,
    focus: Option<FocusHandle>,
    priority: usize,
    /// Whether the body has no padding. The scrollbar's gutter on the right is
    /// kept.
    flush: bool,
}

/// Returns a dialog with the title `title`. The view that owns the dialog draws
/// it. The dialog covers the whole window, whatever that view's bounds are.
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
    /// Sets the panel's width, in place of the width of its size.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.body.push(child.into_any_element());
        self
    }
    /// Adds a button to the footer. The footer is drawn only when it has at
    /// least one button.
    pub fn footer(mut self, button: impl IntoElement) -> Self {
        self.footer.push(button.into_any_element());
        self
    }
    /// Sets what the close button, Escape and a click on the backdrop do.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
    /// Sets the focus handle that has the keyboard focus while no field in the
    /// dialog has it, so that Escape reaches the dialog.
    pub fn focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }
    /// Removes the body's padding, for content that reaches the body's edges,
    /// such as a list.
    pub fn flush(mut self) -> Self {
        self.flush = true;
        self
    }
    /// Sets the dialog's drawing order. A dialog that is opened over another
    /// one is given a higher `priority`, so that it is drawn above the other.
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
        let viewport = window.viewport_size();
        let fixed_height = self.size.fixed_height(f32::from(viewport.height));
        let max_height =
            fixed_height.unwrap_or(f32::from(viewport.height) * self.size.max_height());
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
        // The body has its own focus handle, so that `focus::follow` can tell
        // whether the control that Tab moved the keyboard focus to is inside
        // the body.
        let body_focus = window
            .use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "body-focus".into()),
                cx,
                |_, cx| cx.focus_handle(),
            )
            .read(cx)
            .clone();
        super::focus::follow(&scroll, &body_focus, window, cx);

        // While the dialog fades in or out, its panel is drawn smaller about
        // its centre (97% of its size when hidden), lower (by up to 4px) and
        // partly transparent. GPUI cannot scale the contents, so during the
        // animation only the panel's surface is drawn at the scale. The
        // contents keep their full size and their place, and are clipped to the
        // surface. For this the height of the contents must be known, and it is
        // measured on an earlier frame. Until it has been measured, and
        // whenever the dialog is not animating, the contents set the panel's
        // size as usual.
        let width = self
            .width
            .unwrap_or(self.size.width_in(f32::from(viewport.width)));
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
                            .child(self.title.clone()),
                    )
                    .child({
                        // The close button, which closes the dialog.
                        let close = self.on_close.clone();
                        super::window_chrome::close_button("window-close", move |_, window, cx| {
                            if let Some(close) = &close {
                                close(window, cx);
                            }
                        })
                    })
                    // A 1px line in the secondary border colour along the
                    // bottom of the header.
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
                    .when(fixed_height.is_some(), |body| {
                        body.h(px(body_height.max(0.)))
                    })
                    .child(
                        div()
                            .id("modal-body")
                            .max_h(px(body_height.max(0.)))
                            .when(fixed_height.is_some(), |body| {
                                body.h(px(body_height.max(0.)))
                            })
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .track_focus(&body_focus)
                            .pt(px(padding))
                            .pb(px(padding))
                            .pl(px(padding))
                            // The right padding includes the scrollbar's
                            // gutter, so the contents have the same width
                            // whether or not the scrollbar is shown.
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
        // While no control in the dialog has the keyboard focus, assistive
        // technology reads the dialog itself as focused.
        let mut panel = dialog_panel(self.id.clone(), theme::RADIUS_LG, self.title.clone(), cx)
            .when_some(self.focus.clone(), |panel, focus| panel.a11y_focus(&focus))
            .relative()
            .flex_none()
            .overflow_hidden()
            .top(px(drop))
            .opacity(shown);
        panel = match frame {
            // The backdrop centres the panel, so the smaller panel stays
            // centred.
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
                .when_some(fixed_height, |panel, height| panel.h(px(height)))
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
                .on_action(move |_: &Dismiss, window, cx| escape(window, cx))
                .on_click(move |_, window, cx| handler(window, cx));
        }
        over_window(layer.child(panel), self.priority).into_any_element()
    }
}

/// Returns a dialog's glass panel with the corner radius `radius`. Assistive
/// technology is told that it is a modal dialog named `label`. A press inside
/// the panel is stopped there, so it is not a press on the [`backdrop`].
/// [`Modal`] uses this panel. A dialog with no title bar
/// ([`super::progress_dialog`], the import task dialog) lays out its own
/// contents in one.
pub fn dialog_panel(
    id: impl Into<ElementId>,
    radius: f32,
    label: impl Into<SharedString>,
    cx: &App,
) -> super::GlassPanel {
    super::glass_panel(id, radius, cx)
        .role(Role::Dialog)
        .aria_modal(true)
        .aria_label(label)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Returns the dimmed layer over the whole window, in which a dialog's panel is
/// centred. Its colour is `theme::MODAL_BACKDROP` at `strength` (0 to 1) of
/// that colour's opacity. It stops every press and scroll from reaching what is
/// beneath it. It has the key context `Modal`, in which Escape is [`Dismiss`].
/// With `focus`, it has the keyboard focus while no field in the dialog has it,
/// and it keeps Tab and Shift-Tab among the dialog's controls. The caller adds
/// what Escape and a click on the layer do.
pub fn backdrop(
    id: impl Into<ElementId>,
    strength: f32,
    focus: Option<&FocusHandle>,
    window: &Window,
) -> Stateful<Div> {
    let viewport = window.viewport_size();
    let backdrop = rgba(theme::MODAL_BACKDROP);
    // A press on the layer closes the dialog. The layer is hidden from
    // assistive technology, whose users close the dialog with the close button
    // or Escape.
    let layer = div()
        .id(id)
        .a11y_presentational()
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
        // Nothing beneath the dialog scrolls while the dialog is open.
        .on_scroll(|_, _, cx| cx.stop_propagation());
    match focus {
        Some(focus) => super::focus::scope(layer.track_focus(focus), focus),
        None => layer,
    }
}

/// Draws a dialog's [`backdrop`], with the panel in it, over the whole window
/// at the drawing order `priority`. That is `ui::layer::DIALOG`, or a higher
/// number for a dialog that is opened over another.
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
