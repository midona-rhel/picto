//! What a file the viewer cannot show gets (`UnsupportedDocumentViewer.tsx`):
//! the document shell on its one page, with the message that there is no
//! preview for its type in place of the page.
use super::shell;
use crate::i18n::t;
use gpui::*;

/// `Preview is not available for {mime}.`
pub fn message(mime: &str) -> String {
    let mime = if mime.is_empty() {
        t("this file type")
    } else {
        mime
    };
    crate::tf!("Preview is not available for {value0}.", mime)
}

pub fn view(scroll: &ScrollHandle, mime: &str) -> impl IntoElement {
    shell::shell(scroll, 1, 1).error(Some(message(mime).into()))
}

#[cfg(test)]
mod tests {
    use super::message;

    #[test]
    fn the_message_names_the_type() {
        assert_eq!(
            message("application/zip"),
            "Preview is not available for application/zip."
        );
        assert_eq!(message(""), "Preview is not available for this file type.");
    }
}
