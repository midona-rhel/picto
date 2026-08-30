use crate::NativeSourceAdapter;

use super::moebooru::{self, MoebooruConfig};

pub(super) const CONFIG: MoebooruConfig = MoebooruConfig {
    id: "yandere",
    display_name: "Yande.re",
    domain: "yande.re",
    root: "https://yande.re",
};

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    moebooru::adapter(CONFIG)
}
