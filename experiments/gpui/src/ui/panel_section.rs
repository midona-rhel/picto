use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type MenuHandler = Box<dyn Fn(&Point<Pixels>, &mut Window, &mut App) + 'static>;

struct State {
    /// On while the section is open: the chevron's quarter turn and fade
    /// out, 200 ms.
    open: motion::Fade,
    section: motion::Fade,
    header: motion::Fade,
}

/// A collapsible inspector section (`InspectorSection.tsx`): a rule, a bold
/// tertiary title that turns primary under the pointer, and a chevron that
/// shows only while the section is closed.
#[derive(IntoElement)]
pub struct PanelSection {
    id: ElementId,
    title: SharedString,
    expanded: bool,
    on_toggle: Option<ClickHandler>,
    on_menu: Option<MenuHandler>,
    content: Option<AnyElement>,
}

pub fn panel_section(id: impl Into<ElementId>, title: impl Into<SharedString>) -> PanelSection {
    PanelSection {
        id: id.into(),
        title: title.into(),
        expanded: true,
        on_toggle: None,
        on_menu: None,
        content: None,
    }
}

impl PanelSection {
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
    /// A right-click anywhere in the section (`onContextMenu`), with where
    /// it was.
    pub fn on_menu(
        mut self,
        handler: impl Fn(&Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_menu = Some(Box::new(handler));
        self
    }
    /// What the section shows while open; `None` shows the header alone.
    pub fn content(mut self, content: Option<AnyElement>) -> Self {
        self.content = content;
        self
    }
}

impl RenderOnce for PanelSection {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let expanded = self.expanded;
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            open: motion::Fade::at(expanded),
            section: motion::Fade::default(),
            header: motion::Fade::default(),
        });
        if state.read(cx).open.on() != expanded {
            state.update(cx, |state, _| state.open.set(expanded, motion::NORMAL));
        }
        let (open, over_section, over_header, header_hovered, animating) = {
            let state = state.read(cx);
            (
                state.open.value(),
                state.section.value(),
                state.header.value(),
                state.header.on(),
                state.open.animating() || state.section.animating() || state.header.animating(),
            )
        };
        if animating {
            window.request_animation_frame();
        }
        // Closed: 30%, 50% with the pointer in the section, 55% on the header.
        let chevron = (1. - open) * (0.3 + 0.2 * over_section + 0.05 * over_header);
        let (section_state, header_state) = (state.clone(), state);
        let mut header = div()
            .id("header")
            .h(px(theme::CONTROL_SM))
            .flex()
            .items_center()
            .cursor_pointer()
            .text(theme::TEXT_SM)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if header_hovered {
                theme.text()
            } else {
                theme.text_tertiary()
            })
            .on_hover(move |hovered, _, cx| {
                header_state.update(cx, |state, cx| {
                    state.header.set(*hovered, motion::NORMAL);
                    cx.notify();
                })
            })
            .child(div().flex_1().child(self.title))
            .child(
                div()
                    .size(px(theme::ICON_BUTTON))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path("chevron-section")
                            .size(px(14.))
                            .text_color(if header_hovered {
                                theme.text()
                            } else {
                                theme.text_tertiary()
                            })
                            .opacity(chevron)
                            .with_transformation(Transformation::rotate(radians(
                                std::f32::consts::FRAC_PI_2 * open,
                            ))),
                    ),
            );
        if let Some(handler) = self.on_toggle {
            header = header.on_click(handler);
        }
        let mut section = div()
            .id(self.id)
            .w_full()
            .mt(px(10.))
            .pt(px(10.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .on_hover(move |hovered, _, cx| {
                section_state.update(cx, |state, cx| {
                    state.section.set(*hovered, motion::NORMAL);
                    cx.notify();
                })
            })
            .child(header);
        if let Some(handler) = self.on_menu {
            section = section.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                cx.stop_propagation();
                handler(&event.position, window, cx);
            });
        }
        if let Some(content) = self.content.filter(|_| expanded) {
            section = section.child(div().mt(px(4.)).child(content));
        }
        section
    }
}
