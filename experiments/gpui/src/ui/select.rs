use crate::theme;
use crate::theme::Text as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{cell::Cell, rc::Rc};

type ChangeHandler = Box<dyn Fn(&'static str, &mut Window, &mut App) + 'static>;

/// One choice of a select: the value it stands for, its label, and an
/// optional 14px glyph (turned a quarter left when `rotated`).
#[derive(Clone)]
pub struct Choice {
    pub value: &'static str,
    pub label: SharedString,
    pub icon: Option<(&'static str, bool)>,
}

pub fn choice(value: &'static str, label: impl Into<SharedString>) -> Choice {
    Choice {
        value,
        label: label.into(),
        icon: None,
    }
}

impl Choice {
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some((icon, false));
        self
    }
    /// The glyph turned -90°, as the Justified layout's is.
    pub fn rotated_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some((icon, true));
        self
    }
}

/// What a select keeps between frames.
struct State {
    open: bool,
    /// The row under the pointer, which starts at the current choice.
    highlighted: Option<usize>,
    /// Where the button was last painted, which places the menu.
    button: Rc<Cell<Bounds<Pixels>>>,
}

/// `CmSelect.tsx`: a 26px button showing the current choice with a selector
/// glyph, as wide as its widest choice unless given a width, and a menu of
/// the choices 4px below it (above it when there is no room below), as wide
/// as the button. A press anywhere else closes the menu.
#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    value: SharedString,
    choices: Vec<Choice>,
    width: Option<f32>,
    on_change: Option<ChangeHandler>,
}

pub fn select(
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    choices: Vec<Choice>,
) -> Select {
    Select {
        id: id.into(),
        value: value.into(),
        choices,
        width: None,
        on_change: None,
    }
}

