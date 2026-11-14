use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;
use std::time::Duration;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type HoverHandler = Box<dyn Fn(bool, &mut Window, &mut App) + 'static>;

/// Which of Picto's glyph buttons this is. Each look ports one production
/// class: its size, its colours at rest, under the pointer and pressed, and
/// its transition, which differ from class to class and are kept as they are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// `iconButton.module.css` `.iconBtn`: 24px; the glyph rests at 94% and
    /// lifts to full strength over an 8% overlay on hover; a press, or the
    /// toggled state at rest (`.iconBtnActive`), shows the 14% overlay
    /// instead; disabled dims to 20%. 150 ms on `--ease-default`.
    Icon,
    /// `.querySmallBtn`, the quiet glyph button of table rows and job
    /// strips: the secondary text colour at 60%, rising to full strength
    /// over the hover overlay. Toggled (`.querySmallBtnActive`) it rests at
    /// full strength in the primary text colour; disabled it fades to 20%.
    /// 100 ms `ease`.
    Small,
    /// `OverlayShell.module.css` `.pinBtn`: a picker header's 24px button
    /// whose subtle overlay shows under the pointer over 120 ms `ease`.
    Pin,
    /// `WindowCloseButton`: a 28px square with a thin 16px cross, over the
    /// subtle overlay under the pointer and the pressed overlay while held,
    /// changing over 150 ms on `--ease-default`.
    WindowClose,
    /// `WindowControls`' close button (`.btn.destructiveClose`): the
    /// window-close look, turning the system's red with a white cross under
    /// the pointer, and a darker red while held.
    CaptionClose,
    /// `GridFilterMenu.module.css` `.filterAction` and `.addButton`: 24px,
    /// no fill at rest and the subtle overlay when hovered or toggled, with
    /// no transition; disabled it fades to 25%.
    Filter,
    /// `VideoPlayer.module.css` `.icBtn`: the player's 32px control, white
    /// on the dark controls. Transparent at rest with the glyph at 80%;
    /// hovered it takes a 15% white fill and the glyph lifts to 95%; pressed
    /// the fill is 25%; toggled (`.icBtnActive`) keeps the glyph at 95%.
    /// 150 ms `ease`; the glyph's colour is not transitioned. Its tooltip
    /// opens above it, and a click on it goes no further.
    Media,
    /// `SmartFolderModal.module.css` `.conditionButton`: 24px, ringed and
    /// filled with the text colour at 15% and 5%, 20% and 10% under the
    /// pointer, filled 15% pressed; 120 ms `ease`; disabled at 30%.
    Condition,
    /// `Inspector.module.css` `.tagAddBtn`: the "+" after a row of chips,
    /// 26px, at half strength until hovered (opacity, 200 ms on
    /// `--ease-default`).
    TagAdd,
    /// `SubscriptionsScreen.module.css` `.subscriptionDestinationAdd`: 26px
    /// on the hover surface, the active surface under the pointer with the
    /// glyph turning from the secondary to the primary text colour.
    ChipAdd,
    /// `InspectorField.module.css` `.urlActionBtn`: a 31 × 32 segment at the
    /// end of a source row, ruled off on its left, its glyph at half
    /// strength until hovered over the control surface.
    UrlAction,
    /// `TaskDialog.module.css` `.iconButton`: 26px, the secondary text
    /// colour, turning to the primary one over the hover surface, with no
    /// transition.
    Task,
}

impl Look {
    /// The transition of every property the look animates: its duration and
    /// timing function.
    pub fn transition(self) -> (Duration, motion::CubicBezier) {
        match self {
            Look::Icon | Look::WindowClose | Look::CaptionClose => {
                (motion::BUTTON_HOVER, motion::EASE_DEFAULT)
            }
            Look::Media => (motion::BUTTON_HOVER, motion::EASE),
            Look::Small => (motion::SMALL_BUTTON, motion::EASE),
            Look::Pin => (motion::PIN_BUTTON, motion::EASE),
            Look::Condition => (motion::CONDITION_BUTTON, motion::EASE),
            Look::TagAdd => (motion::TAG_ADD_BUTTON, motion::EASE_DEFAULT),
            Look::Filter | Look::ChipAdd | Look::UrlAction | Look::Task => {
                (Duration::ZERO, motion::EASE_DEFAULT)
            }
        }
    }

