//! The source address field (`InspectorSourceField`): the first address,
//! its host and the rest dimmed, "+N" when there are more, and the link
//! button that opens the list of addresses for editing, with a row each
//! (open, Edit, Delete), an edit field and "Add URL".
use super::Inspector;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, text_input::TextInput, tooltip::Tip};
use gpui::*;
use std::{cell::Cell, rc::Rc};

/// `handleSave`: the list once the address at `index` is saved as `value`.
/// A blank value deletes the address; saving past the end adds one, unless
/// it is blank.
pub fn saved(urls: &[String], index: usize, value: &str) -> Option<Vec<String>> {
    let trimmed = value.trim();
    if index >= urls.len() {
        return (!trimmed.is_empty()).then(|| {
            let mut next = urls.to_vec();
            next.push(trimmed.to_owned());
            next
        });
    }
    let mut next = urls.to_vec();
    if trimmed.is_empty() {
        next.remove(index);
    } else {
        next[index] = trimmed.to_owned();
    }
    Some(next)
}

/// `handleRemove`
pub fn removed(urls: &[String], index: usize) -> Vec<String> {
    urls.iter()
        .enumerate()
        .filter(|(at, _)| *at != index)
        .map(|(_, url)| url.clone())
        .collect()
}

/// `splitUrl`: the host without `www.`, and the path (unless it is `/`),
/// query and fragment; an address that does not parse is all host.
pub fn split(url: &str) -> (String, String) {
    match url::Url::parse(url) {
        Ok(address) => {
            let host = address.host_str().unwrap_or_default();
            let host = host.strip_prefix("www.").unwrap_or(host).to_owned();
            let mut rest = if address.path() == "/" {
                String::new()
            } else {
                address.path().to_owned()
            };
            if let Some(query) = address.query() {
                rest = format!("{rest}?{query}");
            }
            if let Some(fragment) = address.fragment() {
                rest = format!("{rest}#{fragment}");
            }
            (host, rest)
        }
        Err(_) => (url.to_owned(), String::new()),
    }
}

/// The list's state: whether it is open, which row is being edited (the
/// row past the last is a new address), and the field's place.
pub(super) struct SourcesState {
    pub(super) open: bool,
    pub(super) editing: Option<usize>,
    pub(super) input: Entity<TextInput>,
    field: Rc<Cell<Bounds<Pixels>>>,
}

impl SourcesState {
    pub(super) fn new(cx: &mut Context<Inspector>) -> Self {
        Self {
            open: false,
            editing: None,
            input: cx.new(|cx| TextInput::inline("https://...", cx)),
            field: Rc::new(Cell::new(Bounds::default())),
        }
    }
}

/// `UrlLabel`: the host, then the rest at half strength, cut with an
/// ellipsis.
fn label(url: &str) -> Div {
    let (host, rest) = split(url);
    let mut label = div()
        .flex()
        .min_w_0()
        .flex_1()
        .overflow_hidden()
        .whitespace_nowrap()
        .child(div().flex_none().child(host));
    if !rest.is_empty() {
        label = label.child(div().min_w_0().truncate().opacity(0.5).child(rest));
    }
    label
}

impl Inspector {
    /// The addresses the field shows and edits, and whether they can be
    /// edited.
    fn source_urls(&self) -> Option<Vec<String>> {
        match self.shown {
            super::Mode::Item => self.item.as_ref().map(|item| item.root.source_urls.clone()),
            super::Mode::Selection => self.selection_sources(),
            _ => None,
        }
    }

    pub(super) fn close_sources(&mut self, save: bool, cx: &mut Context<Self>) {
        if save {
            self.save_source(cx);
        }
        self.sources.open = false;
        self.sources.editing = None;
        cx.notify();
    }

