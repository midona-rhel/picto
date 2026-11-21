//! Icon assets embedded in the binary.
use gpui::*;

pub struct Assets;
// Defines `icon_bytes`, which returns the bytes of the icon of a given name.
// build.rs generates it from the SVG files in `assets/`. Those are Tabler
// outline icons (see `assets/LICENSE-tabler`) and icons drawn for Picto.
include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// Whether an icon of this name is embedded.
pub fn has_icon(name: &str) -> bool {
    icon_bytes(name).is_some()
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(icon_bytes(path).map(std::borrow::Cow::Borrowed))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
