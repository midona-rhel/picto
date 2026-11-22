use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type HoverHandler = Box<dyn Fn(bool, &mut Window, &mut App) + 'static>;

/// Says which style a shared text button has.
#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// The usual style. The fill is the contrast colour at 5% opacity
    /// (`Theme::surface_hover`), the border is the border colour
    /// (`Theme::border`), and a 1px ring of 20% black is drawn around the
    /// border. While the pointer is over the button, the fill changes to the
    /// contrast colour at 10% opacity over 150 ms.
    Standard,
    /// The style of a button for the main action. It has the size and the
    /// 1px ring of `Standard`. The fill and the border are the primary
    /// colour. While the pointer is over the button, both change to the
    /// primary colour at 60% opacity.
    Primary,
    /// The style of a button for a destructive action. It has the size and
    /// the 1px ring of `Standard`. The fill is the negative colour at 8%
    /// opacity, the border is the negative colour at 35% opacity, and the
    /// label is pale red (`theme::DANGER_TEXT`). While the pointer is over
    /// the button, the fill changes to the negative colour at 15% opacity.
    Danger,
    /// The style of the buttons in the Settings window. The button is 36px
    /// high, its label has the regular font weight, and it has no 1px ring
    /// around its border. While the pointer is over the button, the fill
    /// changes from the contrast colour at 5% opacity to the contrast colour
    /// at 10% opacity over 150 ms, on the curve `motion::EASE`.
    Settings,
    /// The style of a button for the main action in the Settings window. It
    /// has the size of `Settings`. The fill and the border are the primary
    /// colour and the label is semibold. While the pointer is over the
    /// button, the fill changes to a mix of 95% primary colour and 5% of the
    /// colour that text has on the primary colour (`Theme::on_primary`).
    SettingsPrimary,
    /// A style without a visible frame, which a button at the foot of the
    /// task dialog of an import uses. It is `Standard` with a transparent
    /// fill, a transparent border, no 1px ring, and a label in the secondary
    /// text colour. While the pointer is over the button, the fill changes
    /// to the contrast colour at 5% opacity and the label changes to the
    /// primary text colour.
    Ghost,
}

impl Look {
    /// Returns true for the two styles of the Settings window (`Settings`
    /// and `SettingsPrimary`), and false for `Standard` and the styles that
    /// are based on it.
    fn settings(self) -> bool {
        matches!(self, Look::Settings | Look::SettingsPrimary)
    }
}

