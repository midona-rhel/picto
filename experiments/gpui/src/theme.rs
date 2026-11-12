use gpui::*;
// Resolved dark-theme tokens from src/shared/styles/tokens.css.
pub const BACKGROUND: u32 = 0x18191c;
pub const RAIL: u32 = 0x1f2023;
pub const TEXT: u32 = 0xf8f9fb;
pub const ACCENT: u32 = 0x3297ff;
pub const ROW_HEIGHT: f32 = 27.;
/// `--color-surface-2`: 5% contrast over the background.
pub const PLACEHOLDER: u32 = 0x232427;
/// `--color-border-primary`: 10% contrast.
pub const HAIRLINE: u32 = 0xf8f9fb1a;
/// `--color-text-tertiary`: 60% text.
pub const TEXT_TERTIARY: u32 = 0xf8f9fb99;
pub const BADGE_TEXT: u32 = 0xffffffcc;
pub const TILE_RADIUS: f32 = 4.;
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "books" => include_bytes!("../assets/books.svg"),
            "photo" => include_bytes!("../assets/photo.svg"),
            "clock" => include_bytes!("../assets/clock.svg"),
            "folder-question" => include_bytes!("../assets/folder-question.svg"),
            "bookmark-question" => include_bytes!("../assets/bookmark-question.svg"),
            "inbox" => include_bytes!("../assets/inbox.svg"),
            "trash" => include_bytes!("../assets/trash.svg"),
            "folder" => include_bytes!("../assets/folder.svg"),
            "folders" => include_bytes!("../assets/folders.svg"),
            "tag" => include_bytes!("../assets/tag.svg"),
            "search" => include_bytes!("../assets/search.svg"),
            "layout-grid" => include_bytes!("../assets/layout-grid.svg"),
            "chevron-right" => include_bytes!("../assets/chevron-right.svg"),
            "chevron-left" => include_bytes!("../assets/chevron-left.svg"),
            "chevron-down" => include_bytes!("../assets/chevron-down.svg"),
            _ => return Ok(None),
        };
        Ok(Some(std::borrow::Cow::Borrowed(bytes)))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
pub fn icon(name: &'static str) -> impl IntoElement {
    svg().path(name).size(px(16.)).text_color(rgb(TEXT))
}
