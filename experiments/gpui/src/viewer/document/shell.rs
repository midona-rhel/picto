//! The frame that the document viewers draw around a page (production's
//! `DocumentViewerShell.tsx`). It has two parts. The upper part is a viewport
//! with the dark document backdrop, which scrolls and shows the page in its
//! centre. The lower part is a footer as tall as the titlebar, which holds
//! the Previous page button, the text "Page n of m" and the Next page
//! button.
use crate::i18n::t;
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::*;

type ClickHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// The padding in pixels between the viewport's edges and the page
/// (the padding of `.viewport` in production's CSS).
pub const VIEWPORT_PADDING: f32 = 24.;

/// Returns the width and the height in pixels that a page has room for when
/// the shell fills an area of `size`. That is the area's width and its
/// height less the footer's height, each less the padding on both sides of
/// the page. A viewer uses it to lay its page out for the size the shell has
/// in the frame that is being built.
pub fn page_room(size: Size<Pixels>) -> (f32, f32) {
    (
        (f32::from(size.width) - 2. * VIEWPORT_PADDING).max(0.),
        (f32::from(size.height) - theme::TITLEBAR_HEIGHT - 2. * VIEWPORT_PADDING).max(0.),
    )
}

#[derive(IntoElement)]
pub struct DocumentShell {
    /// The number of the page shown, counted from 1, as the footer shows
    /// it.
    page: usize,
    /// The number of pages, or 0 while it is not known.
    count: usize,
    /// A message that is shown in place of the page when the page cannot be
    /// shown.
    error: Option<SharedString>,
    content: Option<AnyElement>,
    scroll: ScrollHandle,
    on_previous: Option<ClickHandler>,
    on_next: Option<ClickHandler>,
}

/// Makes a shell whose viewport scrolls with `scroll` and whose footer shows
/// "Page `page` of `count`". `page` is counted from 1, and `count` is 0 while
/// the number of pages is not known.
pub fn shell(scroll: &ScrollHandle, page: usize, count: usize) -> DocumentShell {
    DocumentShell {
        page,
        count,
        error: None,
        content: None,
        scroll: scroll.clone(),
        on_previous: None,
        on_next: None,
    }
}

impl DocumentShell {
    /// Sets the page that is shown in the viewport. With `None` the
    /// viewport shows only the backdrop, which is what a viewer passes while
    /// it has no page to draw yet.
    pub fn content(mut self, content: Option<impl IntoElement>) -> Self {
        self.content = content.map(IntoElement::into_any_element);
        self
    }

    /// Shows `error` in the centre of the viewport in place of the page
    /// (production's `.message`, which has `role="alert"`). With `None` the
    /// page is shown.
    pub fn error(mut self, error: Option<SharedString>) -> Self {
        self.error = error;
        self
    }

    /// Sets what a click on the Previous page button does. Without a
    /// handler the button is disabled.
    pub fn on_previous(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_previous = Some(Box::new(handler));
        self
    }

    /// Sets what a click on the Next page button does. Without a handler
    /// the button is disabled.
    pub fn on_next(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_next = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for DocumentShell {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let can_previous = self.page > 1 && self.on_previous.is_some();
        let can_next = self.count > 0 && self.page < self.count && self.on_next.is_some();

        let body: AnyElement = match self.error {
            // The message is drawn in the centre of the viewport, in 12 px
            // text with a line height of 1.5 (production's `.message`).
            Some(error) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(theme::TEXT_SM))
                .line_height(px(theme::TEXT_SM * 1.5))
                .text_color(theme.text_secondary())
                // A message that is one of production's translation keys is
                // shown in the app's language. Any other message, such as the
                // detail a decoder reported, is shown as it is.
                .child(crate::i18n::t_shared(error))
                .into_any_element(),
            None => div()
                .id("document-viewport")
                .track_scroll(&self.scroll)
                .overflow_scroll()
                .size_full()
                .flex()
                .child(
                    // This box is at least as large as the viewport and
                    // holds the padding, and the page is centred in it, as
                    // production's `display: grid; place-items: center`
                    // does. A page that is larger than the viewport makes
                    // the box larger, and the box then starts at the
                    // viewport's top-left corner, so every part of the page
                    // can be scrolled into view.
                    div()
                        .flex_none()
                        .min_w_full()
                        .min_h_full()
                        .p(px(VIEWPORT_PADDING))
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(self.content),
                )
                .into_any_element(),
        };
        let viewport = div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .bg(rgb(theme::DOCUMENT_BACKDROP))
            .child(body)
            .child(ui::scrollbar(
                "document-scrollbar",
                &self.scroll,
                window,
                cx,
            ));

        let previous = self.on_previous;
        let next = self.on_next;
        let footer = footer(cx).child(
            // The two buttons with the page count between them, 4 px
            // apart (production's `TitlebarControlGroup`).
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    ui::icon_button("document-previous", "toolbar-chevron-left")
                        .disabled(!can_previous)
                        .tip(ui::tip(t("Previous page")).shortcut("document.previousPage", cx))
                        .on_click(move |_, window, cx| {
                            if let Some(previous) = &previous {
                                previous(window, cx);
                            }
                        }),
                )
                .child(page_status_label(self.page, self.count, cx))
                .child(
                    ui::icon_button("document-next", "toolbar-chevron-right")
                        .disabled(!can_next)
                        .tip(ui::tip(t("Next page")).shortcut("document.nextPage", cx))
                        .on_click(move |_, window, cx| {
                            if let Some(next) = &next {
                                next(window, cx);
                            }
                        }),
                ),
        );

        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.background())
            .text_color(theme.text())
            .child(viewport)
            .child(footer)
    }
}

/// Returns the element that shows "Page n of m" in the footer (production's
/// `.pageStatus`). The text is in the secondary text colour, at medium
/// weight, with digits of equal width. The element is at least 92 px wide,
/// so the buttons beside it do not move when the number of digits changes.
pub fn page_status_label(page: usize, count: usize, cx: &App) -> Div {
    div()
        .min_w(px(92.))
        .flex()
        .justify_center()
        .whitespace_nowrap()
        .text(theme::TEXT_SM)
        .font_weight(FontWeight::MEDIUM)
        .font_features(FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])))
        .text_color(theme::theme(cx).text_secondary())
        .child(page_status(page, count))
}

/// Returns the footer that is drawn under a document (production's
/// `.footer`), without content. It is as tall as the titlebar and has a 1 px
/// line along its top edge. Content added to it is centred in it.
pub fn footer(cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .flex_none()
        .h(px(theme::TITLEBAR_HEIGHT))
        .min_w_0()
        .flex()
        .items_center()
        .justify_center()
        .border_t_1()
        .border_color(theme.border_secondary())
        .bg(theme.background())
        .font_family(theme::FONT_UI)
}

/// Returns the text "Page n of m" for page `page` of `count` pages. While
/// the number of pages is not known (`count` is 0), a dash is written in its
/// place: "Page 1 of —".
pub fn page_status(page: usize, count: usize) -> String {
    // Production builds the text from two translated parts, each with a
    // trailing space: `{t("Page ")}{pageNumber} {t("of ")}{pageCount || '—'}`.
    let (word, of) = (crate::i18n::t("Page "), crate::i18n::t("of "));
    if count == 0 {
        format!("{word}{page} {of}—")
    } else {
        format!("{word}{page} {of}{count}")
    }
}

#[cfg(test)]
mod tests {
    use super::page_status;

    #[test]
    fn the_counter_reads_as_production_does() {
        assert_eq!(page_status(1, 0), "Page 1 of —");
        assert_eq!(page_status(3, 12), "Page 3 of 12");
    }
}
