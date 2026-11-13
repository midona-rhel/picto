use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type MenuHandler = Box<dyn Fn(&Point<Pixels>, &mut Window, &mut App) + 'static>;

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
    // `TAG_TEXT_COLORS`: a pale tint on the dark themes, a dark shade on
    // the light ones.
    let (dark, light) = match hue {
        h if !(15. ..345.).contains(&h) => (0xf8e7e6, 0x513636),
        h if h < 45. => (0xf8f0e1, 0x4c3e31),
        h if h < 75. => (0xf8f5e1, 0x4a4335),
        h if h < 155. => (0xe4f5e9, 0x34403c),
        h if h < 195. => (0xe5f4f6, 0x234449),
        h if h < 245. => (0xdff1f9, 0x3c4e5a),
        h if h < 285. => (0xf3eff9, 0x3f3b4a),
        _ => (0xf8eff4, 0x584753),
    };
    rgb(if theme.is_light() { light } else { dark })
}

/// `.subtag { max-width: 120px }`
const LABEL_LIMIT: f32 = 120.;

/// `text-overflow: ellipsis` as Chromium draws it: a label wider than
/// `limit` keeps the most characters that fit with "…" after them, without
/// dropping trailing punctuation as GPUI's own truncation does.
fn ellipsized(label: &str, limit: f32, measure: impl Fn(&str) -> f32) -> SharedString {
    if measure(label) <= limit {
        return SharedString::from(label.to_owned());
    }
    let ends = label
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .collect::<Vec<_>>();
    // The longest prefix that fits with the ellipsis, found by halving.
    let (mut low, mut high) = (0, ends.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        if measure(&format!("{}…", &label[..ends[middle - 1]])) <= limit {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let prefix = if low == 0 {
        ""
    } else {
        &label[..ends[low - 1]]
    };
    SharedString::from(format!("{prefix}…"))
}

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
    on_menu: Option<MenuHandler>,
}

pub fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>, color: u32) -> Chip {
    Chip {
        id: id.into(),
        namespace: None,
        label: label.into(),
        color,
        on_remove: None,
        on_menu: None,
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
    /// A right-click on the chip, with where it was, for its context menu.
    pub fn on_menu(
        mut self,
        handler: impl Fn(&Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_menu = Some(Box::new(handler));
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
        // The label is cut at 120px with an ellipsis. It is cut here rather
        // than by a truncating element, which would be measured at the full
        // row width and put every chip on a line of its own.
        let mut style = window.text_style();
        style.color = theme.text().into();
        let measure = |text: &str| {
            let text = SharedString::from(text.to_owned());
            let run = style.to_run(text.len());
            f32::from(
                window
                    .text_system()
                    .shape_line(text, px(theme::TEXT_SM), &[run], None)
                    .width,
            )
        };
        let shown = ellipsized(&self.label, LABEL_LIMIT, measure);
        let label = div().h(px(16.)).line_height(px(16.)).child(shown);
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
        if let Some(handler) = self.on_menu {
            chip = chip.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                cx.stop_propagation();
                handler(&event.position, window, cx);
            });
        }
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
    use super::{ellipsized, text_color};
    use crate::theme::{DARK, LIGHT};
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
        // The light themes take the dark shade of the same hue.
        assert_eq!(text_color(0xff6667, LIGHT), rgb(0x513636));
        assert_eq!(text_color(0x00aaff, LIGHT), rgb(0x3c4e5a));
        assert_eq!(text_color(0x868e96, LIGHT), LIGHT.text());
    }

    #[test]
    fn a_long_label_keeps_what_fits_with_the_ellipsis_and_its_punctuation() {
        // Every character 10px wide.
        let measure = |text: &str| text.chars().count() as f32 * 10.;
        assert_eq!(ellipsized("short", 120., measure), "short");
        assert_eq!(
            ellipsized("adobe_photoshop_(artwork)", 120., measure),
            "adobe_photo…"
        );
        assert_eq!(ellipsized("abcdefghijk_(x)", 120., measure), "abcdefghijk…");
        assert_eq!(ellipsized("abcdefghij_(x)", 120., measure), "abcdefghij_…");
    }
}