    /// `handleAdd`: opens the list on a new, empty address.
    fn add_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.source_urls().map_or(0, |urls| urls.len());
        self.edit_source(count, String::new(), window, cx);
    }

    fn edit_source(
        &mut self,
        index: usize,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A row already being edited is saved first, as its blur does.
        self.save_source(cx);
        self.sources.open = true;
        self.sources.editing = Some(index);
        self.sources.input.update(cx, |input, cx| {
            input.set_text(value, cx);
            input.select_everything(cx);
        });
        let handle = self.sources.input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Saves the row being edited, if any.
    pub(super) fn save_source(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.sources.editing.take() else {
            return;
        };
        let value = self.sources.input.read(cx).value();
        let urls = self.source_urls().unwrap_or_default();
        if let Some(next) = saved(&urls, index, &value) {
            self.set_sources(next, cx);
        }
        cx.notify();
    }

    fn toggle_sources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let urls = self.source_urls().unwrap_or_default();
        if urls.is_empty() {
            self.add_source(window, cx);
        } else if self.sources.open {
            self.close_sources(true, cx);
        } else {
            self.sources.open = true;
            cx.notify();
        }
    }

    /// The source field. Without addresses to show (`unavailable`) it holds
    /// a dash and no button.
    pub(super) fn source(
        &mut self,
        urls: Option<&[String]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let unavailable = urls.is_none();
        let urls = urls.unwrap_or_default().to_vec();
        let first = urls.first().cloned();
        let open = self.sources.open && !unavailable;
        let value = if unavailable {
            div().child("—")
        } else {
            match &first {
                Some(url) => label(url),
                None => div().text_color(theme.text_tertiary()).child("http://"),
            }
        };
        let field_bounds = self.sources.field.clone();
        let probe = canvas(
            move |bounds, _, _| field_bounds.set(bounds),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        let mut field = div()
            .id("source-url")
            .relative()
            .h(px(theme::CONTROL_LG))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(if open {
                theme.primary()
            } else {
                theme.border_secondary()
            })
            .bg(if open {
                theme.rail()
            } else {
                theme.control_surface()
            })
            .overflow_hidden()
            .flex()
            .items_center()
            .text(theme::TEXT_MD)
            .line_height(px(18.))
            .whitespace_nowrap()
            .text_color(if unavailable {
                theme.text_tertiary()
            } else {
                theme.text()
            })
            .child(probe)
            .child(
                div()
                    .id("open-source")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .cursor(if unavailable {
                        CursorStyle::Arrow
                    } else {
                        CursorStyle::PointingHand
                    })
                    .child(value)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if unavailable {
                            return;
                        }
                        match &first {
                            Some(address) => cx.open_url(address),
                            None => this.add_source(window, cx),
                        }
                    })),
            );
        if unavailable {
            return div().mb(px(8.)).child(field);
        }
        if urls.len() > 1 {
            field = field.child(
                div()
                    .id("source-count")
                    .group("source-count")
                    .h_full()
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.control_surface()))
                    .child(
                        div()
                            .opacity(0.5)
                            .group_hover("source-count", |style| style.opacity(1.))
                            .child(format!("+{}", urls.len() - 1)),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_sources(window, cx))),
            );
        }
        let action = div()
            .id("manage-sources")
            .group("manage-sources")
            .w(px(31.))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .border_l_1()
            .border_color(theme.border_secondary())
            .bg(if open {
                theme.rail()
            } else {
                gpui::transparent_black().into()
            })
            .cursor_pointer()
            .hover(move |style| style.bg(theme.control_surface()))
            .child(
                ui::icon("link", 14., theme.text())
                    .opacity(if open { 1. } else { 0.5 })
                    .group_hover("manage-sources", |style| style.opacity(1.)),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.toggle_sources(window, cx)));
        field = field.child(action);
        let mut slot = div().relative().mb(px(8.)).child(field);
        if open {
            slot = slot.child(self.source_list(&urls, window, cx));
        }
        slot
    }

    /// `.urlDropdown`: over the field and what follows, leaving the link
    /// button uncovered.
    fn source_list(
        &mut self,
        urls: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let width = f32::from(self.sources.field.get().size.width) - 30.;
        let separator = theme::blend(
            gpui::transparent_black().into(),
            theme.border_secondary(),
            0.55,
        );
        let editing = self.sources.editing;
        let row_button = |id: (&'static str, usize),
                          icon: &'static str,
                          tip: &'static str,
                          window: &mut Window,
                          cx: &mut App| {
            let group = SharedString::from(format!("{}-{}", id.0, id.1));
            let button = div()
                .id(id)
                .group(group.clone())
                .w(px(31.))
                .h(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_l_1()
                .border_color(theme.border_secondary())
                .cursor_pointer()
                .hover(move |style| style.bg(theme.control_surface()))
                .child(
                    ui::icon(icon, 13., theme.text())
                        .opacity(0.5)
                        .group_hover(group, |style| style.opacity(1.)),
                );
            ui::tooltip::attach(
                button,
                (SharedString::from(format!("{}-tip", id.0)), id.1),
                Tip::new(tip),
                32.,
                window,
                cx,
            )
        };
        let input_row = || {
            div()
                .flex_1()
                .min_w_0()
                .h(px(32.))
                .px(px(10.))
                .flex()
                .items_center()
                .child(self.sources.input.clone())
        };
        let mut list = div().w_full().flex().flex_col();
        for (index, url) in urls.iter().enumerate() {
            let mut row = div()
                .id(("source-row", index))
                .min_h(px(32.))
                .flex()
                .items_center();
            if index > 0 {
                row = row.border_t_1().border_color(separator);
            }
            row = if editing == Some(index) {
                row.child(input_row())
            } else {
                let address = url.clone();
                row.child(
                    div()
                        .id(("source-open", index))
                        .flex_1()
                        .min_w_0()
                        .h(px(32.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .hover(move |style| style.bg(theme.control_surface()))
                        // `.urlDropdown .urlLabel { transform: translateY(-1px) }`
                        .child(label(url).mt(px(-2.)))
                        .on_click(move |_, _, cx| cx.open_url(&address)),
                )
            };
            let (edited, kept) = (url.clone(), urls.to_vec());
            row = row
                .child(
                    row_button(
                        ("source-edit", index),
                        "source-edit",
                        "Edit URL",
                        window,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.edit_source(index, edited.clone(), window, cx)
                    })),
                )
                .child(
                    row_button(
                        ("source-delete", index),
                        "source-trash",
                        "Delete URL",
                        window,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.sources.editing == Some(index) {
                            this.sources.editing = None;
                        }
                        this.set_sources(removed(&kept, index), cx);
                        cx.notify();
                    })),
                );
            list = list.child(row);
        }
        if editing == Some(urls.len()) {
            let mut row = div().id("source-new").min_h(px(32.)).flex().items_center();
            if !urls.is_empty() {
                row = row.border_t_1().border_color(separator);
            }
            list = list.child(row.child(input_row()));
        }
        // `.addUrlBtn`
        list = list.child(
            div()
                .id("source-add")
                .w_full()
                .h(px(32.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(4.))
                .border_t_1()
                .border_color(theme.border_secondary())
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .cursor_pointer()
                .hover(move |style| style.text_color(theme.text()))
                .child(ui::icon("source-plus", 13., theme.text_tertiary()))
                .child("Add URL")
                .on_click(cx.listener(|this, _, window, cx| this.add_source(window, cx))),
        );
        let field = self.sources.field.clone();
        let dropdown = div()
            .id("source-list")
            .occlude()
            .w(px(width.max(0.)))
            .min_h(px(32.))
            .max_h(px(240.))
            .overflow_y_scroll()
            .rounded(px(theme::RADIUS_MD))
            .rounded_tr(px(0.))
            .border_1()
            .border_color(theme.primary())
            .bg(theme.rail())
            .shadow(vec![
                BoxShadow::new(px(0.), px(4.), rgba(0x0000002e).into()).blur_radius(px(10.)),
            ])
            .font_family(theme::FONT_INSPECTOR)
            .text(theme::TEXT_MD)
            .line_height(px(18.))
            .text_color(theme.text())
            .on_action(
                cx.listener(|this, _: &crate::ui::text_input::Cancel, _, cx| {
                    // Escape leaves the row as it was.
                    this.sources.editing = None;
                    cx.notify();
                }),
            )
            .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if !field.get().contains(&event.position) {
                    this.close_sources(true, cx);
                }
            }))
            .child(list);
        div()
            .absolute()
            .top_0()
            .left_0()
            .child(deferred(anchored().child(dropdown)).with_priority(2))
    }
}

