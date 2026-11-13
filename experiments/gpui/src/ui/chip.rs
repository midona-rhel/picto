use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The label colour for a chip of this colour (`tagTextColors` in
/// `TagChip.tsx`): a pale tint of the chip's hue, or the primary text colour
/// for a grey chip.
fn text_color(color: u32, theme: theme::Theme) -> Rgba {
    let (r, g, b) = (
        (color >> 16 & 0xff) as f32,
        (color >> 8 & 0xff) as f32,
        (color & 0xff) as f32,
    );
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    if delta < 20. {
        return theme.text();
    }
    let hue = if max == r {
        60. * ((g - b) / delta % 6.)
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    let hue = if hue < 0. { hue + 360. } else { hue };
    rgb(match hue {
        h if !(15. ..345.).contains(&h) => 0xf8e7e6,
        h if h < 45. => 0xf8f0e1,
        h if h < 75. => 0xf8f5e1,
        h if h < 155. => 0xe4f5e9,
        h if h < 195. => 0xe5f4f6,
        h if h < 245. => 0xdff1f9,
        h if h < 285. => 0xf3eff9,
        _ => 0xf8eff4,
    })
}

/// `.subtag { max-width: 120px }`
const LABEL_LIMIT: f32 = 120.;

/// The inspector's label chip (`TagChip.tsx`), for tags and folders: a tint
/// of its colour that deepens under the pointer over 200 ms, with an optional
/// namespace prefix and remove button.
#[derive(IntoElement)]
pub struct Chip {
    id: ElementId,
    namespace: Option<SharedString>,
    label: SharedString,
    color: u32,
    on_remove: Option<ClickHandler>,
}

pub fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>, color: u32) -> Chip {
    Chip {
        id: id.into(),
        namespace: None,
        label: label.into(),
        color,
        on_remove: None,
    }
}

impl Chip {
    /// Shown as `namespace:` before the label.
    pub fn namespace(mut self, namespace: Option<SharedString>) -> Self {
        self.namespace = namespace;
        self
    }
    pub fn on_remove(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_remove = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let hover = fade.read(cx).value();
        if fade.read(cx).animating() {
            window.request_animation_frame();
        }
        // The label is cut at 120px. Only a label that long is given the
        // ellipsis: an element that may truncate is measured at the full row
        // width, which would put every chip on a line of its own.
        let mut style = window.text_style();
        style.color = theme.text().into();
        let width = window
            .text_system()
            .shape_line(
                self.label.clone(),
                px(theme::TEXT_SM),
                &[style.to_run(self.label.len())],
                None,
            )
            .width;
        let label = div().h(px(16.)).line_height(px(16.)).child(self.label);
        let label = if width > px(LABEL_LIMIT) {
            label.w(px(LABEL_LIMIT)).truncate()
        } else {
            label
        };
        let tint = |percent: f32| rgba(self.color << 8 | (percent / 100. * 255.).round() as u32);
        let removable = self.on_remove.is_some();
        let mut chip = div()
            .id(self.id.clone())
            .h(px(theme::CONTROL_SM))
            .pl(px(8.))
            .pr(px(if removable { 4. } else { 8. }))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(tint(25. + 25. * hover))
            .bg(tint(10. + 10. * hover))
            .text_color(text_color(self.color, theme))
            .text(theme::TEXT_SM)
            .whitespace_nowrap()
            .cursor_pointer()
            .on_hover(move |hovered, _, cx| {
                fade.update(cx, |fade, cx| {
                    fade.set(*hovered, motion::NORMAL);
                    cx.notify();
                })
            })
            .children(
                self.namespace
                    .map(|namespace| div().child(format!("{namespace}:"))),
            )
            .child(label);
        if let Some(handler) = self.on_remove {
            let remove = div()
                .id("remove")
                .size(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .hover(move |style| style.bg(theme.contrast(10.)))
                .child(
                    super::icon("inspector-remove", 18., theme.text()).opacity(0.5 + 0.5 * hover),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    handler(event, window, cx);
                });
            chip = chip.child(tooltip::attach(
                remove,
                ElementId::NamedChild(std::sync::Arc::new(self.id), "remove-tip".into()),
                Tip::new("Remove"),
                18.,
                window,
                cx,
            ));
        }
        chip
    }
}

#[cfg(test)]
mod tests {
    use super::text_color;
    use crate::theme::DARK;
    use gpui::rgb;

    #[test]
    fn a_chip_label_takes_a_pale_tint_of_the_chip_hue() {
        // The namespace colours of `TagChip.tsx` and the labels they give.
        assert_eq!(text_color(0xff6667, DARK), rgb(0xf8e7e6)); // creator: red
        assert_eq!(text_color(0x30d159, DARK), rgb(0xe4f5e9)); // character: green
        assert_eq!(text_color(0xc499ff, DARK), rgb(0xf3eff9)); // series: purple
        assert_eq!(text_color(0x00aaff, DARK), rgb(0xdff1f9)); // species: blue
        assert_eq!(text_color(0xffd60a, DARK), rgb(0xf8f5e1)); // rating: yellow
        assert_eq!(text_color(0xff9f0a, DARK), rgb(0xf8f0e1)); // system: orange
        // Grey chips (general tags, folders) keep the primary text colour.
        assert_eq!(text_color(0xbdbec0, DARK), DARK.text());
        assert_eq!(text_color(0x868e96, DARK), DARK.text());
    }
}
