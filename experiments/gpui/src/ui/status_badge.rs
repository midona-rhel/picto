use super::tooltip::{self, Tip};
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

/// `subscriptionStatusPulse`: a syncing badge breathes between 58% and full
/// strength and back, each half on `ease-in-out`.
pub fn pulse(elapsed: Duration) -> f32 {
    0.58 + (1. - 0.58) * motion::pulse(elapsed)
}

/// The small uppercase status label of the subscription screens
/// (`StatusBadge.tsx`): mono 10px text on a 10% tint of its colour inside a
/// 25% border, the text mixed 45% toward the colour. A syncing badge pulses.
#[derive(IntoElement)]
pub struct StatusBadge {
    id: ElementId,
    tone: Tone,
    label: SharedString,
    tip: Option<Tip>,
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
    }
}

impl StatusBadge {
    /// What the badge's `title` explains, shown as a tooltip.
    pub fn tip(mut self, tip: Option<Tip>) -> Self {
        self.tip = tip;
        self
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
            window.request_animation_frame();
            pulse(motion::loop_clock())
        } else {
            1.
        };
        let badge = div()
            .id(self.id.clone())
            .relative()
            .flex_none()
            .h(px(18.))
            .px(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_SM))
            .border_1()
            .border_color(at(25.))
            .bg(at(10.))
            .opacity(opacity)
            // `letter-spacing: 0.04em`
            .child(super::tracked(
                self.label.to_uppercase(),
                font(theme::FONT_MONO),
                theme::TEXT_XS,
                theme::TEXT_XS,
                theme::blend(theme.text(), color, 0.45),
                theme::TEXT_XS * 0.04,
            ));
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
