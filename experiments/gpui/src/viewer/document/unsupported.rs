//! What a file the viewer cannot show gets (`UnsupportedDocumentViewer.tsx`
//! in the document shell): its thumbnail, if it has one, the message that
//! there is no preview for its type, and a button that opens it in the
//! system's default application. Word and PowerPoint files come here by
//! decision: they keep their embedded thumbnail and open elsewhere.
use super::shell;
use crate::theme;
use crate::ui::{self, action_button::Look};
use gpui::*;
use std::path::PathBuf;
use std::sync::Arc;

/// `Preview is not available for {mime}.`
pub fn message(mime: &str) -> String {
    let mime = if mime.is_empty() {
        "this file type"
    } else {
        mime
    };
    format!("Preview is not available for {mime}.")
}

pub fn view(
    scroll: &ScrollHandle,
    thumbnail: Option<Arc<RenderImage>>,
    mime: &str,
    path: PathBuf,
    cx: &App,
) -> impl IntoElement {
    let theme = theme::theme(cx);
    // `.previewImage`: at most half the pane wide and 70% tall, 520px at most.
    let picture = thumbnail.map(|thumbnail| {
        img(thumbnail)
            .max_w(px(520.))
            .max_h(px(520.))
            .w(relative(0.52))
            .h(relative(0.7))
            .object_fit(ObjectFit::Contain)
    });
    let content = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(16.))
        .children(picture)
        .child(
            div()
                .text_size(px(theme::TEXT_SM))
                .line_height(px(theme::TEXT_SM * 1.5))
                .text_color(theme.text_secondary())
                .child(message(mime)),
        )
        .child(
            ui::action_button("document-open-default", Look::Standard)
                .icon("menu-external-link", 14.)
                .label("Open with Default App")
                .tip(ui::tip("Open with Default App").shortcut("file.openDefaultApp", cx))
                .on_click(move |_, _, cx| crate::desktop::open_default(&path, cx)),
        );
    shell::shell(scroll, 1, 1).content(Some(content))
}

#[cfg(test)]
mod tests {
    use super::message;

    #[test]
    fn the_message_names_the_type() {
        assert_eq!(
            message("application/msword"),
            "Preview is not available for application/msword."
        );
        assert_eq!(message(""), "Preview is not available for this file type.");
    }
}
