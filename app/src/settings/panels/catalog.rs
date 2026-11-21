//! The sections of the Settings window, the settings each one holds, and
//! the search that finds them.
use crate::i18n::t;

/// A section of the window (`PANELS`).
pub struct PanelDef {
    pub id: &'static str,
    pub label: &'static str,
    /// The Tabler glyph, drawn at 1.8 in the navigation and at 2 in search
    /// results.
    pub icon: &'static str,
    /// Terms the settings search matches for this section.
    pub keywords: &'static str,
    pub description: &'static str,
    pub separator_before: bool,
}

const fn panel(
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    keywords: &'static str,
    description: &'static str,
    separator_before: bool,
) -> PanelDef {
    PanelDef {
        id,
        label,
        icon,
        keywords,
        description,
        separator_before,
    }
}

// i18n: translated where drawn
pub const PANELS: [PanelDef; 14] = [
    panel(
        "general",
        "General",
        "settings-2",
        "general appearance theme color light dark gray blue purple zoom tags group namespace prefix compact",
        "Appearance and zoom.",
        false,
    ),
    panel(
        "updates",
        "Updates",
        "refresh",
        "updates software version release notes download install latest",
        "Check for new versions of Picto.",
        false,
    ),
    panel(
        "library",
        "Library",
        "library",
        "library statistics media images video audio files size all inbox trash tags folders smart subscriptions collections",
        "Current library contents and storage.",
        false,
    ),
    panel(
        "sidebar",
        "Sidebar",
        "layout-sidebar",
        "sidebar navigation folder tree guides hierarchy lines",
        "Sidebar and folder-tree presentation.",
        false,
    ),
    panel(
        "controls",
        "Controls",
        "adjustments-horizontal",
        "controls grid layout thumbnails spacing density wide tight sort order name resolution extension label count fit",
        "Default grid controls and item presentation.",
        true,
    ),
    panel(
        "preview",
        "Preview",
        "eye",
        "preview image video audio scaling pixelated zoom transparency autoplay loop visualization visualizer spectrum oscilloscope orbit",
        "Media preview behavior.",
        false,
    ),
    panel(
        "shortcuts",
        "Shortcuts",
        "command",
        "shortcuts keyboard shortcut keybind hotkey binding command key layout qwerty qwertz azerty eu us european preset",
        "Keyboard layout and shortcut bindings.",
        false,
    ),
    panel(
        "notifications",
        "Notifications",
        "bell",
        "notifications alerts popups success information warnings errors",
        "Choose which actions show pop-up notifications.",
        true,
    ),
    panel(
        "autoimport",
        "Auto-Import",
        "folder-down",
        "auto import watched folder watch files inbox recursive subfolders",
        "Control imports from watched folders.",
        false,
    ),
    panel(
        "deleting",
        "Inbox & Deleting",
        "trash",
        "inbox reject rejected trash delete permanently undo restore empty",
        "What rejecting does, and how much a delete can be undone.",
        false,
    ),
    panel(
        "subscriptions",
        "Subscriptions",
        "download",
        "subscriptions defaults schedule daily weekly monthly posts run group multi media",
        "Defaults for new subscriptions and source queries.",
        false,
    ),
    panel(
        "cloud",
        "Cloud",
        "cloud",
        "cloud sync backup restore google drive dropbox offline retention snapshots",
        "Library sync, recovery snapshots, and missing files.",
        false,
    ),
    panel(
        "aitagging",
        "AI Models",
        "box",
        "ai tagging tagger models model download threshold confidence auto tag rating",
        "Local models, confidence thresholds, and auto-tag behavior.",
        true,
    ),
    panel(
        "about",
        "About",
        "info-circle",
        "about picto version build copyright license github website release",
        "Version, project information, and licenses.",
        true,
    ),
];

/// `ALL_SETTINGS`: individual settings the search finds, by section.
// i18n: translated where drawn (searched as shown)
const ALL_SETTINGS: [(&str, &str, &str); 14] = [
    (
        "Appearance",
        "theme color light dark gray blue purple zoom",
        "general",
    ),
    (
        "Language",
        "language locale translation english german spanish portuguese french chinese japanese finnish",
        "general",
    ),
    (
        "Keyboard Layout",
        "keyboard layout qwerty qwertz azerty eu us european preset shortcut",
        "shortcuts",
    ),
    (
        "Folder Tree",
        "sidebar folder hierarchy tree guides lines",
        "sidebar",
    ),
    (
        "Grid Defaults",
        "grid layout thumbnail size spacing density wide tight sort name resolution extension label count fit",
        "controls",
    ),
    (
        "Image Preview",
        "image preview scaling smooth pixelated zoom fit actual transparency checkerboard",
        "preview",
    ),
    (
        "Video Preview",
        "video preview autoplay loop playback",
        "preview",
    ),
    (
        "Audio Preview",
        "audio preview visualization spectrum oscilloscope orbit",
        "preview",
    ),
    (
        "Notifications",
        "notifications alerts popups success information warnings errors",
        "notifications",
    ),
    (
        "Rejected items",
        "inbox reject rejected trash delete permanently",
        "deleting",
    ),
    (
        "Undo limit (media items)",
        "undo delete permanently restore trash empty limit",
        "deleting",
    ),
    (
        "Auto-Import",
        "auto import watched folder watch files inbox recursive subfolders",
        "autoimport",
    ),
    (
        "Subscription Defaults",
        "subscriptions schedule daily weekly monthly posts per run group multi media inbox maximum limit capacity",
        "subscriptions",
    ),
    (
        "Cloud Sync and Recovery",
        "cloud sync backup restore google drive dropbox snapshots retention offline",
        "cloud",
    ),
];

/// The sections a search finds (`matchedPanels`): by their label, their
/// keywords, or a setting of theirs whose label or keywords match. The
/// query is already trimmed and lower-cased.
pub fn matched_panels(query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    PANELS
        .iter()
        .enumerate()
        .filter(|(_, panel)| {
            // Labels as shown; the keywords are English, as production's are.
            t(panel.label).to_lowercase().contains(query)
                || panel.keywords.contains(query)
                || ALL_SETTINGS.iter().any(|(label, keywords, of)| {
                    *of == panel.id
                        && (t(label).to_lowercase().contains(query) || keywords.contains(query))
                })
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{PANELS, matched_panels};

    #[test]
    fn search_finds_sections_by_label_keyword_or_setting() {
        let ids = |query: &str| {
            matched_panels(query)
                .into_iter()
                .map(|index| PANELS[index].id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("shortcuts"), ["shortcuts"]);
        // "checkerboard" is only a keyword of the Image Preview setting.
        assert_eq!(ids("checkerboard"), ["preview"]);
        assert!(ids("undo").contains(&"deleting"));
        assert_eq!(ids("trash"), ["library", "deleting"]);
        assert!(ids("").is_empty());
        assert!(ids("zzz").is_empty());
    }
}
