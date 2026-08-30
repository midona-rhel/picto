use crate::NativeSourceAdapter;

use super::moebooru::{self, MoebooruConfig};

pub(super) const CONFIG: MoebooruConfig = MoebooruConfig {
    id: "konachan",
    display_name: "Konachan",
    domain: "konachan.com",
    root: "https://konachan.com",
};

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    moebooru::adapter(CONFIG)
}
