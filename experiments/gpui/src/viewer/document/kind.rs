//! Which renderer an item's media type gets (`detailRendererKind.ts`). The
//! media surface asks this one function what to show.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RendererKind {
    Image,
    JpegXl,
    Video,
    Audio,
    Pdf,
    Flash,
    Font,
    TextDocument,
    Docx,
    Pptx,
    Epub,
    Cbz,
    Djvu,
    Unsupported,
}

pub fn renderer_kind(mime: &str) -> RendererKind {
    use RendererKind::*;
    if mime.starts_with("video/") {
        return Video;
    }
    if mime.starts_with("audio/") {
        return Audio;
    }
    match mime {
        "image/jxl" => JpegXl,
        "application/pdf" => Pdf,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Docx,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Pptx,
        "application/epub+zip" => Epub,
        "application/vnd.comicbook+zip" | "application/x-cbz" => Cbz,
        "image/vnd.djvu" | "image/x-djvu" => Djvu,
        "text/plain" | "text/markdown" | "application/json" | "application/rtf" | "text/rtf" => {
            TextDocument
        }
        "application/x-shockwave-flash" => Flash,
        _ if mime.starts_with("font/") || mime == "application/font-sfnt" => Font,
        _ if mime.starts_with("image/") => Image,
        _ => Unsupported,
    }
}

impl RendererKind {
    /// The kinds this module draws: paged documents, text, fonts and the
    /// unsupported view. Images, JPEG XL and Flash stay on the media
    /// surface's camera; video and audio have players of their own.
    pub fn is_document(self) -> bool {
        use RendererKind::*;
        matches!(
            self,
            Pdf | Font | TextDocument | Docx | Pptx | Epub | Cbz | Djvu | Unsupported
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{RendererKind::*, renderer_kind};

    #[test]
    fn routes_each_type_as_production_does() {
        for (mime, kind) in [
            ("image/png", Image),
            ("image/jxl", JpegXl),
            ("video/mp4", Video),
            ("audio/mpeg", Audio),
            ("application/pdf", Pdf),
            ("text/plain", TextDocument),
            ("text/markdown", TextDocument),
            ("application/json", TextDocument),
            (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                Docx,
            ),
            (
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                Pptx,
            ),
            ("application/epub+zip", Epub),
            ("application/vnd.comicbook+zip", Cbz),
            ("image/vnd.djvu", Djvu),
            ("application/octet-stream", Unsupported),
            ("application/rtf", TextDocument),
            ("application/x-shockwave-flash", Flash),
            ("font/ttf", Font),
            ("font/otf", Font),
            ("font/woff", Font),
            ("font/collection", Font),
        ] {
            assert_eq!(renderer_kind(mime), kind, "{mime}");
        }
    }
}
