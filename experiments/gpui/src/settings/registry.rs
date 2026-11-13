//! The shortcut registry of `src/shared/lib/shortcuts.ts`, as data: every
//! shortcut's id, label, group, description and default keys, the keyboard
//! presets, and how keys are written on the Mac.
use std::collections::BTreeMap;

pub struct ShortcutDef {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub keys: &'static str,
    pub keys2: Option<&'static str>,
    pub description: Option<&'static str>,
    /// Registered for a feature without a handler; Settings does not list it.
    pub deferred: bool,
}

const fn def(
    id: &'static str,
    label: &'static str,
    group: &'static str,
    keys: &'static str,
) -> ShortcutDef {
    ShortcutDef {
        id,
        label,
        group,
        keys,
        keys2: None,
        description: None,
        deferred: false,
    }
}

impl ShortcutDef {
    const fn or(mut self, keys2: &'static str) -> Self {
        self.keys2 = Some(keys2);
        self
    }
    const fn about(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }
    const fn deferred(mut self) -> Self {
        self.deferred = true;
        self
    }
}

const NAV: &str = "Navigation";
const FILE: &str = "File";
const EDIT: &str = "Edit";
const GROUPS: &str = "Groups";
const INBOX: &str = "Inbox";
const VIEW: &str = "View";
const RATING: &str = "Rating";
const VIDEO: &str = "Video";
const DUPLICATES: &str = "Duplicates";