    /// Whether the look has a pressed state (`:active`).
    fn pressable(self) -> bool {
        matches!(
            self,
            Look::Icon | Look::WindowClose | Look::CaptionClose | Look::Media | Look::Condition
        )
    }

    /// The colours, sizes and strengths the look draws with.
    fn paint(self, theme: &theme::Theme, toggled: bool) -> Paint {
        let clear = theme::clear;
        let base = Paint {
            width: theme::ICON_BUTTON,
            height: theme::ICON_BUTTON,
            radius: theme::RADIUS_SM,
            glyph: 16.,
            fill: clear(theme.contrast(0.)),
            fill_hover: clear(theme.contrast(0.)),
            fill_press: clear(theme.contrast(0.)),
            glyph_color: theme.text(),
            glyph_hover: theme.text(),
            opacity: 1.,
            opacity_hover: 1.,
            glyph_opacity: 1.,
            glyph_opacity_hover: 1.,
            disabled_opacity: 1.,
            border: None,
        };
        match self {
            Look::Icon => Paint {
                fill: theme.contrast(if toggled { 14. } else { 0. }),
                fill_hover: theme.contrast(8.),
                fill_press: theme.contrast(14.),
                glyph_opacity: if toggled { 1. } else { 0.94 },
                disabled_opacity: 0.2,
                ..base
            },
            Look::Small => {
                let color = if toggled {
                    theme.text()
                } else {
                    theme.text_secondary()
                };
                Paint {
                    glyph: 14.,
                    fill: clear(theme.hover_overlay()),
                    fill_hover: theme.hover_overlay(),
                    fill_press: theme.hover_overlay(),
                    glyph_color: color,
                    glyph_hover: color,
                    opacity: if toggled { 1. } else { 0.6 },
                    disabled_opacity: 0.2,
                    ..base
                }
            }
            Look::Pin => Paint {
                radius: theme::RADIUS_CONTROL,
                glyph: 14.,
                fill: clear(theme.subtle_overlay()),
                fill_hover: theme.subtle_overlay(),
                fill_press: theme.subtle_overlay(),
                ..base
            },
            Look::WindowClose => Paint {
                width: theme::CONTROL_MD,
                height: theme::CONTROL_MD,
                radius: theme::RADIUS_CONTROL,
                fill: theme.contrast(0.),
                fill_hover: theme.subtle_overlay(),
                fill_press: theme.pressed_overlay(),
                disabled_opacity: 0.4,
                ..base
            },
            Look::CaptionClose => Paint {
                fill_hover: rgba(theme::CAPTION_CLOSE_HOVER),
                fill_press: rgba(theme::CAPTION_CLOSE_PRESS),
                glyph_hover: theme.on_primary(),
                ..Look::WindowClose.paint(theme, toggled)
            },
            Look::Filter => Paint {
                radius: theme::RADIUS_MD,
                fill: if toggled {
                    theme.subtle_overlay()
                } else {
                    clear(theme.subtle_overlay())
                },
                fill_hover: theme.subtle_overlay(),
                fill_press: theme.subtle_overlay(),
                disabled_opacity: 0.25,
                ..base
            },
            Look::Media => {
                let rest = rgba(theme::MEDIA_BUTTON_HOVER);
                Paint {
                    width: 32.,
                    height: 32.,
                    radius: theme::RADIUS_CONTROL,
                    fill: clear(rest),
                    fill_hover: rest,
                    fill_press: rgba(theme::MEDIA_BUTTON_ACTIVE),
                    glyph_color: rgba(if toggled {
                        theme::MEDIA_GLYPH_STRONG
                    } else {
                        theme::MEDIA_GLYPH
                    }),
                    glyph_hover: rgba(theme::MEDIA_GLYPH_STRONG),
                    ..base
                }
            }
            Look::Condition => Paint {
                radius: 3.,
                fill: theme.text_at(5.),
                fill_hover: theme.text_at(10.),
                fill_press: theme.text_at(15.),
                disabled_opacity: 0.3,
                border: Some(Border {
                    left_only: false,
                    rest: theme.text_at(15.),
                    hover: theme.text_at(20.),
                }),
                ..base
            },
            Look::TagAdd => Paint {
                width: 26.,
                height: theme::CONTROL_SM,
                radius: theme::RADIUS_MD,
                glyph: 15.,
                opacity: 0.5,
                ..base
            },
            Look::ChipAdd => Paint {
                width: theme::CONTROL_SM,
                height: theme::CONTROL_SM,
                radius: theme::RADIUS_MD,
                glyph: 14.,
                fill: theme.surface_hover(),
                fill_hover: theme.surface_active(),
                fill_press: theme.surface_active(),
                glyph_color: theme.text_secondary(),
                ..base
            },
            Look::Task => Paint {
                width: 26.,
                height: 26.,
                radius: theme::RADIUS_MD,
                glyph: 15.,
                fill: clear(theme.surface_hover()),
                fill_hover: theme.surface_hover(),
                fill_press: theme.surface_hover(),
                glyph_color: theme.text_secondary(),
                ..base
            },
            Look::UrlAction => Paint {
                width: 31.,
                height: 32.,
                radius: 0.,
                glyph: 13.,
                fill: clear(theme.control_surface()),
                fill_hover: theme.control_surface(),
                fill_press: theme.control_surface(),
                glyph_opacity: 0.5,
                border: Some(Border {
                    left_only: true,
                    rest: theme.border_secondary(),
                    hover: theme.border_secondary(),
                }),
                ..base
            },
        }
    }
}

