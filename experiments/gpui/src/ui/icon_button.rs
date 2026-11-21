use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;
use std::time::Duration;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type HoverHandler = Box<dyn Fn(bool, &mut Window, &mut App) + 'static>;
type PressHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

/// Which of Picto's glyph buttons this is. Each look copies one CSS class of
/// the Electron app: the button's size, its colours normally, under the
/// pointer and while pressed, and how long the change between them takes.
/// These values differ from class to class. A look keeps the values of its
/// class unless its comment says otherwise.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// `iconButton.module.css` `.iconBtn`: 24px. The glyph is drawn at 94%
    /// strength. Under the pointer the glyph is at full strength and the
    /// button's background is an 8% overlay. While the button is pressed, and
    /// all the time while it is toggled (`.iconBtnActive`), the background is
    /// the 14% overlay. A disabled button is drawn at 20% strength. Changes
    /// take 150 ms on `--ease-default`.
    Icon,
    /// The small glyph button of table rows and job strips. Its glyph is
    /// drawn at full strength in the secondary text colour. Under the
    /// pointer the glyph takes the primary text colour and the button gets
    /// the hover overlay as its background, over 100 ms. When toggled, the
    /// glyph is in the primary text colour all the time. When disabled, the
    /// whole button is drawn at 20% strength, so that only a button that
    /// cannot be pressed looks dimmed. (The Electron app's `.querySmallBtn`
    /// draws an enabled button at 60%, which reads as disabled.)
    Small,
    /// `OverlayShell.module.css` `.pinBtn`: the 24px button in a picker's
    /// header. Under the pointer its background is the subtle overlay. The
    /// change takes 120 ms on `ease`.
    Pin,
    /// `WindowCloseButton`: a 28px square with a thin 16px cross. Its
    /// background is the subtle overlay under the pointer and the pressed
    /// overlay while the button is held down. Changes take 150 ms on
    /// `--ease-default`.
    WindowClose,
    /// `WindowControls`' close button (`.btn.destructiveClose`): the same as
    /// `WindowClose`, except that under the pointer the background is the
    /// system's red and the cross is white, and while the button is held down
    /// the red is darker.
    CaptionClose,
    /// `GridFilterMenu.module.css` `.filterAction` and `.addButton`: 24px.
    /// Normally it has no background. Under the pointer, or while toggled, its
    /// background is the subtle overlay. The change is immediate. A disabled
    /// button is drawn at 25% strength.
    Filter,
    /// `VideoPlayer.module.css` `.icBtn`: the video player's 32px button,
    /// white on the dark controls. Normally it has no background and its glyph
    /// is at 80% strength. Under the pointer the background is white at 15%
    /// and the glyph is at 95%. While pressed the background is white at 25%.
    /// While toggled (`.icBtnActive`) the glyph stays at 95%. The background
    /// changes over 150 ms on `ease`; the glyph's colour changes immediately.
    /// Its tooltip opens above it. A click on it is not passed on to anything
    /// else.
    Media,
    /// `SmartFolderModal.module.css` `.conditionButton`: 24px with a border.
    /// Normally the border is the text colour at 15% and the background is the
    /// text colour at 5%. Under the pointer they are 20% and 10%. While
    /// pressed the background is 15%. Changes take 120 ms on `ease`. A
    /// disabled button is drawn at 30% strength.
    Condition,
    /// `Inspector.module.css` `.tagAddBtn`: the 26px "+" button after a row of
    /// chips. It is drawn at 50% strength, and at full strength under the
    /// pointer. The change takes 200 ms on `--ease-default`.
    TagAdd,
    /// `InspectorField.module.css` `.urlActionBtn`: a 31 × 32 button at the
    /// end of a source's row, with a 1px line along its left edge. Its glyph
    /// is drawn at 50% strength. Under the pointer the glyph is at full
    /// strength and the background is the control surface colour.
    UrlAction,
    /// `TaskDialog.module.css` `.iconButton`: 26px. The glyph is in the
    /// secondary text colour. Under the pointer the glyph is in the primary
    /// text colour and the background is the hover colour. The change is
    /// immediate.
    Task,
    /// `LibrarySwitcher.module.css` `.closeButton`: 24px. The glyph is in the
    /// tertiary text colour. Under the pointer the glyph is in the primary
    /// text colour and the background is the hover colour. The change is
    /// immediate. The diagnostics panel's `.iconButton` is the same at 26px.
    Close,
    /// `AiTaggerPanel.module.css` `.navButton`: a 24px chevron in the text
    /// colour. Under the pointer the background is the hover colour. The
    /// change is immediate. A disabled button is drawn at 28% strength.
    Nav,
    /// `ShortcutsPanel.module.css` `.clearButton`: the 22px button inside a
    /// shortcut's field. The glyph is in the tertiary text colour. Under the
    /// pointer the glyph is in the primary text colour and the background is
    /// the text colour at 10%. The change is immediate.
    ShortcutClear,
}