/// `SHORTCUT_DEFS`, in its order.
pub const SHORTCUT_DEFS: &[ShortcutDef] = &[
    def("nav.allActive", "All Active", NAV, "Mod+1"),
    def("nav.inbox", "Inbox", NAV, "Mod+2"),
    def("nav.untagged", "Untagged", NAV, "Mod+3"),
    def("nav.trash", "Trash", NAV, "Mod+4"),
    def("nav.search", "Search", NAV, "Mod+F"),
    def("nav.commandPalette", "Command Palette", NAV, "Mod+K")
        .about("Open command palette")
        .deferred(),
    def("nav.goToFolder", "Go to Folder", NAV, "Mod+J")
        .about("Quick-jump to a folder or smart folder")
        .deferred(),
    def("nav.back", "Go Back", NAV, "Alt+ArrowLeft").or("Mod+["),
    def("nav.forward", "Go Forward", NAV, "Alt+ArrowRight").or("Mod+]"),
    def("file.import", "Import Files", FILE, "Mod+I"),
    def("file.export", "Export Originals", FILE, "Mod+E"),
    def("file.exportAs", "Export As...", FILE, "Mod+Shift+E"),
    def("file.settings", "Settings", FILE, "Mod+,"),
    def("file.delete", "Delete", FILE, "Mod+Backspace")
        .about("Context-dependent: trash in active scope, permanent delete in trash scope"),
    def(
        "file.restore",
        "Restore from Trash",
        FILE,
        "Mod+Shift+Backspace",
    ),
    def("file.newFolder", "New Folder", FILE, "Mod+Shift+N")
        .about("Create a new folder in the sidebar"),
    def("file.newSubfolder", "New Subfolder", FILE, "Alt+N")
        .about("Create a subfolder under the current folder"),
    def(
        "file.newSmartFolder",
        "New Smart Folder",
        FILE,
        "Mod+Shift+Alt+N",
    )
    .about("Create a new smart folder"),
    def(
        "folder.autoTags",
        "Set Folder Auto Tags",
        FILE,
        "Mod+Shift+R",
    )
    .about("Set tags applied when media enters the selected folder"),
    def("file.addToFolder", "Add to Folder...", FILE, "Mod+Shift+J")
        .about("Open folder picker to add selected files"),
    def(
        "file.addToLastFolder",
        "Add to Last Folder",
        FILE,
        "Shift+D",
    )
    .about("Add selected files to the last used folder")
    .deferred(),
    def(
        "file.removeFromFolder",
        "Remove from Folder",
        FILE,
        "Mod+Backspace",
    ),
    def(
        "file.regenerateThumbnail",
        "Regenerate Thumbnail",
        FILE,
        "Mod+Shift+T",
    )
    .about("Regenerate thumbnails for selected files"),
    def(
        "file.openDefaultApp",
        "Open with Default App",
        FILE,
        "Shift+Enter",
    ),
    def("file.revealInFolder", "Reveal in Folder", FILE, "Mod+Enter"),
    def("file.openNewWindow", "Open in New Window", FILE, "Mod+O"),
    def("organize.addTag", "Add Tags", EDIT, "T").about("Open tag panel for selected images"),
    def("organize.addFolder", "Add to Folders", EDIT, "F")
        .about("Open folder picker for selected images"),
    def("organize.autoTag", "Auto-Tag", EDIT, "Mod+Shift+A")
        .about("Open AI auto-tagger for selected images"),
    def("edit.undo", "Undo", EDIT, "Mod+Z"),
    def("edit.redo", "Redo", EDIT, "Mod+Shift+Z"),
    def("edit.selectAll", "Select All", EDIT, "Mod+A"),
    def("edit.deselectAll", "Deselect All", EDIT, "Escape"),
    def("edit.rename", "Rename", EDIT, "Ctrl+R").about("Rename selected file"),
    def("edit.copy", "Copy", EDIT, "Mod+C"),
    def("edit.copyFilePath", "Copy File Path", EDIT, "Mod+Alt+C"),
    def("edit.copyTags", "Copy Tags", EDIT, "Mod+Shift+C").deferred(),
    def("edit.pasteTags", "Paste Tags", EDIT, "Mod+Shift+V").deferred(),
    def("edit.pasteImport", "Paste Import", EDIT, "Mod+V"),
    def("group.removeMembers", "Remove from Group", GROUPS, "Delete").or("Backspace"),
    def("inbox.accept", "Accept", INBOX, "Z").about("Accept inbox item (set to active)"),
    def("inbox.reject", "Reject", INBOX, "X").about("Reject inbox item (move to trash)"),
    def("view.detailView", "Media View", VIEW, "Enter").about("Open selected image in media view"),
    def("view.quicklook", "Quick Look", VIEW, "Space").about("Preview selected image"),
    def("view.fitWindow", "Fit to Window", VIEW, "`")
        .or("Shift+F")
        .about("EU: Shift+F (backtick inaccessible on DE/FR/Nordic)"),
    def("view.actualSize", "Actual Size", VIEW, "Mod+0"),
    def("view.zoomIn", "Zoom In", VIEW, "+").or("="),
    def("view.zoomOut", "Zoom Out", VIEW, "-"),
    def("view.grayscale", "Toggle Grayscale", VIEW, "Mod+Alt+G")
        .about("Toggle grayscale preview mode"),
    def("view.slideshow", "Slideshow", VIEW, "F5")
        .about("Start slideshow presentation mode")
        .deferred(),
    def("view.prevImage", "Previous Image", VIEW, "ArrowLeft").or("A"),
    def("view.nextImage", "Next Image", VIEW, "ArrowRight").or("D"),
    def("view.closeDetail", "Close Media View", VIEW, "Escape"),
    def("view.alwaysOnTop", "Always on Top", VIEW, "Shift+T").about("Toggle window always on top"),
    def("view.navigator", "Toggle Navigator", VIEW, "Mod+Alt+8")
        .about("Toggle navigator overlay when zoomed")
        .deferred(),
    def("view.toggleSidebar", "Toggle Sidebar", VIEW, "Mod+Alt+1"),
    def(
        "view.toggleInspector",
        "Toggle Inspector",
        VIEW,
        "Mod+Alt+2",
    ),
    def("view.toggleBothPanels", "Toggle Panels", VIEW, "Tab"),
    def("view.layoutGrid", "Grid Layout", VIEW, "Alt+1").deferred(),
    def("view.layoutWaterfall", "Waterfall Layout", VIEW, "Alt+2").deferred(),
    def("view.layoutJustified", "Justified Layout", VIEW, "Alt+3").deferred(),
    def("view.toggleTileName", "Toggle Tile Name", VIEW, "Mod+Alt+4").deferred(),
    def(
        "view.toggleTileMetadata",
        "Toggle Tile Info",
        VIEW,
        "Mod+Alt+5",
    )
    .about("Show or hide resolution and extension on tiles")
    .deferred(),
    def("view.toggleLogs", "Toggle Logs", VIEW, "Mod+L").about("Show or hide the log viewer panel"),
    def("document.previousPage", "Previous Page", VIEW, "J")
        .about("Open the previous page in a document"),
    def("document.nextPage", "Next Page", VIEW, "L").about("Open the next page in a document"),
    def("grid.moveLeft", "Grid: Move Left", NAV, "ArrowLeft").or("A"),
    def("grid.moveRight", "Grid: Move Right", NAV, "ArrowRight").or("D"),
    def("grid.moveUp", "Grid: Move Up", NAV, "ArrowUp").or("W"),
    def("grid.moveDown", "Grid: Move Down", NAV, "ArrowDown").or("S"),
    def("grid.first", "First Image", NAV, "Home"),
    def("grid.last", "Last Image", NAV, "End"),
    def("grid.pageUp", "Page Up", NAV, "PageUp").about("Jump up by one screenful"),
    def("grid.pageDown", "Page Down", NAV, "PageDown").about("Jump down by one screenful"),
    def("rate.0", "Clear Rating", RATING, "0").about("Remove rating from selected images"),
    def("rate.1", "Rate 1 Star", RATING, "1").about("Rate selected images 1 star"),
    def("rate.2", "Rate 2 Stars", RATING, "2").about("Rate selected images 2 stars"),
    def("rate.3", "Rate 3 Stars", RATING, "3").about("Rate selected images 3 stars"),
    def("rate.4", "Rate 4 Stars", RATING, "4").about("Rate selected images 4 stars"),
    def("rate.5", "Rate 5 Stars", RATING, "5").about("Rate selected images 5 stars"),
    def("video.togglePlay", "Toggle Play/Pause", VIDEO, "K")
        .about("Play or pause video; Space remains the Quick Look toggle"),
    def("video.seekBackward", "Seek Backward", VIDEO, "J").about("Seek backward 5 seconds"),
    def("video.seekForward", "Seek Forward", VIDEO, "L").about("Seek forward 5 seconds"),
    def("video.volumeUp", "Volume Up", VIDEO, "ArrowUp").about("Increase volume"),
    def("video.volumeDown", "Volume Down", VIDEO, "ArrowDown").about("Decrease volume"),
    def("video.toggleMute", "Toggle Mute", VIDEO, "M").about("Mute or unmute video"),
    def("video.toggleLoop", "Toggle Loop", VIDEO, "O").about("Toggle loop playback"),
    def("video.rateIncrease", "Speed Up", VIDEO, "]")
        .or("Shift+.")
        .about("Increase playback speed"),
    def("video.rateDecrease", "Slow Down", VIDEO, "[")
        .or("Shift+,")
        .about("Decrease playback speed"),
    def("video.rateReset", "Reset Speed", VIDEO, "\\")
        .or("Shift+R")
        .about("Reset playback speed to 1x"),
    def("video.fullscreen", "Toggle Fullscreen", VIDEO, "F")
        .about("Enter or leave fullscreen playback"),
    def("dup.smartMerge", "Smart Merge", DUPLICATES, "S")
        .about("Auto-merge keeping the better file"),
    def("dup.keepLeft", "Keep Left", DUPLICATES, "Z").about("Keep the left file, delete right"),
    def("dup.keepRight", "Keep Right", DUPLICATES, "X").about("Keep the right file, delete left"),
    def("dup.keepBoth", "Keep Both", DUPLICATES, "B").about("Keep both files and dismiss the pair"),
    def("dup.notDuplicate", "Not Duplicate", DUPLICATES, "N").about("Mark pair as not duplicate"),
    def("dup.fitToWindow", "Fit to Window", DUPLICATES, "F")
        .about("Reset zoom to fit images in view"),
    def("dup.prevPair", "Previous Pair", DUPLICATES, "ArrowLeft")
        .about("Go to previous duplicate pair"),
    def("dup.nextPair", "Next Pair", DUPLICATES, "ArrowRight").about("Go to next duplicate pair"),
];

