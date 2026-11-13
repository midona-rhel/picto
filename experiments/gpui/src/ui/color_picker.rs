//! `ColorPicker.tsx`: a row of colour swatches, the chosen one ringed in its
//! colour over the app background with a check. In a context menu it is the
//! menu's swatch row ([`menu_row`]); elsewhere it is [`color_picker`].
use super::menu;
use crate::theme;
use gpui::*;
use std::rc::Rc;

/// `FOLDER_COLORS`: the label and the stored hex, `None` for the default.
pub const FOLDER_COLORS: [(&str, Option<&str>); 10] = [
    ("Default", None),
    ("Blue", Some("#339AF0")),
    ("Red", Some("#FA5252")),
    ("Orange", Some("#FD7E14")),
    ("Yellow", Some("#FAB005")),
    ("Green", Some("#40C057")),
    ("Teal", Some("#12B886")),
    ("Indigo", Some("#5C7CFA")),
    ("Purple", Some("#7950F2")),
    ("Pink", Some("#E64980")),
];

const SWATCH: f32 = 18.;

/// Which swatch shows `value` chosen. A colour the row does not have
/// rings nothing, as `local === c.hex` finds no swatch.
pub fn selected(value: Option<&str>) -> Option<usize> {
    FOLDER_COLORS
        .iter()
        .position(|(_, hex)| match (hex, value) {
            (None, None) => true,
            (Some(hex), Some(value)) => hex.eq_ignore_ascii_case(value),
            _ => false,
        })
}

/// The swatch row inside a context menu. Its chosen value is the hex, or
/// empty for the default.
pub fn menu_row(action: &'static str, value: Option<&str>) -> menu::Swatches {
    menu::swatches(
        action,
        FOLDER_COLORS
            .iter()
            .map(|(label, hex)| menu::Swatch {
                label: (*label).into(),
                color: hex.and_then(theme::parse_hex).map(rgb),
                value: hex.unwrap_or_default().into(),
            })
            .collect(),
        selected(value),
    )
}

type Handler = Rc<dyn Fn(Option<&'static str>, &mut Window, &mut App) + 'static>;

/// The swatch row on its own, spread across the width it is given.
#[derive(IntoElement)]
pub struct ColorPicker {
    id: ElementId,
    value: Option<String>,
    on_pick: Option<Handler>,
}

pub fn color_picker(id: impl Into<ElementId>, value: Option<String>) -> ColorPicker {
    ColorPicker {
        id: id.into(),
        value,
        on_pick: None,
    }
}

impl ColorPicker {
    pub fn on_pick(
        mut self,
        handler: impl Fn(Option<&'static str>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_pick = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ColorPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let chosen = selected(self.value.as_deref());
        let mut row = div()
            .id(self.id.clone())
            .flex()
            .items_center()
            .justify_between()
            .py(px(4.));
        for (position, (label, hex)) in FOLDER_COLORS.iter().enumerate() {
            let color = hex
                .and_then(theme::parse_hex)
                .map(rgb)
                .unwrap_or(theme.text());
            let check = if hex.is_some() {
                theme.on_primary()
            } else {
                theme.background()
            };
            let mut ring = div()
                .size(px(SWATCH))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(color);
            if chosen == Some(position) {
                ring = ring
                    .child(super::icon("swatch-check", 10., check))
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), theme.background().into())
                            .spread_radius(px(1.5)),
                        BoxShadow::new(px(0.), px(0.), color.into()).spread_radius(px(3.)),
                    ]);
            }
            let handler = self.on_pick.clone();
            let hex = *hex;
            let target = div()
                .id(("swatch", position))
                .cursor_pointer()
                .child(ring)
                .on_click(move |_, window, cx| {
                    if let Some(handler) = &handler {
                        handler(hex, window, cx);
                    }
                });
            row = row.child(super::tooltip::attach(
                target,
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), (*label).into()),
                super::tip(*label),
                SWATCH,
                window,
                cx,
            ));
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::selected;

    #[test]
    fn the_chosen_swatch_is_found_by_its_hex_in_any_case() {
        assert_eq!(selected(None), Some(0));
        assert_eq!(selected(Some("#fa5252")), Some(2));
        assert_eq!(selected(Some("#123456")), None);
    }
}