/// A look's 1px border: all round, or on the left only.
#[derive(Clone, Copy)]
struct Border {
    left_only: bool,
    rest: Rgba,
    hover: Rgba,
}

/// A look resolved against the theme.
#[derive(Clone, Copy)]
struct Paint {
    width: f32,
    height: f32,
    radius: f32,
    /// The glyph's size unless the caller sets one.
    glyph: f32,
    fill: Rgba,
    fill_hover: Rgba,
    fill_press: Rgba,
    /// The glyph's colour, switched (not faded) under the pointer.
    glyph_color: Rgba,
    glyph_hover: Rgba,
    /// The whole button's strength (CSS `opacity`), at rest and hovered.
    opacity: f32,
    opacity_hover: f32,
    /// The glyph's own strength, at rest and hovered.
    glyph_opacity: f32,
    glyph_opacity_hover: f32,
    /// The whole button's strength while disabled.
    disabled_opacity: f32,
    border: Option<Border>,
}

impl Paint {
    /// The fill at `hover` and `press` (each 0 to 1, already eased): from
    /// rest toward the hover fill, then from there toward the pressed one.
    fn fill_at(&self, hover: f32, press: f32) -> Rgba {
        theme::blend(
            theme::blend(self.fill, self.fill_hover, hover),
            self.fill_press,
            press,
        )
    }

    /// The whole button's strength at `hover`, dimmed toward the disabled
    /// strength by `dimmed`.
    fn opacity_at(&self, hover: f32, dimmed: f32) -> f32 {
        let lit = self.opacity + (self.opacity_hover - self.opacity) * hover;
        lit + (self.disabled_opacity - lit) * dimmed
    }
}

/// What a glyph button keeps between frames.
struct State {
    hover: motion::Fade,
    press: motion::Fade,
    /// On while the button is disabled.
    dimmed: motion::Fade,
}