/// The groups in the order Settings lists them (`getShortcutGroups`).
const GROUP_ORDER: [&str; 9] = [
    NAV, FILE, EDIT, GROUPS, RATING, VIEW, INBOX, VIDEO, DUPLICATES,
];

/// Shortcuts whose primary key needs AltGr on European layouts; the EU
/// preset swaps their two bindings.
const EU_SWAP_IDS: [&str; 4] = [
    "view.fitWindow",
    "video.rateIncrease",
    "video.rateDecrease",
    "video.rateReset",
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum KeyboardPreset {
    #[default]
    Us,
    Eu,
}

impl KeyboardPreset {
    pub fn value(self) -> &'static str {
        match self {
            KeyboardPreset::Us => "us",
            KeyboardPreset::Eu => "eu",
        }
    }
    pub fn from_value(value: &str) -> Self {
        if value == "eu" {
            KeyboardPreset::Eu
        } else {
            KeyboardPreset::Us
        }
    }
}

/// A binding the user changed: either slot, `Some("")` when cleared.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Override {
    pub keys: Option<String>,
    pub keys2: Option<String>,
}

pub type Overrides = BTreeMap<&'static str, Override>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slot {
    Keys,
    Keys2,
}

/// One shortcut as Settings shows it: the preset applied, then overrides.
#[derive(Clone, Debug, PartialEq)]
pub struct Shortcut {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub description: Option<&'static str>,
    pub keys: String,
    pub keys2: String,
}

