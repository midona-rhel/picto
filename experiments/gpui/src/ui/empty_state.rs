use crate::theme;
use gpui::*;

/// A screen with nothing to show (`EmptyState.tsx`): a small glass frame
/// holding a glyph, fading out toward its foot, then a title, a description,
/// the actions and a 2px progress slot, centred as one column.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: (&'static str, f32),
    title: SharedString,
    description: SharedString,
    actions: Vec<AnyElement>,
}

pub fn empty_state(
    icon: &'static str,
    size: f32,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
) -> EmptyState {
    EmptyState {
        icon: (icon, size),
        title: title.into(),
        description: description.into(),
        actions: Vec::new(),
    }
}

impl EmptyState {
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let background = theme.background();
        // One border has one colour, so the lighter top and darker bottom
        // edges are drawn as 1px lines over the ring.
        let edges = |radius: f32, top: Rgba, bottom: Rgba| {
            div()
                .absolute()
                .inset_0()
                .rounded(px(radius))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(radius))
                        .right(px(radius))
                        .h(px(1.))
                        .bg(top),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left(px(radius))
                        .right(px(radius))
                        .h(px(1.))
                        .bg(bottom),
                )
        };
        let frame = div()
            .relative()
            .w(px(90.))
            .h(px(120.))
            .flex_none()
            // `margin-bottom: -40px`: the title rides up over the faded foot.
            .mb(px(-40.))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.border())
                    .bg(theme.contrast(12.))
                    .child(edges(
                        theme::RADIUS_MD,
                        theme.contrast(25.),
                        theme.black(20.),
                    ))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .m(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(theme.border())
                            .bg(theme.contrast(6.))
                            .child(edges(3., theme.contrast(15.), theme.black(10.)))
                            .child(super::icon(self.icon.0, self.icon.1, background)),
                    ),
            )
            // `mask-image: linear-gradient(to bottom, black 30%, transparent)`:
            // GPUI has no masks, so the app background is laid over instead.
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(
                    Rgba {
                        a: 0.,
                        ..background
                    },
                    0.3,
                ),
                linear_color_stop(background, 1.),
            )));
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .p(px(40.))
            .text_center()
            .child(frame)
            .child(
                div()
                    .relative()
                    .text_color(theme.text())
                    .text_size(px(theme::TEXT_XL))
                    .line_height(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title),
            )
            .child(
                div()
                    .max_w(px(560.))
                    .text_color(theme.text_secondary())
                    .text_size(px(theme::TEXT_SM))
                    .line_height(px(18.))
                    .child(self.description),
            );
        if !self.actions.is_empty() {
            root = root.child(
                div()
                    .mt(px(4.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .children(self.actions),
            );
        }
        // The 180 × 2 progress slot keeps its place while empty.
        // TODO: the indeterminate `ProgressBar` a screen shows here while it
        // works, once a screen here has work to show.
        root.child(div().w(px(180.)).h(px(2.)).flex_none())
    }
}
