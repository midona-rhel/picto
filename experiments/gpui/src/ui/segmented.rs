//! Small segmented glyph controls on the control surface: the filter
//! pickers' match rule (`FilterLogicTabs`), the view menu's sort direction
//! (`.dirPill`) and the pickers' view tabs (`OverlayShell` `.viewTabs`).
use super::tooltip::{self, Tip};
use crate::theme;
use gpui::*;
use std::rc::Rc;

type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>;

/// Which of the three controls to draw.
#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// `FilterLogicTabs.module.css`: 24px buttons 2px apart, the hovered one
    /// on the subtle overlay, the chosen one on the active surface.
    Logic,
    /// `.dirPill`: 26×24 buttons 1px apart at 40% until chosen, the chosen
    /// one at full strength on `--color-contrast-12`.
    Direction,
    /// `.viewTabs`: 28×24 buttons side by side, the chosen one on the
    /// active surface.
    View,
}

impl Look {
    fn button(self) -> (f32, f32) {
        match self {
            Look::Logic => (24., 24.),
            Look::Direction => (26., 24.),
            Look::View => (28., 24.),
        }
    }
    fn gap(self) -> f32 {
        match self {
            Look::Logic => 2.,
            Look::Direction => 1.,
            Look::View => 0.,
        }
    }
}

/// One choice: its 14px glyph and what it is called, which its tooltip
/// says where the control has tooltips.
pub struct Segment {
    pub icon: &'static str,
    pub label: SharedString,
}

pub fn segment(icon: &'static str, label: impl Into<SharedString>) -> Segment {
    Segment {
        icon,
        label: label.into(),
    }
}

#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    look: Look,
    segments: Vec<Segment>,
    selected: Option<usize>,
    tips: bool,
    on_select: Option<SelectHandler>,
}

pub fn segmented(
    id: impl Into<ElementId>,
    look: Look,
    segments: Vec<Segment>,
    selected: Option<usize>,
) -> Segmented {
    Segmented {
        id: id.into(),
        look,
        segments,
        selected,
        // `FilterLogicTabs` and the view tabs wrap each button in a
        // `KbdTooltip`; the direction pill has only accessible labels.
        tips: look != Look::Direction,
        on_select: None,
    }
}

impl Segmented {
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (width, height) = self.look.button();
        let mut root = div()
            .id(self.id.clone())
            .occlude()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(self.look.gap()))
            .p(px(2.))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme.control_surface());
        for (index, segment) in self.segments.into_iter().enumerate() {
            let chosen = self.selected == Some(index);
            let mut button = div()
                .id(ElementId::NamedChild(
                    std::sync::Arc::new(self.id.clone()),
                    format!("segment-{index}").into(),
                ))
                .w(px(width))
                .h(px(height))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer();
            let glyph = super::icon(segment.icon, 14., theme.text());
            button = match self.look {
                Look::Logic => {
                    let button = if chosen {
                        button.bg(theme.surface_active())
                    } else {
                        button.hover(move |style| style.bg(theme.subtle_overlay()))
                    };
                    button.child(glyph)
                }
                Look::Direction => {
                    let button = if chosen {
                        button.bg(theme.contrast(12.))
                    } else {
                        button
                    };
                    button.child(glyph.opacity(if chosen { 1. } else { 0.4 }))
                }
                Look::View => {
                    let button = if chosen {
                        button.bg(theme.surface_active())
                    } else {
                        button
                    };
                    button.child(glyph)
                }
            };
            if let Some(handler) = self.on_select.clone() {
                button = button.on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    handler(index, window, cx)
                });
            }
            if self.tips {
                button = tooltip::attach(
                    button,
                    ElementId::NamedChild(
                        std::sync::Arc::new(self.id.clone()),
                        format!("segment-tip-{index}").into(),
                    ),
                    Tip::new(segment.label),
                    height,
                    window,
                    cx,
                );
            }
            root = root.child(button);
        }
        root
    }
}