impl Select {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn on_change(
        mut self,
        handler: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

/// `--control-height-sm`, the height of the button and of each choice.
const ROW: f32 = theme::CONTROL_SM;

/// Whether the menu opens above the button (`CmSelect`'s `flipUp`): when it
/// does not fit below and there is more room above.
pub fn opens_upward(choices: usize, button_top: f32, button_bottom: f32, viewport: f32) -> bool {
    let menu = (choices as f32 * ROW + 8.).min(164.);
    let below = viewport - button_bottom - 8.;
    menu > below && button_top > below
}

fn glyph(icon: Option<(&'static str, bool)>, color: Rgba) -> Option<AnyElement> {
    let (name, rotated) = icon?;
    let mut glyph = super::icon(name, 14., color);
    if rotated {
        glyph = glyph.with_transformation(Transformation::rotate(radians(
            -std::f32::consts::FRAC_PI_2,
        )));
    }
    Some(
        div()
            .w(px(20.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(glyph)
            .into_any_element(),
    )
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            open: false,
            highlighted: None,
            button: Rc::new(Cell::new(Bounds::default())),
        });
        let current = self
            .choices
            .iter()
            .position(|choice| choice.value == self.value.as_ref());
        let (open, highlighted, measured) = {
            let state = state.read(cx);
            (state.open, state.highlighted, state.button.clone())
        };
        let has_icons = self.choices.iter().any(|choice| choice.icon.is_some());

        // The label column holds every choice's label at no height, so the
        // button is as wide as the widest one.
        let mut label = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div().truncate().child(
                    current
                        .map(|at| self.choices[at].label.clone())
                        .unwrap_or_else(|| self.value.clone()),
                ),
            );
        if self.width.is_none() {
            for choice in &self.choices {
                label = label.child(
                    div()
                        .h_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(choice.label.clone()),
                );
            }
        }
        let toggle_state = state.clone();
        let mut button = div()
            .id(self.id.clone())
            .occlude()
            .relative()
            .h(px(ROW))
            .pl(px(12.))
            .pr(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme.select_surface())
            .border_1()
            .border_color(theme.border_secondary())
            .text_color(theme.text())
            .text(theme::TEXT_MD)
            .line_height(px(24.))
            .whitespace_nowrap()
            .cursor_pointer()
            .children(glyph(
                current.and_then(|at| self.choices[at].icon),
                theme.text(),
            ))
            .child(label)
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
            // Records where the button is, for the menu. A move while open
            // (the page scrolled) draws again with the new place.
            .child({
                let measured = measured.clone();
                let open = open;
                canvas(
                    move |bounds, window, _| {
                        if measured.replace(bounds) != bounds && open {
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
            })
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                toggle_state.update(cx, |state, cx| {
                    state.open = !state.open;
                    state.highlighted = current;
                    cx.notify();
                });
            });
        if let Some(width) = self.width {
            button = button.w(px(width));
        }
        if !open {
            return div().flex().child(button);
        }

        let bounds = measured.get();
        let viewport = window.viewport_size();
        let upward = opens_upward(
            self.choices.len(),
            f32::from(bounds.top()),
            f32::from(bounds.bottom()),
            f32::from(viewport.height),
        );
        let handler = self.on_change.map(Rc::new);
        let mut menu = div()
            .id(ElementId::NamedChild(
                std::sync::Arc::new(self.id.clone()),
                "menu".into(),
            ))
            .occlude()
            .w(bounds.size.width)
            // Seven rows, their gaps and the padding.
            .max_h(px(7. * ROW + 6. * 2. + 12.))
            .overflow_y_scroll()
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme.select_surface())
            .border_1()
            .border_color(theme.border_secondary())
            .shadow(theme::panel_shadow())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .on_mouse_down_out({
                let state = state.clone();
                move |event, _, cx| {
                    // A press on the button itself is the button's to toggle.
                    if bounds.contains(&event.position) {
                        return;
                    }
                    state.update(cx, |state, cx| {
                        state.open = false;
                        cx.notify();
                    });
                }
            });
        for (index, choice) in self.choices.iter().enumerate() {
            let lit = highlighted == Some(index) || current == Some(index);
            let (hover_state, click_state) = (state.clone(), state.clone());
            let handler = handler.clone();
            let value = choice.value;
            menu = menu.child(
                div()
                    .id(index)
                    .h(px(ROW))
                    .flex_none()
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(theme::RADIUS_SM))
                    // `--color-menu-hover` and the chosen row's
                    // `--color-hover-overlay` are the same colour.
                    .when(lit, |row| row.bg(theme.hover_overlay()))
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .when(has_icons, |row| {
                        row.child(
                            glyph(choice.icon, theme.text())
                                .unwrap_or_else(|| div().w(px(20.)).flex_none().into_any_element()),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(choice.label.clone()),
                    )
                    .on_mouse_move(move |_, _, cx| {
                        hover_state.update(cx, |state, cx| {
                            if state.highlighted != Some(index) {
                                state.highlighted = Some(index);
                                cx.notify();
                            }
                        })
                    })
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        click_state.update(cx, |state, cx| {
                            state.open = false;
                            cx.notify();
                        });
                        if let Some(handler) = &handler {
                            handler(value, window, cx);
                        }
                    }),
            );
        }
        let origin = if upward {
            // Its bottom 4px above the button's top.
            point(bounds.left(), bounds.top() - px(4.))
        } else {
            point(bounds.left(), bounds.bottom() + px(4.))
        };
        div().flex().child(button).child(
            deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Window)
                    .anchor(if upward {
                        Anchor::BottomLeft
                    } else {
                        Anchor::TopLeft
                    })
                    .position(origin)
                    .child(menu),
            )
            .with_priority(2),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::opens_upward;

    #[test]
    fn the_menu_opens_upward_only_when_there_is_more_room_above() {
        // Three choices need 86px; 400px below is plenty.
        assert!(!opens_upward(3, 100., 126., 534.));
        // 50px below is too little, and more is above.
        assert!(opens_upward(3, 500., 526., 584.));
        // Too little either way: below wins unless above is larger.
        assert!(!opens_upward(8, 20., 46., 120.));
    }
}
