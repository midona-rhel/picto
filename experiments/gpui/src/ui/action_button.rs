use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Which of `actionButton.module.css`'s buttons to draw.
#[derive(Clone, Copy, PartialEq)]
pub enum Variant {
    /// `.btn`
    Secondary,
    /// `.btn.btnPrimary`
    Primary,
    /// `.btn.btnDanger`
    Danger,
}

/// The labelled push button of dialogs and forms (`actionButton.module.css`):
/// 30px, or 26px when compact, with an optional 14px glyph before the label.
/// Its fill changes under the pointer over 150 ms; disabled, it dims to 35%
/// and takes no clicks.
#[derive(IntoElement)]
pub struct ActionButton {
    id: ElementId,
    label: SharedString,
    icon: Option<&'static str>,
    variant: Variant,
    compact: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

pub fn action_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> ActionButton {
    ActionButton {
        id: id.into(),
        label: label.into(),
        icon: None,
        variant: Variant::Secondary,
        compact: false,
        disabled: false,
        on_click: None,
    }
}

impl ActionButton {
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }
    /// `.buttonCompact`: 26px tall with 16px sides.
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }
    /// A glyph before the label.
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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
        let hover = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        if self.disabled && hover.read(cx).on() {
            hover.update(cx, |fade, _| fade.set(false, motion::BUTTON_HOVER));
        }
        let strength = hover.read(cx).value();
        if hover.read(cx).animating() {
            window.request_animation_frame();
        }
        let (rest, lit, border, text) = match self.variant {
            Variant::Secondary => (
                theme.surface_hover(),
                theme.subtle_overlay(),
                (theme.border(), theme.border()),
                theme.text(),
            ),
            Variant::Primary => (
                theme.primary(),
                theme.primary_at(60.),
                (theme.primary(), theme.primary_at(60.)),
                rgb(0xffffff),
            ),
            Variant::Danger => (
                theme.negative_at(8.),
                theme.negative_at(15.),
                (theme.negative_at(35.), theme.negative_at(35.)),
                rgb(theme::DANGER_TEXT),
            ),
        };
        let mut button = div()
            .id(self.id)
            .occlude()
            .h(px(if self.compact {
                theme::CONTROL_SM
            } else {
                theme::ACTION_BUTTON
            }))
            .px(px(if self.compact { 16. } else { 20. }))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(theme::blend(border.0, border.1, strength))
            .bg(theme::blend(rest, lit, strength))
            // `box-shadow: 0 0 0 1px var(--color-black-20)`
            .shadow(vec![
                BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_20).into()).spread_radius(px(1.)),
            ])
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .font_weight(FontWeight::MEDIUM)
            .text_color(text)
            .whitespace_nowrap()
            .children(self.icon.map(|icon| super::icon(icon, 14., text)))
            .child(self.label);
        if self.disabled {
            return button.opacity(0.35);
        }
        button = button.cursor_pointer().on_hover(move |hovered, _, cx| {
            hover.update(cx, |fade, cx| {
                fade.set(*hovered, motion::BUTTON_HOVER);
                cx.notify();
            })
        });
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}
