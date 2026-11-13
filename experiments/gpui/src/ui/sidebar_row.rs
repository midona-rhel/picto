use crate::grid::format;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// One sidebar list row (`SidebarRow.tsx`): disclosure, glyph, label and
/// count over a highlight that is solid when the row is active and otherwise
/// fades with the pointer over 50 ms.
#[derive(IntoElement)]
pub struct SidebarRow {
    id: ElementId,
    depth: usize,
    icon: &'static str,
    label: SharedString,
    count: Option<u64>,
    active: bool,
    /// `Some(expanded)` when the row has children.
    disclosure: Option<bool>,
    on_click: Option<ClickHandler>,
    on_toggle: Option<ClickHandler>,
}

pub fn sidebar_row(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: impl Into<SharedString>,
) -> SidebarRow {
    SidebarRow {
        id: id.into(),
        depth: 0,
        icon,
        label: label.into(),
        count: None,
        active: false,
        disclosure: None,
        on_click: None,
        on_toggle: None,
    }
}

impl SidebarRow {
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }
    pub fn count(mut self, count: Option<u64>) -> Self {
        self.count = count;
        self
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn disclosure(mut self, expanded: Option<bool>) -> Self {
        self.disclosure = expanded;
        self
    }
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
    pub fn on_toggle(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SidebarRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let (hover, hovered) = {
            let fade = fade.read(cx);
            (fade.value(), fade.on())
        };
        if fade.read(cx).animating() {
            window.request_animation_frame();
        }
        let indent = self.depth as f32 * theme::SIDEBAR_INDENT;
        let mut disclosure = div()
            .id("disclosure")
            .absolute()
            .left(px(2. + indent))
            .w(px(theme::SIDEBAR_DISCLOSURE))
            .h(px(theme::SIDEBAR_ROW))
            .flex_none()
            .flex()
            .items_center()
            .justify_center();
        if let Some(expanded) = self.disclosure {
            disclosure = disclosure
                .rounded(px(theme::RADIUS_SM))
                .hover(move |style| style.bg(theme.hover_overlay()))
                .child(super::icon(
                    if expanded {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    16.,
                    theme.text(),
                ));
            if let Some(handler) = self.on_toggle {
                disclosure = disclosure.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    handler(event, window, cx);
                });
            }
        }
        let mut row = div()
            .id(self.id)
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .h(px(theme::SIDEBAR_ROW))
            .pl(px(theme::SIDEBAR_INSET + indent))
            .pr(px(2.))
            .on_hover(move |hovered, _, cx| {
                fade.update(cx, |fade, cx| {
                    fade.set(*hovered, motion::SIDEBAR_HOVER);
                    cx.notify();
                })
            })
            .child(
                div()
                    .absolute()
                    .left(px(theme::SIDEBAR_INSET + indent))
                    .right(px(2.))
                    .top_0()
                    .bottom(px(1.))
                    .rounded(px(theme::RADIUS_MD))
                    .bg(if self.active {
                        theme.surface_active()
                    } else {
                        theme.surface_hover()
                    })
                    .opacity(if self.active { 1. } else { hover }),
            )
            .child(disclosure)
            .child(
                div()
                    .w(px(theme::SIDEBAR_GLYPH_COLUMN))
                    .h(px(theme::SIDEBAR_ROW))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        super::icon(self.icon, theme::SIDEBAR_ICON, theme.text())
                            .opacity(if self.active || hovered { 1. } else { 0.8 }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(self.label),
            );
        if let Some(count) = self.count {
            row = row.child(
                div()
                    .px(px(5.))
                    .min_w(px(36.))
                    .text_right()
                    .text_size(px(theme::TEXT_CAPTION))
                    .font_family(theme::FONT_MONO)
                    .text_color(if self.active {
                        theme.text()
                    } else {
                        theme.text_tertiary()
                    })
                    .child(format::grouped(count)),
            );
        }
        if let Some(handler) = self.on_click {
            row = row.on_click(handler);
        }
        row
    }
}
