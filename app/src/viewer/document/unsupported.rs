//! The view for a file that the viewer cannot show. It is the document shell
//! with one page, and in place of the page it shows the message that there
//! is no preview for the file's type.
use super::shell;
use crate::i18n::t;
use gpui::*;

/// Returns the message "Preview is not available for {mime}." in the app's
/// language. An empty `mime` is written as "this file type".
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