impl Look {
    /// How long the look's changes take, and the timing curve they follow.
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
            Look::Filter
            | Look::UrlAction
            | Look::Task
            | Look::Close
            | Look::Nav
            | Look::ShortcutClear => (Duration::ZERO, motion::EASE_DEFAULT),
        }
    }

    /// Whether the look is drawn differently while the button is held down
    /// (`:active`).
    fn pressable(self) -> bool {
        matches!(
            self,
            Look::Icon | Look::WindowClose | Look::CaptionClose | Look::Media | Look::Condition
        )
    }

    /// The sizes, colours and strengths the look is drawn with in `theme`.
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
                // A button that can be pressed is drawn at full strength,
                // so that only a disabled one looks dimmed. Under the
                // pointer its glyph takes the primary text colour.
                Paint {
                    glyph: 14.,
                    fill: clear(theme.hover_overlay()),
                    fill_hover: theme.hover_overlay(),
                    fill_press: theme.hover_overlay(),
                    glyph_color: color,
                    glyph_hover: theme.text(),
                    opacity: 1.,
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
            Look::Close => Paint {
                glyph: 14.,
                fill: clear(theme.surface_hover()),
                fill_hover: theme.surface_hover(),
                fill_press: theme.surface_hover(),
                glyph_color: theme.text_tertiary(),
                ..base
            },
            Look::Nav => Paint {
                radius: 5.,
                glyph: 15.,
                fill: clear(theme.surface_hover()),
                fill_hover: theme.surface_hover(),
                fill_press: theme.surface_hover(),
                disabled_opacity: 0.28,
                ..base
            },
            Look::ShortcutClear => Paint {
                width: 22.,
                height: 22.,
                radius: theme::RADIUS_CONTROL,
                glyph: 13.,
                fill: clear(theme.contrast(10.)),
                fill_hover: theme.contrast(10.),
                fill_press: theme.contrast(10.),
                glyph_color: theme.text_tertiary(),
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

/// A look's 1px border: on all four sides, or on the left side only.
#[derive(Clone, Copy)]
struct Border {
    left_only: bool,
    rest: Rgba,
    hover: Rgba,
}

/// The values one look is drawn with in one theme.
#[derive(Clone, Copy)]
struct Paint {
    width: f32,
    height: f32,
    radius: f32,
    /// The glyph's size, unless the caller sets another.
    glyph: f32,
    fill: Rgba,
    fill_hover: Rgba,
    fill_press: Rgba,
    /// The glyph's colour normally and under the pointer. The colour changes
    /// immediately; it is not faded.
    glyph_color: Rgba,
    glyph_hover: Rgba,
    /// The strength of the whole button (CSS `opacity`), normally and under
    /// the pointer.
    opacity: f32,
    opacity_hover: f32,
    /// The strength of the glyph alone, normally and under the pointer.
    glyph_opacity: f32,
    glyph_opacity_hover: f32,
    /// The strength of the whole button while it is disabled.
    disabled_opacity: f32,
    border: Option<Border>,
}

impl Paint {
    /// The background colour for a pointer that is `hover` of the way onto the
    /// button and a press that is `press` of the way down. Each is 0 to 1 and
    /// already follows the timing curve. The colour goes from the normal
    /// background to the hover background, and from that to the pressed
    /// background.
    fn fill_at(&self, hover: f32, press: f32) -> Rgba {
        theme::blend(
            theme::blend(self.fill, self.fill_hover, hover),
            self.fill_press,
            press,
        )
    }

    /// The strength of the whole button for a pointer that is `hover` of the
    /// way onto it, when the button is `dimmed` of the way to disabled.
    fn opacity_at(&self, hover: f32, dimmed: f32) -> f32 {
        let lit = self.opacity + (self.opacity_hover - self.opacity) * hover;
        lit + (self.disabled_opacity - lit) * dimmed
    }
}

/// What a glyph button remembers from one frame to the next.
struct State {
    hover: motion::Fade,
    press: motion::Fade,
    /// On while the button is disabled.
    dimmed: motion::Fade,
}

/// What is drawn inside the button.
enum Content {
    Glyph(&'static str),
    /// `.icBtn.rateButton`: a short label in the monospace caption font. The
    /// button is as wide as the label plus 4px on each side, and at least
    /// 28px.
    Label(SharedString),
    /// A glyph that the caller draws, in the look's glyph colour.
    Element(AnyElement),
}

/// The glyph button that every screen uses, drawn in one of its [`Look`]s. A
/// press on it is not passed on to what is behind it, so a button in the
/// titlebar never drags the window and a button in a picker's header never
/// drags the picker. A disabled button does not react to the pointer and shows
/// no tooltip.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    look: Look,
    content: Content,
    glyph: Option<f32>,
    /// A size for the square button other than the look's size.
    size: Option<f32>,
    radius: Option<f32>,
    disabled: bool,
    toggled: bool,
    badge: Option<usize>,
    tip: Option<Tip>,
    /// The name given to assistive technology, when that is not the tooltip's
    /// text.
    name: Option<SharedString>,
    /// `aria-pressed`, for the buttons that the Electron app marks as toggles.
    pressed: Option<bool>,
    on_click: Option<ClickHandler>,
    on_hover: Option<HoverHandler>,
    on_secondary: Option<PressHandler>,
    /// The part of the window's frame that this button is (minimise, maximise
    /// or close), which the system then handles itself. Used for Windows'
    /// caption buttons.
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
        name: None,
        pressed: None,
        on_click: None,
        on_hover: None,
        on_secondary: None,
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

    /// Sets the glyph's size, for the few buttons whose glyph is not the
    /// look's size.
    pub fn glyph(mut self, size: f32) -> Self {
        self.glyph = Some(size);
        self
    }

    /// Sets the size of the square button, for the few buttons that are not
    /// the look's size.
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    /// Sets the corner radius, for the few buttons that do not have the look's
    /// radius.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    /// Draws the button as switched on, as the look's active CSS class does.
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.toggled = toggled;
        self
    }

    /// Shows a label instead of the glyph.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.content = Content::Label(label.into());
        self
    }

    /// Shows a glyph drawn by the caller instead of an icon.
    pub fn glyph_element(mut self, glyph: impl IntoElement) -> Self {
        self.content = Content::Element(glyph.into_any_element());
        self
    }

    /// Shows a count in the button's top-right corner. A count of zero is not
    /// shown.
    pub fn badge(mut self, count: usize) -> Self {
        self.badge = (count > 0).then_some(count);
        self
    }

    pub fn tip(mut self, tip: Tip) -> Self {
        self.tip = Some(tip);
        self
    }

    /// Sets what assistive technology calls the button, for a button whose
    /// `aria-label` in the Electron app is not its tooltip's text, or that has
    /// no tooltip. Without this, the name is the button's label, or else its
    /// tooltip's text.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Reports the button to assistive technology as a toggle that is on or
    /// off (`aria-pressed`).
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = Some(pressed);
        self
    }

    /// Adds another element inside the button, such as one that records the
    /// button's bounds.
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

    /// Makes the button the system's minimise, maximise or close button. On
    /// Windows the system then does the action itself (and shows its snap
    /// layouts over the maximise button), so there the button takes no click
    /// handler.
    pub fn window_control(mut self, area: WindowControlArea) -> Self {
        self.window_control = Some(area);
        self
    }

    /// Sets the handler for a press of the right mouse button, which opens the
    /// button's context menu.
    pub fn on_secondary(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary = Some(Box::new(handler));
        self
    }

    /// Sets a handler that is called when the pointer enters or leaves the
    /// button. It is called for a disabled button too.
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
        // A button that becomes disabled while the pointer is on it stops
        // being drawn as hovered or pressed, and its tooltip closes.
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

        // The strength of the whole button is applied to its background,
        // border and glyph. It is not applied to the tooltip, which is a child
        // of the button and must stay at full strength.
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

        // The button occludes what is behind it, so a press inside its bounds
        // reaches nothing behind it. The button also stops the press from
        // reaching the window's own listeners (for example a grid's selection
        // rectangle behind a dialog). The panel that holds the button cannot
        // stop that itself, because the occluded panel does not receive the
        // press.
        let name = self.name.clone().or_else(|| match &self.content {
            Content::Label(label) => Some(label.clone()),
            _ => self.tip.as_ref().map(|tip| tip.label().clone()),
        });
        let mut button = div()
            .id(self.id.clone())
            .role(Role::Button)
            .aria_disabled(disabled)
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
        if let Some(name) = name {
            button = button.aria_label(name);
        }
        if let Some(pressed) = self.pressed {
            button = button.aria_toggled(super::a11y::toggled(pressed));
        }
        if let Some(area) = self.window_control {
            button = button.window_control_area(area);
        }
        if let Some(handler) = self.on_secondary
            && !disabled
        {
            button = button.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                cx.stop_propagation();
                handler(event, window, cx);
            });
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
            // A disabled button is not drawn as hovered and shows no
            // tooltip, but it still covers what is behind it. The caller is
            // therefore still told when the pointer enters or leaves it, so
            // that a row which shows its buttons under the pointer keeps
            // showing them while the pointer is on a disabled one.
            if let Some(handler) = self.on_hover {
                button = button.on_hover(move |hovered, window, cx| handler(*hovered, window, cx));
            }
            return button;
        }
        let focus = super::focus::control(&self.id, window, cx);
        // `SmartFolderModal`'s `.conditionButton:focus-visible`. The Electron
        // app's other glyph buttons draw no focus ring.
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
        assert_eq!(ms(Look::UrlAction), 0);
    }

    #[test]
    fn the_icon_look_moves_between_its_overlays() {
        let paint = Look::Icon.paint(&DARK, false);
        assert_eq!(paint.fill_at(0., 0.).a, 0.);
        assert!(same(paint.fill_at(1., 0.), DARK.contrast(8.)));
        assert!(same(paint.fill_at(1., 1.), DARK.contrast(14.)));
        // While toggled, the background is the pressed overlay even with no
        // pointer on the button.
        let toggled = Look::Icon.paint(&DARK, true);
        assert!(same(toggled.fill_at(0., 0.), DARK.contrast(14.)));
        // A disabled button is drawn at 20% strength.
        assert!((paint.opacity_at(0., 1.) - 0.2).abs() < 1e-6);
        assert_eq!(paint.opacity_at(1., 0.), 1.);
    }

    #[test]
    fn the_small_look_is_dimmed_only_when_disabled() {
        let paint = Look::Small.paint(&DARK, false);
        // Enabled: full strength at rest and under the pointer.
        assert_eq!(paint.opacity_at(0., 0.), 1.);
        assert_eq!(paint.opacity_at(1., 0.), 1.);
        // Disabled: 20%.
        assert!((paint.opacity_at(0., 1.) - 0.2).abs() < 1e-6);
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
        // The background halfway to the hover colour is still pure white; only
        // its transparency changes.
        let half = paint.fill_at(0.5, 0.);
        assert_eq!(half, blend(paint.fill, hover, 0.5));
        assert_eq!((half.r, half.g, half.b), (1., 1., 1.));
    }
}
