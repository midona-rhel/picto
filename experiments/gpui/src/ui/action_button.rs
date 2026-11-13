use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Which of `actionButton.module.css`'s buttons.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// `.btnPrimary`: the primary colour, lightening to its 60% on hover.
    Primary,
    /// `.btn`: the hover surface, deepening to the subtle overlay on hover.
    /// The ghost button is styled the same.
    Secondary,
}

/// The shared action button of modals, forms and screen headers: 30px tall,
/// a medium-weight label after an optional glyph, a 1px border and a 1px
/// dark ring, its fill changing over 150 ms on hover. Disabled, it dims to
/// 35% and takes no pointer.
#[derive(IntoElement)]
pub struct ActionButton {
    id: ElementId,
    variant: Variant,
    icon: Option<&'static str>,
    label: SharedString,
    padding: f32,
    gap: f32,
    height: f32,
    full_width: bool,
    disabled: bool,
    tip: Option<Tip>,
    on_click: Option<ClickHandler>,
}

pub fn action_button(
    id: impl Into<ElementId>,
    variant: Variant,
    label: impl Into<SharedString>,
) -> ActionButton {
    ActionButton {
        id: id.into(),
        variant,
        icon: None,
        label: label.into(),
        padding: 20.,
        gap: 8.,
        height: theme::ACTION_BUTTON,
        full_width: false,
        disabled: false,
        tip: None,
        on_click: None,
    }
}

impl ActionButton {
    /// A 14px glyph before the label.
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    /// The space either side of the content, where a screen narrows it.
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }
    /// The space between the glyph and the label.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
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

impl RenderOnce for ActionButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        if self.disabled && fade.read(cx).on() {
            fade.update(cx, |fade, _| fade.set(false, motion::BUTTON_HOVER));
        }
        let hover = fade.read(cx).value();
        if fade.read(cx).animating() || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        let (rest, hovered, border, text) = match self.variant {
            Variant::Primary => (
                theme.primary(),
                theme.primary_60(),
                Some((theme.primary(), theme.primary_60())),
                theme.on_primary(),
            ),
            Variant::Secondary => (
                theme.surface_hover(),
                theme.contrast(10.),
                None,
                theme.text(),
            ),
        };
        let border = match border {
            Some((rest, hovered)) => theme::blend(rest, hovered, hover),
            None => theme.border(),
        };
        let mut button = div()
            .id(self.id)
            .occlude()
            .relative()
            .h(px(self.height))
            .flex_none()
            .px(px(self.padding))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(self.gap))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(border)
            .bg(theme::blend(rest, hovered, hover))
            .shadow(vec![
                BoxShadow::new(px(0.), px(0.), rgba(0x00000033).into()).spread_radius(px(1.)),
            ])
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .font_weight(FontWeight::MEDIUM)
            .text_color(text)
            .whitespace_nowrap();
        if self.full_width {
            button = button.w_full().min_w_0();
        }
        if let Some(icon) = self.icon {
            button = button.child(super::icon(icon, 14., text).flex_none());
        }
        button = button.child(self.label);
        if self.disabled {
            return button.opacity(0.35);
        }
        if let Some(tip) = &self.tip {
            button = button.children(hint.read(cx).layer(tip, self.height, cx));
        }
        let mut button = button.cursor_pointer().on_hover(move |hovered, _, cx| {
            fade.update(cx, |fade, cx| {
                fade.set(*hovered, motion::BUTTON_HOVER);
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