/// What the button shows.
enum Content {
    Glyph(&'static str),
    /// `.icBtn.rateButton`: a short label in the monospace caption, as wide
    /// as it is, at least 28px, 4px either side.
    Label(SharedString),
    /// A glyph the caller draws itself, in the look's glyph colour.
    Element(AnyElement),
}

/// The shared glyph button, in one of its [`Look`]s. A press on it goes no
/// further, so a button in a titlebar strip never drags the window and one
/// in a picker's header never drags the picker. Disabled, it shows no hover
/// and no tooltip.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    look: Look,
    content: Content,
    glyph: Option<f32>,
    /// A square size other than the look's.
    size: Option<f32>,
    radius: Option<f32>,
    disabled: bool,
    toggled: bool,
    badge: Option<usize>,
    tip: Option<Tip>,
    on_click: Option<ClickHandler>,
    on_hover: Option<HoverHandler>,
    /// The part of the window's frame the button stands for, which the
    /// system then handles (Windows' caption buttons).
    window_control: Option<WindowControlArea>,
    children: Vec<AnyElement>,
}

pub fn icon_button(id: impl Into<ElementId>, icon: &'static str) -> IconButton {
    IconButton {
        id: id.into(),
        look: Look::Icon,
        content: Content::Glyph(icon),
        glyph: None,
        size: None,
        radius: None,
        disabled: false,
        toggled: false,
        badge: None,
        tip: None,
        on_click: None,
        on_hover: None,
        window_control: None,
        children: Vec::new(),
    }
}

impl IconButton {
    pub fn look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The glyph's size, for the few buttons whose artwork is not the
    /// look's.
    pub fn glyph(mut self, size: f32) -> Self {
        self.glyph = Some(size);
        self
    }

