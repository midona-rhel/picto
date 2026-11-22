//! Segmented controls: a few choices side by side on a track, of which one is
//! chosen. The controls that show glyphs are on the control surface colour:
//! the match rule of the filter pickers, the sort direction in the view menu,
//! and the view tabs of the pickers. The controls that show labels are tab
//! strips: the tabs of a subscription's detail screen and the tabs of the
//! batch rename dialog.
use super::tooltip::{self, Tip};
use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use std::rc::Rc;

type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>;

/// Which of the controls to draw.
#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// The match rule of the filter pickers: 24px buttons, 2px apart. The
    /// button under the pointer has the subtle overlay as its background, and
    /// the chosen one has the active surface colour.
    Logic,
    /// The sort direction in the view menu: buttons 26px wide and 24px high,
    /// 1px apart. A button's glyph is drawn at 40% strength until the button
    /// is chosen. The chosen button has its glyph at full strength and
    /// `Theme::contrast(12.)` as its background.
    Direction,
    /// The view tabs of the pickers: tabs 28px wide and 24px high, with no gap
    /// between them. The chosen one has the active surface colour. A tab does
    /// not change under the pointer.
    View,
    /// Tabs with labels on a faint track. The chosen tab has the active
    /// surface colour and a soft shadow. A tab's label is in the tertiary text
    /// colour, in the secondary text colour under the pointer, and in the
    /// primary text colour when the tab is chosen.
    Tabs,
    /// The tabs of the batch rename dialog. The tabs share the strip's width
    /// equally and are 28px high. The track is black at 20%, with 2px of
    /// padding and 7px corners. A tab's label is in the secondary text colour.
    /// Under the pointer the tab has the hover surface colour and its label
    /// the primary text colour. The chosen tab has the active surface colour
    /// and no shadow.
    FilledTabs,
}

impl Look {
    fn button(self) -> (f32, f32) {
        match self {
            Look::Logic => (24., 24.),
            Look::Direction => (26., 24.),
            Look::View => (28., 24.),
            // Labelled tabs are as wide as their label.
            Look::Tabs => (0., theme::CONTROL_MD),
            Look::FilledTabs => (0., 28.),
        }
    }
    fn gap(self) -> f32 {
        match self {
            Look::Logic | Look::Tabs | Look::FilledTabs => 2.,
            Look::Direction => 1.,
            Look::View => 0.,
        }
    }
    fn labelled(self) -> bool {
        matches!(self, Look::Tabs | Look::FilledTabs)
    }
}

/// One choice: the name of its 14px glyph, and its label. A control with
/// glyphs shows the label in the choice's tooltip, if the control has
/// tooltips. A tab shows its label and no glyph.
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

/// Returns a tab with the label `label`, for [`Look::Tabs`] and
/// [`Look::FilledTabs`].
pub fn tab(label: impl Into<SharedString>) -> Segment {
    segment("", label)
}

#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    look: Look,
    segments: Vec<Segment>,
    selected: Option<usize>,
    tips: bool,
    /// The name that assistive technology gives the group of choices.
    name: Option<SharedString>,
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
        // The match rule and the view tabs show each choice's label in a
        // tooltip. The sort direction has labels only for assistive
        // technology, and a tab shows its label itself.
        tips: matches!(look, Look::Logic | Look::View),
        name: None,
        on_select: None,
    }
}

impl Segmented {
    /// Sets the name that assistive technology gives the group of choices.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (width, height) = self.look.button();
        // For assistive technology, tabs are a tab list in which the chosen
        // tab is selected. The controls with glyphs are a group of buttons in
        // which the chosen button is pressed.
        let tabs = self.look.labelled();
        let count = self.segments.len();
        let mut root = div()
            .id(self.id.clone())
            .role(if tabs { Role::TabList } else { Role::Group })
            .flex()
            .items_center()
            .gap(px(self.look.gap()));
        if let Some(name) = self.name.clone() {
            root = root.aria_label(name);
        }
        let mut root = match self.look {
            Look::Tabs => root
                .p(px(3.))
                .rounded(px(theme::RADIUS_MD))
                .bg(theme.footer_surface()),
            Look::FilledTabs => root.p(px(2.)).rounded(px(7.)).bg(rgba(theme::BLACK_20)),
            _ => root
                .occlude()
                .flex_none()
                .p(px(2.))
                .rounded(px(theme::RADIUS_MD))
                .bg(theme.control_surface()),
        };
        for (index, segment) in self.segments.into_iter().enumerate() {
            let chosen = self.selected == Some(index);
            let id = ElementId::NamedChild(
                std::sync::Arc::new(self.id.clone()),
                format!("segment-{index}").into(),
            );
            // Each segment is its own Tab stop. Enter and Space choose it. It
            // draws no focus ring.
            let focus = super::focus::control(&id, window, cx);
            let mut button = div()
                .id(id)
                .aria_label(segment.label.clone())
                .aria_position_in_set(index + 1)
                .aria_size_of_set(count)
                .h(px(height))
                .flex()
                .items_center()
                .justify_center();
            button = if tabs {
                button.role(Role::Tab).aria_selected(chosen)
            } else {
                button
                    .role(Role::Button)
                    .aria_toggled(super::a11y::toggled(chosen))
            };
            if !self.look.labelled() {
                button = button
                    .w(px(width))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer();
            }
            let glyph = || super::icon(segment.icon, 14., theme.text());
            let label = segment.label.clone();
            button = match self.look {
                Look::Logic => {
                    let button = if chosen {
                        button.bg(theme.surface_active())
                    } else {
                        button.hover(move |style| style.bg(theme.subtle_overlay()))
                    };
                    button.child(glyph())
                }
                Look::View => {
                    let button = if chosen {
                        button.bg(theme.surface_active())
                    } else {
                        button
                    };
                    button.child(glyph())
                }
                Look::Direction => {
                    let button = if chosen {
                        button.bg(theme.contrast(12.))
                    } else {
                        button
                    };
                    button.child(glyph().opacity(if chosen { 1. } else { 0.4 }))
                }
                Look::Tabs => {
                    let button = button
                        .px(px(16.))
                        .rounded(px(theme::RADIUS_SM))
                        .font_family(theme::FONT_UI)
                        .text(theme::TEXT_MD)
                        .line_height(px(theme::CONTROL_MD))
                        .whitespace_nowrap()
                        .cursor_pointer()
                        .child(label);
                    if chosen {
                        button
                            .bg(theme.surface_active())
                            .shadow(vec![
                                BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_20).into())
                                    .blur_radius(px(4.)),
                            ])
                            .text_color(theme.text())
                    } else {
                        button
                            .text_color(theme.text_tertiary())
                            .hover(move |style| style.text_color(theme.text_secondary()))
                    }
                }
                Look::FilledTabs => {
                    let button = button
                        .flex_1()
                        .min_w_0()
                        .rounded(px(5.))
                        .font_family(theme::FONT_UI)
                        .text(theme::TEXT_MD)
                        .whitespace_nowrap()
                        .child(label);
                    let text = theme.text();
                    if chosen {
                        button.bg(theme.surface_active()).text_color(text)
                    } else {
                        let hover = theme.surface_hover();
                        button
                            .text_color(theme.text_secondary())
                            .hover(move |style| style.bg(hover).text_color(text))
                    }
                }
            };
            if let Some(handler) = self.on_select.clone() {
                let pressed = handler.clone();
                button = button.on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    handler(index, window, cx)
                });
                button = focus.attach(button, super::focus::Kind::Button, move |_, window, cx| {
                    pressed(index, window, cx)
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
