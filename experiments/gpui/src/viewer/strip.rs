//! The previous / count / next strip along the bottom of the media
//! (`QuickLookNavigation.tsx`), used here to step through the members of a
//! collection.
use crate::ui::tooltip::{self, Tip};
use crate::{motion, theme, ui};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// `.button`: 48×32, transparent until hovered, when it takes
/// `--color-surface-hover` over 200 ms (`ease`). Disabled it dims to 30% and
/// takes no pointer.
#[derive(IntoElement)]
pub struct StripButton {
    id: ElementId,
    icon: &'static str,
    disabled: bool,
    tip: Tip,
    on_click: Option<ClickHandler>,
}

pub fn strip_button(id: impl Into<ElementId>, icon: &'static str, tip: Tip) -> StripButton {
    StripButton {
        id: id.into(),
        icon,
        disabled: false,
        tip,
        on_click: None,
    }
}

impl StripButton {
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

impl RenderOnce for StripButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let hover = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        if self.disabled && hover.read(cx).on() {
            hover.update(cx, |hover, _| hover.set(false, motion::STRIP_BUTTON));
            hint.update(cx, |hint, cx| hint.set(false, cx));
        }
        if hover.read(cx).animating() || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        let strength = hover.read(cx).value_with(motion::EASE);
        let button = div()
            .id(self.id)
            .relative()
            .w(px(48.))
            .h(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_MD))
            .bg(theme::blend(
                Rgba {
                    a: 0.,
                    ..theme.surface_hover()
                },
                theme.surface_hover(),
                strength,
            ))
            .child(ui::icon(self.icon, 16., theme.text()));
        if self.disabled {
            return button.opacity(0.3);
        }
        let layer = hint.read(cx).layer(&self.tip, 32., cx);
        let mut button = button
            .occlude()
            .cursor_pointer()
            .on_hover(move |hovered, _, cx| {
                hover.update(cx, |hover, cx| {
                    hover.set(*hovered, motion::STRIP_BUTTON);
                    cx.notify();
                });
                hint.update(cx, |hint, cx| hint.set(*hovered, cx));
            })
            .children(layer);
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}

/// `.counter`: the member's place, in the monospace face with tabular
/// figures, 13px on a 13px line.
pub fn counter(current: usize, total: usize, cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .min_w(px(60.))
        .flex()
        .items_center()
        .justify_center()
        .font_family(theme::FONT_MONO)
        // `font-variant-numeric: tabular-nums`
        .font_features(FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])))
        .text_size(px(theme::TEXT_MD))
        .line_height(px(theme::TEXT_MD))
        .text_color(theme.text())
        .child(format!("{current} / {total}"))
}

/// `.toolbar`: the strip itself, 48px tall along the bottom, centred, 8px
/// between its parts.
pub fn strip() -> Div {
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(48.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
}
