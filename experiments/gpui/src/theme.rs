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
/// `--glass-bg`: the background at 92%.
pub const GLASS: u32 = 0x18191ceb;
pub const TILE_RADIUS: f32 = 4.;
/// `--sidebar-width`
pub const SIDEBAR_WIDTH: f32 = 300.;
pub const INSPECTOR_WIDTH: f32 = 280.;
/// `--titlebar-height`
pub const TITLEBAR_HEIGHT: f32 = 48.;
/// `--color-border-secondary` over the rails.
pub const RAIL_BORDER: u32 = 0x353638;
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
    "folder-open",
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
/// `--color-surface-hover`: 5% contrast.
pub const SURFACE_HOVER: u32 = 0xf8f9fb0d;
/// `--color-surface-active`: 10% contrast.
pub const SURFACE_ACTIVE: u32 = 0xf8f9fb1a;
/// `--color-hover-overlay`: 8% contrast.
pub const HOVER_OVERLAY: u32 = 0xf8f9fb14;
/// `--color-active-overlay`: 14% contrast.
pub const ACTIVE_OVERLAY: u32 = 0xf8f9fb24;
/// A sidebar row glyph: 19px, dimmed until its row is hovered or active.
pub fn sidebar_icon(name: &'static str, lit: bool) -> impl IntoElement {
    svg()
        .path(name)
        .size(px(19.))
        .text_color(rgb(TEXT))
        .opacity(if lit { 1. } else { 0.8 })
}
