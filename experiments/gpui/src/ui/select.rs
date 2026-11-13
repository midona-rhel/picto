use crate::theme;
use crate::theme::Text as _;
use gpui::*;

/// The closed custom select (`CmSelect.tsx`): the chosen option's label and
/// a selector glyph at half strength, in a 26px box of the select surface.
#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    label: SharedString,
    width: f32,
}

pub fn select(id: impl Into<ElementId>, label: impl Into<SharedString>, width: f32) -> Select {
    Select {
        id: id.into(),
        label: label.into(),
        width,
    }
}

impl RenderOnce for Select {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        // TODO: open the options below the button and report the choice.
        div()
            .id(self.id)
            .occlude()
            .w(px(self.width))
            .h(px(theme::CONTROL_SM))
            .flex_none()
            .pl(px(12.))
            .pr(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme.select_surface())
            .border_1()
            .border_color(theme.border_secondary())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .line_height(px(24.))
            .text_color(theme.text())
            .whitespace_nowrap()
            .cursor_pointer()
            .child(div().flex_1().min_w_0().truncate().child(self.label))
            .child(
                div()
                    .w(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.5)
                    .child(super::icon("selector-2", 14., theme.text())),
            )
    }
}