/// The shared action button. It is a button with a text label and an
/// optional icon before the label, drawn in one of the styles in [`Look`]. A
/// disabled button is drawn at 35% opacity and does not react to the
/// pointer: its colour does not change while the pointer is over it, it
/// cannot be clicked, and it shows no tooltip. The button calls `occlude`,
/// so the pointer does not reach the elements behind it.
#[derive(IntoElement)]
pub struct ActionButton {
    id: ElementId,
    look: Look,
    icon: Option<(&'static str, f32)>,
    label: Option<SharedString>,
    /// A second, smaller text in the tertiary text colour, drawn after the
    /// label, at the end of the button. When it is set, the label is
    /// left-aligned and fills the rest of the width. The buttons that choose
    /// a cloud provider in the library manager use it.
    caption: Option<SharedString>,
    height: f32,
    width: Option<f32>,
    full_width: bool,
    padding: f32,
    gap: f32,
    text_size: f32,
    weight: FontWeight,
    disabled: bool,
    tip: Option<Tip>,
    on_click: Option<ClickHandler>,
    on_hover: Option<HoverHandler>,
    on_focus: Option<HoverHandler>,
}

pub fn action_button(id: impl Into<ElementId>, look: Look) -> ActionButton {
    let settings = look.settings();
    ActionButton {
        id: id.into(),
        look,
        icon: None,
        label: None,
        caption: None,
        height: if settings { 36. } else { theme::ACTION_BUTTON },
        width: None,
        full_width: false,
        padding: if settings { 16. } else { 20. },
        gap: if settings { 6. } else { 8. },
        text_size: theme::TEXT_MD,
        weight: match look {
            Look::Standard | Look::Primary | Look::Danger | Look::Ghost => FontWeight::MEDIUM,
            Look::Settings => FontWeight::NORMAL,
            Look::SettingsPrimary => FontWeight::SEMIBOLD,
        },
        disabled: false,
        tip: None,
        on_click: None,
        on_hover: None,
        on_focus: None,
    }
}

impl ActionButton {
    pub fn icon(mut self, icon: &'static str, size: f32) -> Self {
        self.icon = Some((icon, size));
        self
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// Sets a small text in the tertiary text colour that is drawn after the
    /// label, at the end of the button. With a caption, the label is
    /// left-aligned and, when it does not fit, is cut short with an
    /// ellipsis.
    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    /// Makes the button as wide as the space that its parent gives it.
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
    /// Sets a fixed width in pixels. This is for a button that shows only an
    /// icon and no label.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    /// Sets the padding in pixels to the left and to the right of the
    /// button's content.
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
    pub fn text_size(mut self, size: f32) -> Self {
        self.text_size = size;
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
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
    /// Sets a handler that is called with `true` when the pointer moves onto
    /// the button and with `false` when the pointer leaves it.
    pub fn on_hover(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }
    /// Sets a handler that is called with `true` when the button gets
    /// keyboard focus and with `false` when it loses keyboard focus. It is
    /// also called with `false` when the button becomes disabled while it
    /// has keyboard focus.
    /// The button only finds out that its focus has changed while it is
    /// being drawn, so the handler runs during drawing. A handler that
    /// changes what a view shows must therefore also ask for another frame
    /// to be drawn.
    pub fn on_focus(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_focus = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ActionButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        // A button that becomes disabled while the pointer is over it stops
        // showing its hover colour immediately, and its tooltip is told that
        // the pointer has left.
        if self.disabled && fade.read(cx).on() {
            fade.update(cx, |fade, _| *fade = motion::Fade::default());
            hint.update(cx, |hint, cx| hint.set(false, cx));
        }
        let (hover, animating) = {
            let fade = fade.read(cx);
            // The change of colour under the pointer follows the curve
            // `motion::EASE` in the two Settings styles and the curve
            // `motion::EASE_DEFAULT` in the other styles.
            let curve = if self.look.settings() {
                motion::EASE
            } else {
                motion::EASE_DEFAULT
            };
            (fade.value_with(curve), fade.animating())
        };
        if animating || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        let (background, border, text) = match self.look {
            Look::Standard => (
                theme::blend(theme.surface_hover(), theme.contrast(10.), hover),
                theme.border(),
                theme.text(),
            ),
            Look::Primary => {
                let color = theme::blend(theme.primary(), theme.primary_at(60.), hover);
                (color, color, theme.on_primary())
            }
            Look::Danger => (
                theme::blend(theme.negative_at(8.), theme.negative_at(15.), hover),
                theme.negative_at(35.),
                rgb(theme::DANGER_TEXT),
            ),
            Look::Settings => (
                theme::blend(theme.surface_hover(), theme.surface_active(), hover),
                theme.border(),
                theme.text(),
            ),
            Look::Ghost => (
                theme::blend(
                    theme::clear(theme.surface_hover()),
                    theme.surface_hover(),
                    hover,
                ),
                theme::clear(theme.border()),
                theme::blend(theme.text_secondary(), theme.text(), hover),
            ),
            // While the pointer is over this button, its fill is a mix of 95%
            // primary colour and 5% of the colour of text on the primary
            // colour.
            Look::SettingsPrimary => (
                theme::blend(
                    theme.primary(),
                    theme::blend(theme.primary(), theme.on_primary(), 0.05),
                    hover,
                ),
                theme.primary(),
                theme.on_primary(),
            ),
        };
        // The button's accessible name is its label. A button that shows
        // only an icon has no label, so its name is its tooltip's text.
        let name = self
            .label
            .clone()
            .or_else(|| self.tip.as_ref().map(|tip| tip.label().clone()));
        let mut button = div()
            .id(self.id.clone())
            .role(Role::Button)
            .aria_disabled(self.disabled)
            .occlude()
            .relative()
            .h(px(self.height))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(self.gap))
            .px(px(self.padding))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(border)
            .bg(background)
            .text_color(text)
            .font_family(theme::FONT_UI)
            .text(self.text_size)
            .font_weight(self.weight)
            .whitespace_nowrap();
        if let Some(width) = self.width {
            button = button.w(px(width));
        } else if self.full_width {
            button = button.w_full().min_w_0();
        }
        // The 1px ring of 20% black around the border. The two Settings
        // styles and the Ghost style do not have it.
        if !self.look.settings() && self.look != Look::Ghost {
            button = button.shadow(vec![
                BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_20).into()).spread_radius(px(1.)),
            ]);
        }
        if let Some((icon, size)) = self.icon {
            button = button.child(super::icon(icon, size, text).flex_none());
        }
        if let Some(name) = name {
            button = button.aria_label(name);
        }
        match (self.label, self.caption) {
            (Some(label), Some(caption)) => {
                button = button.justify_start().child(
                    div().flex_1().min_w_0().truncate().child(label),
                );
                button = button.child(
                    div()
                        .flex_none()
                        .max_w(relative(0.5))
                        .truncate()
                        .text(theme::TEXT_CAPTION)
                        .text_color(theme.text_tertiary())
                        .child(caption),
                );
            }
            (Some(label), None) => button = button.child(label),
            (None, _) => {}
        }
        let focus = super::focus::control(&self.id, window, cx);
        if let Some(handler) = &self.on_focus {
            let focused = !self.disabled && focus.focused(window);
            let told = window.use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "focused".into()),
                cx,
                |_, _| false,
            );
            if *told.read(cx) != focused {
                told.update(cx, |told, _| *told = focused);
                handler(focused, window, cx);
            }
        }
        if self.disabled {
            // GPUI applies the opacity to the border and to the fill
            // separately and draws the border on top of the fill. A border
            // that has the same colour as the fill would therefore get that
            // colour twice and look stronger than the rest of the button,
            // so it is made transparent here.
            if border == background {
                button = button.border_color(theme::clear(border));
            }
            return button.opacity(0.35);
        }
        if let Some(tip) = &self.tip {
            button = button.children(hint.read(cx).layer(tip, self.height, cx));
        }
        // The focus ring is drawn while the button has the keyboard focus.
        // The two Settings styles draw no focus ring.
        if !self.look.settings() {
            button = button.children(focus.ring(
                super::focus::Ring::button(&theme),
                theme::RADIUS_MD,
                1.,
            ));
        }
        let duration = motion::BUTTON_HOVER;
        let on_hover = self.on_hover;
        button = button
            .cursor_pointer()
            .on_hover(move |hovered, window, cx| {
                fade.update(cx, |fade, cx| {
                    fade.set(*hovered, duration);
                    cx.notify();
                });
                hint.update(cx, |hint, cx| hint.set(*hovered, cx));
                if let Some(handler) = &on_hover {
                    handler(*hovered, window, cx);
                }
            });
        if let Some(handler) = self.on_click {
            let handler: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> = Rc::from(handler);
            let pressed = handler.clone();
            button = button.on_click(move |event, window, cx| handler(event, window, cx));
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