fn resolve(def: &ShortcutDef, preset: KeyboardPreset, overrides: &Overrides) -> Shortcut {
    let (mut keys, mut keys2) = (def.keys.to_owned(), def.keys2.unwrap_or("").to_owned());
    if preset == KeyboardPreset::Eu && EU_SWAP_IDS.contains(&def.id) && def.keys2.is_some() {
        std::mem::swap(&mut keys, &mut keys2);
    }
    if let Some(changed) = overrides.get(def.id) {
        if let Some(value) = &changed.keys {
            keys = value.clone();
        }
        if let Some(value) = &changed.keys2 {
            keys2 = value.clone();
        }
    }
    Shortcut {
        id: def.id,
        label: def.label,
        group: def.group,
        description: def.description,
        keys,
        keys2,
    }
}

/// One shortcut by id, the preset and overrides applied (`getShortcut`).
pub fn shortcut(id: &str, preset: KeyboardPreset, overrides: &Overrides) -> Option<Shortcut> {
    SHORTCUT_DEFS
        .iter()
        .find(|def| def.id == id)
        .map(|def| resolve(def, preset, overrides))
}

/// Every shortcut, the preset and overrides applied, in registry order.
pub fn all(preset: KeyboardPreset, overrides: &Overrides) -> Vec<Shortcut> {
    SHORTCUT_DEFS
        .iter()
        .map(|def| resolve(def, preset, overrides))
        .collect()
}

pub struct Group {
    pub name: &'static str,
    pub items: Vec<Shortcut>,
}

/// The working shortcuts, grouped in the Settings order.
pub fn groups(preset: KeyboardPreset, overrides: &Overrides) -> Vec<Group> {
    GROUP_ORDER
        .iter()
        .map(|name| Group {
            name,
            items: SHORTCUT_DEFS
                .iter()
                .filter(|def| !def.deferred && def.group == *name)
                .map(|def| resolve(def, preset, overrides))
                .collect(),
        })
        .filter(|group| !group.items.is_empty())
        .collect()
}