#[cfg(test)]
mod tests {
    use super::{removed, saved, split};

    fn urls(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn saving_a_row_replaces_it_or_deletes_it_when_blank() {
        let list = urls(&["https://a.example/1", "https://b.example/2"]);
        assert_eq!(
            saved(&list, 1, "  https://c.example  "),
            Some(urls(&["https://a.example/1", "https://c.example"]))
        );
        assert_eq!(saved(&list, 0, " "), Some(urls(&["https://b.example/2"])));
    }

    #[test]
    fn saving_past_the_end_adds_an_address_unless_it_is_blank() {
        let list = urls(&["https://a.example"]);
        assert_eq!(
            saved(&list, 1, "https://new.example"),
            Some(urls(&["https://a.example", "https://new.example"]))
        );
        assert_eq!(saved(&list, 1, ""), None);
        assert_eq!(saved(&[], 0, "x"), Some(urls(&["x"])));
    }

    #[test]
    fn deleting_takes_out_one_address() {
        let list = urls(&["a", "b", "c"]);
        assert_eq!(removed(&list, 1), urls(&["a", "c"]));
        assert_eq!(removed(&list, 5), list);
    }

    #[test]
    fn an_address_shows_its_host_without_www_then_the_rest() {
        assert_eq!(
            split("https://www.example.com/a/b?x=1#top"),
            ("example.com".into(), "/a/b?x=1#top".into())
        );
        assert_eq!(
            split("https://example.com/"),
            ("example.com".into(), "".into())
        );
        assert_eq!(split("not a url"), ("not a url".into(), "".into()));
    }
}
