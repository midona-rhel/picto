use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type HoverHandler = Box<dyn Fn(bool, &mut Window, &mut App) + 'static>;

/// Which of the shared text buttons this is.
#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// `actionButton.module.css` `.btn`: the hover surface in a primary
    /// border with a dark 1px ring, deepening to the subtle overlay under
    /// the pointer over 150 ms.
    Standard,
    /// `.btn.btnPrimary`: the primary colour, at 60% under the pointer.
    Primary,
}

/// The shared action button (`actionButton.module.css`): a text button with
/// an optional leading glyph, in one of its looks.
/// Disabled, it fades to 35% and takes no pointer, so it has no hover and no
/// tooltip. It occludes what is behind it.
#[derive(IntoElement)]
pub struct ActionButton {
    id: ElementId,
    look: Look,
    icon: Option<(&'static str, f32)>,
    label: Option<SharedString>,
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
}

pub fn action_button(id: impl Into<ElementId>, look: Look) -> ActionButton {
    ActionButton {
        id: id.into(),
        look,
        icon: None,
        label: None,
        height: theme::ACTION_BUTTON,
        width: None,
        full_width: false,
        padding: 20.,
        gap: 8.,
        text_size: theme::TEXT_MD,
        weight: FontWeight::MEDIUM,
        disabled: false,
        tip: None,
        on_click: None,
        on_hover: None,
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
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    /// `.buttonCompact`: the small control height with 16px padding.
    pub fn compact(mut self) -> Self {
        self.height = theme::CONTROL_SM;
        self.padding = 16.;
        self
    }
    /// Fills the width it is given, as a button in a grid track does.
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
    /// A fixed width, for a button that shows only its glyph.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    /// The space on either side of the content.
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
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
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
    /// Told when the pointer enters or leaves the button.
    pub fn on_hover(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
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
        // A button disabled under the pointer loses its hover at once.
        if self.disabled && fade.read(cx).on() {
            fade.update(cx, |fade, _| *fade = motion::Fade::default());
            hint.update(cx, |hint, cx| hint.set(false, cx));
        }
        let (hover, animating) = {
            let fade = fade.read(cx);
            (fade.value(), fade.animating())
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
        };
        let mut button = div()
            .id(self.id)
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
        // `box-shadow: 0 0 0 1px var(--color-black-20)`
        button = button.shadow(vec![
            BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_20).into()).spread_radius(px(1.)),
        ]);
        if let Some((icon, size)) = self.icon {
            button = button.child(super::icon(icon, size, text).flex_none());
        }
        if let Some(label) = self.label {
            button = button.child(label);
        }
        if self.disabled {
            return button.opacity(0.35);
        }
        if let Some(tip) = &self.tip {
            button = button.children(hint.read(cx).layer(tip, self.height, cx));
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
            button = button.on_click(handler);
        }
        button
    }
}