/// `filterGroups`: label, group, description or written keys contain the
/// query, ignoring case.
pub fn filter(groups: Vec<Group>, query: &str) -> Vec<Group> {
    if query.trim().is_empty() {
        return groups;
    }
    let query = query.to_lowercase();
    groups
        .into_iter()
        .map(|group| Group {
            name: group.name,
            items: group
                .items
                .into_iter()
                .filter(|item| {
                    item.label.to_lowercase().contains(&query)
                        || item.group.to_lowercase().contains(&query)
                        || item
                            .description
                            .is_some_and(|text| text.to_lowercase().contains(&query))
                        || display(&item.keys).to_lowercase().contains(&query)
                })
                .collect(),
        })
        .filter(|group| !group.items.is_empty())
        .collect()
}

/// `findShortcutConflict`: another shortcut already bound to `value`.
/// Delete and Remove from Folder share their key on purpose.
pub fn conflict(
    id: &str,
    value: &str,
    preset: KeyboardPreset,
    overrides: &Overrides,
) -> Option<&'static str> {
    if value.is_empty() {
        return None;
    }
    let paired = |a: &str, b: &str| {
        [a, b].contains(&"file.removeFromFolder") && [a, b].contains(&"file.delete")
    };
    SHORTCUT_DEFS
        .iter()
        .map(|def| resolve(def, preset, overrides))
        .filter(|other| !paired(id, other.id))
        .find(|other| other.id != id && (other.keys == value || other.keys2 == value))
        .map(|other| other.label)
}

/// `formatKeysAsArray` with the Mac's symbols.
pub fn parts(keys: &str) -> Vec<String> {
    if keys == "+" {
        return vec!["+".into()];
    }
    let (base, trailing_plus) = match keys.strip_suffix("++") {
        Some(base) => (base, true),
        None => (keys, false),
    };
    let mut parts: Vec<&str> = base.split('+').filter(|part| !part.is_empty()).collect();
    if trailing_plus {
        parts.push("+");
    }
    parts
        .into_iter()
        .map(|part| {
            match part {
                "Mod" => "⌘",
                "Shift" => "⇧",
                "Alt" => "⌥",
                "Ctrl" => "⌃",
                "ArrowLeft" => "←",
                "ArrowRight" => "→",
                "ArrowUp" => "↑",
                "ArrowDown" => "↓",
                "Enter" => "↩",
                "Escape" => "Esc",
                "Backspace" => "⌫",
                "Delete" => "⌦",
                "Space" => "␣",
                "Tab" => "⇥",
                "PageUp" => "PgUp",
                "PageDown" => "PgDn",
                other => other,
            }
            .to_owned()
        })
        .collect()
}

/// `formatKeysDisplay` on the Mac: the symbols run together.
pub fn display(keys: &str) -> String {
    parts(keys).concat()
}

