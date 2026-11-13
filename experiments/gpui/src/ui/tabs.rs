use crate::theme;
use crate::theme::Text as _;
use gpui::*;

type SelectHandler = Box<dyn Fn(usize, &mut Window, &mut App) + 'static>;

/// A segmented tab strip (`.subscriptionTabs`): tabs on a faint track, the
/// chosen one raised on the active surface with a soft shadow. A tab's label
/// is the tertiary text colour, the secondary under the pointer, and the
/// primary when chosen.
#[derive(IntoElement)]
pub struct Tabs {
    id: ElementId,
    labels: Vec<SharedString>,
    selected: usize,
    on_select: Option<SelectHandler>,
}

pub fn tabs(id: impl Into<ElementId>, labels: Vec<SharedString>, selected: usize) -> Tabs {
    Tabs {
        id: id.into(),
        labels,
        selected,
        on_select: None,
    }
}

impl Tabs {
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Tabs {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let handler = self.on_select.map(std::rc::Rc::new);
        let mut strip = div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(3.))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme.footer_surface());
        for (index, label) in self.labels.into_iter().enumerate() {
            let chosen = index == self.selected;
            let mut tab = div()
                .id(("tab", index))
                .h(px(theme::CONTROL_MD))
                .px(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .font_family(theme::FONT_UI)
                .text(theme::TEXT_MD)
                .line_height(px(theme::CONTROL_MD))
                .whitespace_nowrap()
                .cursor_pointer()
                .child(label);
            tab = if chosen {
                tab.bg(theme.surface_active())
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), rgba(0x00000033).into()).blur_radius(px(4.)),
                    ])
                    .text_color(theme.text())
            } else {
                tab.text_color(theme.text_tertiary())
                    .hover(move |style| style.text_color(theme.text_secondary()))
            };
            if let Some(handler) = handler.clone() {
                tab = tab.on_click(move |_, window, cx| handler(index, window, cx));
            }
            strip = strip.child(tab);
        }
        strip
    }
}
