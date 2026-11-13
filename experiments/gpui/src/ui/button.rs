use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Which of the app's bordered text buttons to draw.
#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// `actionButton.module.css` `.btn`: modals, forms, the AI model rows.
    Action,
    /// `Settings.module.css` `.footerBtn` and `.inlineButton`.
    Settings,
    /// `Settings.module.css` `.footerBtnPrimary`.
    SettingsPrimary,
}

struct Style {
    height: f32,
    padding: f32,
    gap: f32,
    weight: FontWeight,
    text: Rgba,
    border: (Rgba, Rgba),
    fill: (Rgba, Rgba),
    /// `box-shadow: 0 0 0 1px var(--color-black-20)`.
    ring: bool,
    curve: motion::CubicBezier,
}

impl Look {
    fn style(self, theme: theme::Theme) -> Style {
        let action = Style {
            height: theme::ACTION_BUTTON,
            padding: 20.,
            gap: 8.,
            weight: FontWeight::MEDIUM,
            text: theme.text(),
            border: (theme.border(), theme.border()),
            // `--color-surface-hover`, then `--color-subtle-overlay`.
            fill: (theme.surface_hover(), theme.contrast(10.)),
            ring: true,
            curve: motion::EASE_DEFAULT,
        };
        let settings = Style {
            height: 36.,
            padding: 16.,
            gap: 6.,
            weight: FontWeight::NORMAL,
            fill: (theme.surface_hover(), theme.surface_active()),
            ring: false,
            curve: motion::EASE,
            ..action
        };
        match self {
            Look::Action => action,
            Look::Settings => settings,
            Look::SettingsPrimary => Style {
                weight: FontWeight::SEMIBOLD,
                text: rgb(0xffffff),
                border: (theme.primary(), theme.primary()),
                // `color-mix(in srgb, var(--color-primary) 95%, #fff)`
                fill: (
                    theme.primary(),
                    theme::blend(theme.primary(), rgb(0xffffff), 0.05),
                ),
                ..settings
            },
        }
    }
}

/// A bordered text button. Its fill (and, for the primary action, its
/// border) changes over 150 ms under the pointer; disabled, it dims to 35%
/// and takes no input. It occludes what is behind it, so it works inside a
/// titlebar strip.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    look: Look,
    width: Option<f32>,
    disabled: bool,
    tip: Option<Tip>,
    on_click: Option<ClickHandler>,
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, _: &App) -> Button {
    Button {
        id: id.into(),
        label: label.into(),
        look: Look::Action,
        width: None,
        disabled: false,
        tip: None,
        on_click: None,
    }
}

impl Button {
    pub fn look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }
    /// A fixed width, for buttons that line up in a column.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
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
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let style = self.look.style(theme);
        let hover = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        if self.disabled && hover.read(cx).on() {
            hover.update(cx, |hover, _| hover.set(false, motion::BUTTON_HOVER));
        }
        let strength = hover.read(cx).value_with(style.curve);
        if hover.read(cx).animating() || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        let mut button = div()
            .id(self.id)
            .occlude()
            .relative()
            .h(px(style.height))
            .flex_none()
            .px(px(style.padding))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(style.gap))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(theme::blend(style.border.0, style.border.1, strength))
            .bg(theme::blend(style.fill.0, style.fill.1, strength))
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .font_weight(style.weight)
            .text_color(style.text)
            .whitespace_nowrap()
            .child(self.label);
        if style.ring {
            button = button.shadow(vec![
                BoxShadow::new(px(0.), px(0.), rgba(0x00000033).into()).spread_radius(px(1.)),
            ]);
        }
        if let Some(width) = self.width {
            button = button.w(px(width)).px_0();
        }
        if self.disabled {
            return button.opacity(0.35);
        }
        if let Some(tip) = &self.tip {
            button = button.children(hint.read(cx).layer(tip, style.height, cx));
        }
        let mut button = button.cursor_pointer().on_hover(move |hovered, _, cx| {
            hover.update(cx, |hover, cx| {
                hover.set(*hovered, motion::BUTTON_HOVER);
                cx.notify();
            });
            hint.update(cx, |hint, cx| hint.set(*hovered, cx));
        });
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}