    /// The button's square size, for the few that are not the look's.
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    /// The corner radius, for the few that are not the look's.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    /// Shows the button as switched on, as the look's active class does.
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.toggled = toggled;
        self
    }

    /// A label in place of the glyph.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.content = Content::Label(label.into());
        self
    }

    /// A glyph drawn by the caller in place of an icon.
    pub fn glyph_element(mut self, glyph: impl IntoElement) -> Self {
        self.content = Content::Element(glyph.into_any_element());
        self
    }

    /// A count in the button's top-right corner, hidden at zero.
    pub fn badge(mut self, count: usize) -> Self {
        self.badge = (count > 0).then_some(count);
        self
    }

    pub fn tip(mut self, tip: Tip) -> Self {
        self.tip = Some(tip);
        self
    }

    /// Something more inside the button, such as a probe of its bounds.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Makes the button the system's minimise, maximise or close button: on
    /// Windows the system then performs it (and offers its snap layouts over
    /// maximise), so such a button takes no click handler there.
    pub fn window_control(mut self, area: WindowControlArea) -> Self {
        self.window_control = Some(area);
        self
    }

    /// Told when the pointer enters or leaves the button.
    pub fn on_hover(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let look = self.look;
        let (duration, curve) = look.transition();
        let paint = look.paint(&theme, self.toggled);
        let disabled = self.disabled;
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            hover: motion::Fade::default(),
            press: motion::Fade::default(),
            dimmed: motion::Fade::at(disabled),
        });
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        // A button disabled under the pointer loses its hover and press.
        if state.read(cx).dimmed.on() != disabled {
            state.update(cx, |state, _| {
                state.dimmed.set(disabled, duration);
                if disabled {
                    state.hover.set(false, duration);
                    state.press.set(false, duration);
                }
            });
            hint.update(cx, |hint, cx| hint.set(false, cx));
        }
        let (hover, press, dimmed, hovered, animating) = {
            let state = state.read(cx);
            (
                state.hover.value_with(curve),
                state.press.value_with(curve),
                state.dimmed.value_with(curve),
                state.hover.on(),
                state.hover.animating() || state.press.animating() || state.dimmed.animating(),
            )
        };
        if animating || hint.read(cx).animating() {
            window.request_animation_frame();
        }

        // The whole button's strength applies to its fill, border and
        // glyph, not to its tooltip, which is a child of it.
        let opacity = paint.opacity_at(hover, dimmed);
        let faded = |color: Rgba| Rgba {
            a: color.a * opacity,
            ..color
        };
        let (width, height) = match self.size {
            Some(size) => (size, size),
            None => (paint.width, paint.height),
        };
        let radius = self.radius.unwrap_or(paint.radius);
        let glyph_color = if hovered {
            paint.glyph_hover
        } else {
            paint.glyph_color
        };
        let glyph_opacity = (paint.glyph_opacity
            + (paint.glyph_opacity_hover - paint.glyph_opacity) * hover)
            * opacity;

        // It occludes what is behind it, so a press on it reaches nothing
        // there by its bounds; it also stops the press from going on to the
        // window's own listeners (a grid's marquee under a dialog), which
        // the panel holding it can no longer stop once it is occluded.
        let mut button = div()
            .id(self.id.clone())
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .h(px(height))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius))
            .bg(faded(paint.fill_at(hover, press)));
        if let Some(border) = paint.border {
            let color = faded(theme::blend(border.rest, border.hover, hover));
            button = if border.left_only {
                button.border_l_1()
            } else {
                button.border_1()
            }
            .border_color(color);
        }
        button = match self.content {
            Content::Glyph(name) => button.w(px(width)).child(
                super::icon(name, self.glyph.unwrap_or(paint.glyph), glyph_color)
                    .opacity(glyph_opacity),
            ),
            Content::Label(label) => button
                .min_w(px(28.))
                .px(px(4.))
                .font_family(theme::FONT_MONO)
                .text(theme::TEXT_CAPTION)
                .text_color(Rgba {
                    a: glyph_color.a * glyph_opacity,
                    ..glyph_color
                })
                .whitespace_nowrap()
                .child(label),
            Content::Element(glyph) => button
                .w(px(width))
                .child(div().opacity(glyph_opacity).child(glyph)),
        };
        button = button.children(self.children);
        if let Some(area) = self.window_control {
            button = button.window_control_area(area);
        }
        if let Some(count) = self.badge {
            button = button.child(
                div()
                    .absolute()
                    .top(px(1.))
                    .right(px(1.))
                    .min_w(px(12.))
                    .h(px(12.))
                    .px(px(3.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .bg(theme.primary())
                    .text_color(theme.on_primary())
                    .text(theme::TEXT_XS)
                    .line_height(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(count.to_string()),
            );
        }
        if disabled {
            return button;
        }
        let focus = super::focus::control(&self.id, window, cx);
        // `SmartFolderModal`'s `.conditionButton:focus-visible`; production's
        // other glyph buttons draw no ring.
        if look == Look::Condition {
            let ring = super::focus::Ring {
                width: 1.,
                offset: 1.,
                color: theme.text(),
            };
            let border = if paint.border.is_some() { 1. } else { 0. };
            button = button.children(focus.ring(ring, radius, border));
        }
        if let Some(tip) = &self.tip {
            let tip = if look == Look::Media {
                tip.clone().above()
            } else {
                tip.clone()
            };
            button = button.children(hint.read(cx).layer(&tip, height, cx));
        }
        let (entered, down, up, out) = (state.clone(), state.clone(), state.clone(), state);
        let release = move |state: &Entity<State>, cx: &mut App| {
            if state.read(cx).press.on() {
                state.update(cx, |state, cx| {
                    state.press.set(false, duration);
                    cx.notify();
                });
            }
        };
        let on_hover = self.on_hover;
        let mut button = button
            .cursor_pointer()
            .on_hover(move |hovered, window, cx| {
                entered.update(cx, |state, cx| {
                    state.hover.set(*hovered, duration);
                    cx.notify();
                });
                hint.update(cx, |hint, cx| hint.set(*hovered, cx));
                if let Some(handler) = &on_hover {
                    handler(*hovered, window, cx);
                }
            });
        if look.pressable() {
            button = button
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    down.update(cx, |state, cx| {
                        state.press.set(true, duration);
                        cx.notify();
                    });
                })
                .on_mouse_up(MouseButton::Left, move |_, _, cx| release(&up, cx))
                .on_mouse_up_out(MouseButton::Left, move |_, _, cx| release(&out, cx));
        }
        if let Some(handler) = self.on_click {
            let handler: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> = Rc::from(handler);
            let pressed = handler.clone();
            button = if look == Look::Media {
                button.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    handler(event, window, cx);
                })
            } else {
                button.on_click(move |event, window, cx| handler(event, window, cx))
            };
            button = focus.attach(
                button,
                super::focus::Kind::Button,
                move |event, window, cx| pressed(event, window, cx),
            );
        } else {
            button = focus.attach(button, super::focus::Kind::Button, |_, _, _| {});
        }
        button
    }
}

