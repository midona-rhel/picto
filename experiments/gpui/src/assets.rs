//! Icon assets embedded in the binary.
use gpui::*;

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
