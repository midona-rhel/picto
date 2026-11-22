use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::time::Duration;

/// A status badge's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Running,
    Paused,
    Attention,
    Success,
    Idle,
}

/// Returns the opacity of a badge with the tone `Running`, `elapsed` after the
/// pulse began. The badge goes from 58% to full strength and back again, and
/// repeats this.
pub fn pulse(elapsed: Duration) -> f32 {
    0.58 + (1. - 0.58) * motion::pulse(elapsed)
}

/// The small status label of the subscription screens. Its text is
/// caption-sized and starts with a capital letter. The text's colour is 45% of
/// the way from the text colour to the badge's colour. The background is the
/// badge's colour at 10%, and the border is that colour at 25%. A badge with
/// the tone `Running` pulses.
#[derive(IntoElement)]
pub struct StatusBadge {
    id: ElementId,
    tone: Tone,
    label: SharedString,
    tip: Option<Tip>,
    large: bool,
}

pub fn status_badge(
    id: impl Into<ElementId>,
    tone: Tone,
    label: impl Into<SharedString>,
) -> StatusBadge {
    StatusBadge {
        id: id.into(),
        tone,
        label: label.into(),
        tip: None,
        large: false,
    }
}

impl StatusBadge {
    /// Sets the tooltip that explains the badge.
    pub fn tip(mut self, tip: Option<Tip>) -> Self {
        self.tip = tip;
        self
    }

    /// Makes this the larger badge that the Accounts dialog uses for an
    /// account's state: as high as a small control, in the small text size,
    /// and at most 112px wide.
    pub fn large(mut self) -> Self {
        self.large = true;
        self
    }
}

/// Returns `label` with its first letter as a capital and the rest unchanged,
/// which is how the badge writes it. A state that the library names in lower
/// case is then shown with a capital first letter.
pub fn sentence(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

impl RenderOnce for StatusBadge {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let color = match self.tone {
            Tone::Running => theme.primary(),
            Tone::Paused => rgb(theme::WARNING),
            Tone::Attention => rgb(theme::DANGER),
            Tone::Success => rgb(theme::SUCCESS),
            Tone::Idle => theme.text_tertiary(),
        };
        let at = |percent: f32| Rgba {
            a: color.a * percent / 100.,
            ..color
        };
        let opacity = if self.tone == Tone::Running {
            motion::loop_frame(window, cx);
            pulse(motion::loop_clock())
        } else {
            1.
        };
        let text = theme::blend(theme.text(), color, 0.45);
        let badge = div()
            .id(self.id.clone())
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .border_1()
            .border_color(at(25.))
            .bg(at(10.))
            .opacity(opacity);
        let badge = if self.large {
            badge
                .max_w(px(112.))
                .h(px(theme::CONTROL_SM))
                .px(px(8.))
                .overflow_hidden()
                .rounded(px(theme::RADIUS_MD))
                .child(
                    div()
                        .text(theme::TEXT_SM)
                        .line_height(px(theme::TEXT_SM))
                        .text_color(text)
                        .truncate()
                        .child(sentence(&self.label)),
                )
        } else {
            badge
                .h(px(18.))
                .px(px(5.))
                .rounded(px(theme::RADIUS_SM))
                .child(
                    div()
                        .text(theme::TEXT_CAPTION)
                        .line_height(px(theme::TEXT_CAPTION))
                        .text_color(text)
                        .child(sentence(&self.label)),
                )
        };
        match self.tip {
            Some(tip) => tooltip::attach(
                badge,
                ElementId::NamedChild(std::sync::Arc::new(self.id), "tip".into()),
                tip,
                18.,
                window,
                cx,
            )
            .into_any_element(),
            None => badge.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pulse;
    use std::time::Duration;

    #[test]
    fn a_syncing_badge_breathes_from_dim_to_full_and_back() {
        assert!((pulse(Duration::ZERO) - 0.58).abs() < 1e-4);
        assert!((pulse(Duration::from_millis(1200)) - 1.).abs() < 1e-4);
        assert!((pulse(Duration::from_millis(2400)) - 0.58).abs() < 1e-4);
        let rising = pulse(Duration::from_millis(600));
        let falling = pulse(Duration::from_millis(1800));
        assert!((rising - falling).abs() < 1e-3 && rising > 0.58 && rising < 1.);
    }
}