/// `eventToShortcutString`: a pressed key as the registry writes it, or
/// `None` for a modifier on its own. `key` is GPUI's name for the key.
pub fn from_keystroke(
    key: &str,
    command: bool,
    control: bool,
    alt: bool,
    shift: bool,
) -> Option<String> {
    let named = match key {
        "shift" | "control" | "alt" | "platform" | "function" | "" => return None,
        "space" => "Space".to_owned(),
        "left" => "ArrowLeft".to_owned(),
        "right" => "ArrowRight".to_owned(),
        "up" => "ArrowUp".to_owned(),
        "down" => "ArrowDown".to_owned(),
        "enter" => "Enter".to_owned(),
        "escape" => "Escape".to_owned(),
        "backspace" => "Backspace".to_owned(),
        "delete" => "Delete".to_owned(),
        "tab" => "Tab".to_owned(),
        "home" => "Home".to_owned(),
        "end" => "End".to_owned(),
        "pageup" => "PageUp".to_owned(),
        "pagedown" => "PageDown".to_owned(),
        function
            if function.len() > 1
                && function.starts_with('f')
                && function[1..].parse::<u8>().is_ok() =>
        {
            function.to_uppercase()
        }
        single if single.chars().count() == 1 => single.to_uppercase(),
        other => other.to_owned(),
    };
    let mut parts = Vec::new();
    if command {
        parts.push("Mod".to_owned());
    }
    if control {
        parts.push("Ctrl".to_owned());
    }
    if alt {
        parts.push("Alt".to_owned());
    }
    if shift {
        parts.push("Shift".to_owned());
    }
    parts.push(named);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_lists_working_shortcuts_in_group_order() {
        let groups = groups(KeyboardPreset::Us, &Overrides::new());
        let names: Vec<_> = groups.iter().map(|group| group.name).collect();
        assert_eq!(
            names,
            [
                NAV, FILE, EDIT, GROUPS, RATING, VIEW, INBOX, VIDEO, DUPLICATES
            ]
        );
        let navigation: Vec<_> = groups[0].items.iter().map(|item| item.id).collect();
        assert!(!navigation.contains(&"nav.commandPalette"));
        assert_eq!(navigation.first(), Some(&"nav.allActive"));
        assert_eq!(navigation.last(), Some(&"grid.pageDown"));
        assert_eq!(groups[0].items.len(), 15);
    }

    #[test]
    fn the_eu_preset_swaps_altgr_keys_and_overrides_win() {
        let mut overrides = Overrides::new();
        let fit = |groups: &[Group]| {
            groups
                .iter()
                .flat_map(|group| &group.items)
                .find(|item| item.id == "view.fitWindow")
                .map(|item| (item.keys.clone(), item.keys2.clone()))
                .unwrap()
        };
        assert_eq!(
            fit(&groups(KeyboardPreset::Eu, &overrides)),
            ("Shift+F".into(), "`".into())
        );
        overrides.insert(
            "view.fitWindow",
            Override {
                keys2: Some(String::new()),
                ..Override::default()
            },
        );
        assert_eq!(
            fit(&groups(KeyboardPreset::Us, &overrides)),
            ("`".into(), String::new())
        );
    }

    #[test]
    fn keys_are_written_with_mac_symbols() {
        assert_eq!(display("Mod+Shift+Backspace"), "⌘⇧⌫");
        assert_eq!(display("Alt+ArrowLeft"), "⌥←");
        assert_eq!(display("+"), "+");
        assert_eq!(display("Mod++"), "⌘+");
        assert_eq!(display("Mod+,"), "⌘,");
    }

    #[test]
    fn conflicts_skip_the_paired_delete_keys() {
        let none = Overrides::new();
        assert_eq!(
            conflict("file.import", "Mod+Z", KeyboardPreset::Us, &none),
            Some("Undo")
        );
        assert_eq!(
            conflict("file.delete", "Mod+Backspace", KeyboardPreset::Us, &none),
            None
        );
        assert_eq!(conflict("file.import", "", KeyboardPreset::Us, &none), None);
    }

    #[test]
    fn search_matches_written_keys_and_descriptions() {
        let found = filter(groups(KeyboardPreset::Us, &Overrides::new()), "⌘⇧z");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].items[0].id, "edit.redo");
        let found = filter(groups(KeyboardPreset::Us, &Overrides::new()), "screenful");
        assert_eq!(found[0].items.len(), 2);
    }

    #[test]
    fn keystrokes_become_registry_strings() {
        assert_eq!(
            from_keystroke("k", true, false, false, true).as_deref(),
            Some("Mod+Shift+K")
        );
        assert_eq!(
            from_keystroke("left", false, false, true, false).as_deref(),
            Some("Alt+ArrowLeft")
        );
        assert_eq!(
            from_keystroke("f5", false, false, false, false).as_deref(),
            Some("F5")
        );
        assert_eq!(from_keystroke("shift", false, false, false, true), None);
    }
}
