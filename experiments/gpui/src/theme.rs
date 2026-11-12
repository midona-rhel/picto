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
/// Embeds `assets/<name>.svg` for each listed icon name.
macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        fn icon_bytes(path: &str) -> Option<&'static [u8]> {
            match path {
                $($name => Some(include_bytes!(concat!("../assets/", $name, ".svg")).as_slice()),)*
                _ => None,
            }
        }
    };
}
icons!(
    // Tabler outline icons, as the Electron sidebar uses them.
    "books",
    "photo",
    "clock",
    "folder-question",
    "bookmark-question",
    "inbox",
    "trash",
    "folder",
    "folders",
    "tag",
    "search",
    "layout-grid",
    "chevron-right",
    "chevron-left",
    "chevron-down",
    "bookmark",
    "arrows-shuffle",
    "download",
    "copy",
    "settings",
    "selector",
    // Picto's own 16px toolbar glyphs, from src/shared/ui/icons/toolbar-icons.tsx.
    "toolbar-filter",
    "toolbar-layout",
    "toolbar-panel",
    "history-back",
    "history-forward",
    "toolbar-plus",
    "toolbar-minus",
);
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(icon_bytes(path).map(std::borrow::Cow::Borrowed))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
pub fn icon(name: &'static str) -> impl IntoElement {
    svg().path(name).size(px(16.)).text_color(rgb(TEXT))
}
/// `--color-text-secondary`: 70% text.
pub const TEXT_SECONDARY: u32 = 0xf8f9fbb3;
/// A 24px toolbar button holding one 16px glyph.
pub fn icon_button(id: &'static str, name: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .hover(|style| style.bg(rgba(0xf8f9fb14)))
        .child(
            svg()
                .path(name)
                .size(px(16.))
                .text_color(rgba(TEXT_SECONDARY)),
        )
}
