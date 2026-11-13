use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::prelude::FluentBuilder;
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// `EmptyState`: a framed glyph that fades out toward the bottom, a title, a
/// description, and optional buttons, centred in the space it is given.
pub fn empty_state(
    glyph: &'static str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    actions: Vec<AnyElement>,
    cx: &App,
) -> Div {
    let theme = theme::theme(cx);
    // One border colour per element: the sides, the top and the bottom are
    // each their own edge laid over the box, and the box keeps a pixel of
    // padding where its border would be.
    let bevel = |radius: f32, top: Rgba, bottom: Rgba| {
        let edge = || div().absolute().inset_0().rounded(px(radius));
        [
            edge()
                .border_l_1()
                .border_r_1()
                .border_color(theme.border()),
            edge().border_t_1().border_color(top),
            edge().border_b_1().border_color(bottom),
        ]
    };
    let inner = div()
        .relative()
        .flex_1()
        .m(px(6.))
        .p(px(1.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .bg(theme.contrast(6.))
        .children(bevel(3., theme.contrast(15.), rgba(theme::BLACK_10)))
        .child(super::icon(glyph, 28., theme.background()));
    let glass = div()
        .absolute()
        .inset_0()
        .p(px(1.))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(theme::RADIUS_MD))
        .bg(theme.contrast(12.))
        .children(bevel(
            theme::RADIUS_MD,
            theme.contrast(25.),
            rgba(theme::BLACK_20),
        ))
        .child(inner);
    // `mask-image: linear-gradient(black 30%, transparent)`. There are no
    // masks, so the app background is laid over the frame instead, clear at
    // 30% and solid at the bottom.
    let background = theme.background();
    let fade = div().absolute().inset_0().bg(linear_gradient(
        180.,
        linear_color_stop(
            Rgba {
                a: 0.,
                ..background
            },
            0.3,
        ),
        linear_color_stop(background, 1.),
    ));
    // The frame is 120px tall with a -40px bottom margin: it takes 80px of
    // the column and the title overlaps its faded end.
    let frame = div().relative().flex_none().w(px(90.)).h(px(80.)).child(
        div()
            .absolute()
            .top_0()
            .left_0()
            .w(px(90.))
            .h(px(120.))
            .child(glass)
            .child(fade),
    );
    div()
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
                .text_size(px(theme::TEXT_XL))
                .line_height(px(24.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text())
                .child(title.into()),
        )
        .child(
            div()
                .max_w(px(560.))
                .text_size(px(theme::TEXT_SM))
                .line_height(px(18.))
                .text_color(theme.text_secondary())
                .child(description.into()),
        )
        .when(!actions.is_empty(), |root| {
            root.child(
                div()
                    .mt(px(4.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .children(actions),
            )
        })
}

/// `EmptyStateAction`: a 32px bordered button with a glyph and a label whose
/// background deepens over 250 ms on hover.
#[derive(IntoElement)]
pub struct EmptyStateAction {
    id: ElementId,
    glyph: &'static str,
    label: SharedString,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

pub fn empty_state_action(
    id: impl Into<ElementId>,
    glyph: &'static str,
    label: impl Into<SharedString>,
) -> EmptyStateAction {
    EmptyStateAction {
        id: id.into(),
        glyph,
        label: label.into(),
        disabled: false,
        on_click: None,
    }
}

impl EmptyStateAction {
    /// `.action:disabled`: 35% and no pointer.
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

impl RenderOnce for EmptyStateAction {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let hover = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let (value, animating) = {
            let fade = hover.read(cx);
            (fade.value_with(motion::EASE_STANDARD), fade.animating())
        };
        if animating {
            window.request_animation_frame();
        }
        let mut button = div()
            .id(self.id)
            .occlude()
            .h(px(theme::CONTROL_LG))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .px(px(16.))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(theme.border())
            .bg(theme::blend(
                theme.surface_hover(),
                theme.surface_active(),
                value,
            ))
            .text_color(theme.text())
            .text(theme::TEXT_MD)
            .whitespace_nowrap()
            .child(super::icon(self.glyph, 14., theme.text()))
            .child(self.label);
        if self.disabled {
            return button.opacity(0.35);
        }
        button = button.cursor_pointer().on_hover(move |hovered, _, cx| {
            hover.update(cx, |fade, cx| {
                fade.set(*hovered, motion::EMPTY_ACTION_HOVER);
                cx.notify();
            })
        });
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}