#[cfg(test)]
mod tests {
    use super::Look;
    use crate::theme::{DARK, blend};
    use gpui::Rgba;

    fn same(a: Rgba, b: Rgba) -> bool {
        [a.r - b.r, a.g - b.g, a.b - b.b, a.a - b.a]
            .iter()
            .all(|d| d.abs() < 1e-4)
    }

    #[test]
    fn each_look_keeps_its_own_transition() {
        let ms = |look: Look| look.transition().0.as_millis();
        assert_eq!(ms(Look::Icon), 150);
        assert_eq!(ms(Look::WindowClose), 150);
        assert_eq!(ms(Look::Media), 150);
        assert_eq!(ms(Look::Small), 100);
        assert_eq!(ms(Look::Pin), 120);
        assert_eq!(ms(Look::Condition), 120);
        assert_eq!(ms(Look::TagAdd), 200);
        assert_eq!(ms(Look::Filter), 0);
        assert_eq!(ms(Look::ChipAdd), 0);
        assert_eq!(ms(Look::UrlAction), 0);
    }

    #[test]
    fn the_icon_look_moves_between_its_overlays() {
        let paint = Look::Icon.paint(&DARK, false);
        assert_eq!(paint.fill_at(0., 0.).a, 0.);
        assert!(same(paint.fill_at(1., 0.), DARK.contrast(8.)));
        assert!(same(paint.fill_at(1., 1.), DARK.contrast(14.)));
        // Toggled it rests on the pressed overlay.
        let toggled = Look::Icon.paint(&DARK, true);
        assert!(same(toggled.fill_at(0., 0.), DARK.contrast(14.)));
        // Disabled dims everything to a fifth.
        assert!((paint.opacity_at(0., 1.) - 0.2).abs() < 1e-6);
        assert_eq!(paint.opacity_at(1., 0.), 1.);
    }

    #[test]
    fn the_small_look_fades_its_fill_with_the_whole_button() {
        // `.querySmallBtn` fades the whole button, fill included, from 60%.
        let paint = Look::Small.paint(&DARK, false);
        let half = paint.fill_at(0.5, 0.).a * paint.opacity_at(0.5, 0.);
        let expected = DARK.hover_overlay().a * 0.5 * (0.6 + 0.4 * 0.5);
        assert!((half - expected).abs() < 1e-4, "{half} {expected}");
        assert_eq!(paint.opacity_at(0., 0.), 0.6);
        assert!((paint.opacity_at(0., 1.) - 0.2).abs() < 1e-6);
        assert_eq!(Look::Small.paint(&DARK, true).opacity_at(0., 0.), 1.);
    }

    #[test]
    fn the_media_look_is_white_on_the_controls() {
        let paint = Look::Media.paint(&DARK, false);
        let hover = gpui::rgba(crate::theme::MEDIA_BUTTON_HOVER);
        assert!(same(paint.fill_at(1., 0.), hover));
        assert!(same(
            paint.fill_at(1., 1.),
            gpui::rgba(crate::theme::MEDIA_BUTTON_ACTIVE)
        ));
        // Fading in from clear keeps the white.
        let half = paint.fill_at(0.5, 0.);
        assert_eq!(half, blend(paint.fill, hover, 0.5));
        assert_eq!((half.r, half.g, half.b), (1., 1., 1.));
    }
}
