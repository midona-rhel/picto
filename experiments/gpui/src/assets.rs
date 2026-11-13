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
    "x",
    "pin",
    "pin-filled",
    "link",
    "star",
    "star-filled",
    "arrow-up",
    "chevron-section",
    "plus",
    // Picto's own sidebar disclosure triangle (a 7×5 CSS border triangle).
    "triangle-down",
    // Picto's own 16px toolbar glyphs, from src/shared/ui/icons/toolbar-icons.tsx.
    "toolbar-filter",
    "toolbar-layout",
    "toolbar-panel",
    "history-back",
    "history-forward",
    "toolbar-plus",
    "toolbar-minus",
    "inspector-remove",
    "inspector-add",
    "inspector-export",
    // The group glyph of src/shared/ui/icons/group-icons.tsx.
    "group",
    // The grid's hover magnifier, as CanvasGrid.tsx draws it.
    "magnifier",
    // Tabler icons at the 1.25 stroke every context-menu glyph has, and
    // Picto's own rename glyph from sidebar-menu-icons.tsx.
    "menu-external-link",
    "menu-brand-finder",
    "menu-clipboard-copy",
    "menu-trash",
    "menu-arrow-back-up",
    "menu-rename",
);
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(icon_bytes(path).map(std::borrow::Cow::Borrowed))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
