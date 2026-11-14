use super::tooltip::{self, Tip};
use crate::grid::format;
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type AddHandler = Box<dyn Fn(&Bounds<Pixels>, &mut Window, &mut App) + 'static>;
type PressHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

struct State {
    /// The title pill, 50 ms.
    hover: motion::Fade,
    /// The chevron and the add button appearing, 200 ms.
    reveal: motion::Fade,
}

/// A sidebar section title (`SectionRow` in `SidebarRow.tsx`). Hovering the
/// header lights a pill behind the title, turns the title to the primary text
/// colour, and reveals a chevron and the section's add button. A click folds
/// the section.
#[derive(IntoElement)]
pub struct SectionHeader {
    id: ElementId,
    label: SharedString,
    count: usize,
    expanded: bool,
    on_toggle: Option<ClickHandler>,
    add: Option<(Tip, AddHandler)>,
    on_secondary: Option<PressHandler>,
}

pub fn section_header(id: impl Into<ElementId>, label: impl Into<SharedString>) -> SectionHeader {
    SectionHeader {
        id: id.into(),
        label: label.into(),
        count: 0,
        expanded: true,
        on_toggle: None,
        add: None,
        on_secondary: None,
    }
}

impl SectionHeader {
    /// Shown after the title when it is not zero.
    pub fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }
    pub fn on_toggle(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
    /// The button at the header's right edge that adds to the section. The
    /// handler is given the button's bounds, for a menu that opens below it.
    pub fn on_add(
        mut self,
        tip: Tip,
        handler: impl Fn(&Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.add = Some((tip, Box::new(handler)));
        self
    }
    /// A right-button press: the section's context menu.
    pub fn on_secondary(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            hover: motion::Fade::default(),
            reveal: motion::Fade::default(),
        });
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        // Where the add button was last painted, for its handler.
        let placed = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "add".into()),
            cx,
            |_, _| Bounds::<Pixels>::default(),
        );
        let (hover, hovered, reveal, animating) = {
            let state = state.read(cx);
            (
                state.hover.value(),
                state.hover.on(),
                state.reveal.value(),
                state.hover.animating() || state.reveal.animating(),
            )
        };
        if animating || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        let tertiary = theme.text_tertiary();
        let mut title = div()
            .relative()
            .h(px(24.))
            // `list-anchor + folder-glyph-inset − control-inset + text-nudge`
            .ml(px(theme::SIDEBAR_INSET + 3.5
                - theme::SIDEBAR_SECTION_INSET
                + 1.))
            .px(px(theme::SIDEBAR_SECTION_INSET))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top_0()
                    .bottom(px(1.))
                    .rounded(px(theme::RADIUS_MD))
                    .bg(theme.surface_hover())
                    .opacity(hover),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .text(theme::TEXT_SM)
                    .text_color(tertiary)
                    .child(
                        div()
                            .text_color(if hovered { theme.text() } else { tertiary })
                            .child(self.label),
                    )
                    // The count follows the title with no space: the space
                    // the markup has is a flex item's leading space, which
                    // the browser drops.
                    .children(
                        (self.count > 0)
                            .then(|| format!("({})", format::grouped(self.count as u64))),
                    ),
            );
        if self.on_toggle.is_some() {
            title = title.child(
                svg()
                    .path("chevron-right")
                    .relative()
                    .ml(px(1.))
                    .size(px(11.))
                    .text_color(tertiary)
                    .opacity(0.7 * reveal)
                    .with_transformation(Transformation::rotate(radians(if self.expanded {
                        std::f32::consts::FRAC_PI_2
                    } else {
                        0.
                    }))),
            );
        }
        let header_state = state.clone();
        let mut header = div()
            .id(self.id)
            .relative()
            .mt(px(8.))
            .h(px(24.))
            .flex()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .on_hover(move |hovered, _, cx| {
                header_state.update(cx, |state, cx| {
                    state.hover.set(*hovered, motion::FAST);
                    state.reveal.set(*hovered, motion::NORMAL);
                    cx.notify();
                })
            })
            .child(title);
        if let Some((tip, handler)) = self.add {
            let entered = hint.clone();
            let measure = placed.clone();
            header = header.child(
                div()
                    .id("add")
                    .group("add")
                    .relative()
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme::RADIUS_CONTROL))
                    .hover(move |style| style.bg(theme.surface_hover()))
                    .on_hover(move |hovered, _, cx| {
                        entered.update(cx, |hint, cx| hint.set(*hovered, cx))
                    })
                    .child(
                        svg()
                            .path("plus")
                            .size(px(14.))
                            .text_color(tertiary)
                            .opacity(0.85 * reveal)
                            .group_hover("add", move |style| {
                                style.text_color(theme.text()).opacity(1.)
                            }),
                    )
                    .child(
                        canvas(
                            move |bounds, _, cx| measure.update(cx, |placed, _| *placed = bounds),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .children(hint.read(cx).layer(&tip, 20., cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let bounds = *placed.read(cx);
                        handler(&bounds, window, cx);
                    }),
            );
        }
        if let Some(handler) = self.on_toggle {
            header = header.on_click(handler);
        }
        if let Some(handler) = self.on_secondary {
            header = header.on_mouse_down(MouseButton::Right, handler);
        }
        header
    }
}
