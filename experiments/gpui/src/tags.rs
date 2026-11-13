//! Tags apart from any one view: their names and groups, the groups'
//! colours and order, the rows every tag context menu starts with, and the
//! starred tags and group colours kept in the library's application
//! settings.
use crate::ui::menu;
use gpui::App;
use picto_library::{TagNamespaceRecord, TagRecord};
use std::collections::BTreeMap;

// ── Names ───────────────────────────────────────────────────────

/// `tagNamespace`: the group a namespace names, or none for General.
pub fn group_of(namespace: &str) -> Option<String> {
    let normalized = namespace.trim().to_lowercase();
    (!normalized.is_empty() && normalized != "general").then_some(normalized)
}

/// `tagName`: `namespace:subname`, or the subname alone in General. This
/// is the name the library adds, removes and stars a tag by.
pub fn name(namespace: &str, subname: &str) -> String {
    name_in_group(subname, group_of(namespace).as_deref())
}

/// [`name`] of a record.
pub fn record_name(tag: &TagRecord) -> String {
    name(&tag.namespace, &tag.subname)
}

/// `tagNameInGroup`: the tag's name once moved into `group`.
pub fn name_in_group(subname: &str, group: Option<&str>) -> String {
    match group {
        Some(group) => format!("{group}:{subname}"),
        None => subname.to_owned(),
    }
}

/// `splitTag`: a typed name's namespace and subname, split at the first
/// colon; a name without one has no namespace.
pub fn split(name: &str) -> (Option<&str>, &str) {
    match name.split_once(':') {
        Some((namespace, subname)) => (Some(namespace), subname),
        None => (None, name),
    }
}

/// `TagChip`'s `showNamespace`: whether a chip may put the namespace before
/// the subname.
pub fn chip_shows_namespace(namespace: &str) -> bool {
    !matches!(namespace, "" | "general" | "default")
}

// ── Colours and order ───────────────────────────────────────────

/// `NS_COLORS` in `TagChip.tsx`: a chip's colour for a namespace. It knows
/// more namespaces than the groups' presets below.
pub fn chip_color(namespace: &str) -> u32 {
    match namespace.to_lowercase().as_str() {
        "creator" | "studio" => 0xff6667,
        "character" | "person" => 0x30d159,
        "series" => 0xc499ff,
        "species" => 0x00aaff,
        "rating" => 0xffd60a,
        "system" => 0xff9f0a,
        _ => 0xbdbec0,
    }
}

/// `tagGroupPresentation`: a group's preset colour and where it sorts.
fn preset(namespace: &str) -> (u32, u8) {
    match namespace.to_lowercase().as_str() {
        "creator" => (0xff6667, 0),
        "series" => (0xc499ff, 1),
        "character" => (0x30d159, 2),
        "species" => (0x00aaff, 3),
        "rating" => (0xffd60a, 4),
        _ => (0xbdbec0, 10),
    }
}

/// `tagGroupOrder`
pub fn group_order(namespace: &str) -> u8 {
    preset(namespace).1
}

/// `tagGroupColor` without the chosen colours: the group's preset.
pub fn group_preset_color(namespace: &str) -> u32 {
    preset(namespace).0
}

/// `tagGroupColor`: the colour chosen for the group, else its preset.
pub fn group_color(namespace: &str, preferences: &Preferences) -> u32 {
    preferences
        .colors
        .get(namespace)
        .and_then(|color| crate::theme::parse_css_color(color))
        .unwrap_or(group_preset_color(namespace))
}

/// The inspector's order for a tag's group (`tagKey` in `Inspector.tsx`):
/// the group order, with General after every named group.
pub fn inspector_order(namespace: &str) -> u8 {
    match namespace.to_lowercase().as_str() {
        "" | "general" => 99,
        other => group_order(other),
    }
}

// ── The common tag menu ─────────────────────────────────────────

/// `buildCommonTagContextEntries`: Filter, Star, the group moves when
/// `groups` (into each other group of `namespaces`, or out of its own), then
/// Copy. Their actions are `filter`, `star`/`unstar`, `move-to-group` (with
/// the group as the value), `remove-from-group` and `copy`; callers append
/// their own rows.
pub fn common_menu(
    tag: &TagRecord,
    namespaces: &[TagNamespaceRecord],
    groups: bool,
    cx: &App,
) -> Vec<menu::Entry> {
    let starred =
        Preferences::from_settings(crate::settings::current(cx)).is_starred(&record_name(tag));
    let current = group_of(&tag.namespace);
    let mut entries: Vec<menu::Entry> = vec![
        // `IconSearch` at the menu rows' 1.25 stroke (`menu-search` is the
        // search field's heavier glyph).
        menu::item("filter", "Filter Items with This Tag")
            .icon("menu-filter")
            .disabled(tag.tag_id.0 == 0)
            .into(),
        if starred {
            menu::item("unstar", "Remove from Starred").icon("menu-star-off")
        } else {
            menu::item("star", "Add to Starred").icon("menu-star")
        }
        .into(),
    ];
    if groups {
        let mut targets = namespaces
            .iter()
            .filter_map(|namespace| group_of(&namespace.name))
            .filter(|group| Some(group) != current.as_ref())
            .collect::<Vec<_>>();
        targets.sort_by(|left, right| crate::text::natural_order(left, right));
        targets.dedup();
        if !targets.is_empty() {
            entries.push(
                menu::submenu(
                    if current.is_some() {
                        "Move to Group…"
                    } else {
                        "Add to Group…"
                    },
                    targets
                        .into_iter()
                        .map(|group| {
                            menu::item("move-to-group", group.clone())
                                .icon("menu-bookmark")
                                .value(group)
                                .into()
                        })
                        .collect(),
                )
                .icon("menu-bookmarks")
                .into(),
            );
        }
        if current.is_some() {
            entries.push(
                menu::item("remove-from-group", "Remove from this Group")
                    .icon("menu-bookmark")
                    .into(),
            );
        }
    }
    entries.push(menu::Entry::Separator);
    entries.push(menu::item("copy", "Copy Tag").icon("menu-copy").into());
    entries
}

/// The tag search a [`TagTokenInput`](crate::ui::tag_token_input) offers:
/// the library's first twelve tags matching what was typed.
pub fn token_search(
    session: gpui::Entity<crate::session::Session>,
) -> crate::ui::tag_token_input::Search {
    std::rc::Rc::new(move |query, cx| {
        session.read(cx).run(move |library| {
            library
                .tag_page(None, Some(&query), 0, 12)
                .map(|(tags, _, _)| {
                    tags.into_iter()
                        .map(|tag| crate::ui::tag_token_input::Found {
                            namespace: tag.namespace,
                            subname: tag.subname,
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
    })
}

// ── Preferences ─────────────────────────────────────────────────

/// The tag preferences of the library's application settings
/// (`tagPreferences.ts`): starred tags by name, and colours chosen for
/// groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    pub starred: Vec<String>,
    pub colors: BTreeMap<String, String>,
}

impl Preferences {
    /// The preferences as the settings model holds them.
    pub fn from_settings(settings: &crate::settings::model::AppSettings) -> Self {
        Self {
            starred: settings.starred_tags.clone(),
            colors: settings
                .tag_group_colors
                .iter()
                .filter_map(|(group, color)| Some((group.clone(), color.as_str()?.to_owned())))
                .collect(),
        }
    }

    /// The group colours as the settings model stores them.
    pub fn colors_json(&self) -> serde_json::Map<String, serde_json::Value> {
        self.colors
            .iter()
            .map(|(group, color)| (group.clone(), serde_json::Value::from(color.clone())))
            .collect()
    }

    pub fn is_starred(&self, name: &str) -> bool {
        self.starred.iter().any(|starred| starred == name)
    }

    /// `setTagStarred`: the starred list, sorted, with `name` in or out.
    pub fn with_star(&self, name: &str, starred: bool) -> Vec<String> {
        let mut tags = self.starred.clone();
        tags.retain(|tag| tag != name);
        if starred {
            tags.push(name.to_owned());
            tags.sort();
        }
        tags
    }

    /// `replaceStarredTag`: the list with a renamed tag's new name, or none
    /// when the old name was not starred.
    pub fn renamed_star(&self, previous: &str, next: &str) -> Option<Vec<String>> {
        if previous == next || !self.is_starred(previous) {
            return None;
        }
        let mut tags = self
            .starred
            .iter()
            .filter(|tag| *tag != previous)
            .cloned()
            .chain([next.to_owned()])
            .collect::<Vec<_>>();
        tags.sort();
        tags.dedup();
        Some(tags)
    }

    /// `setTagGroupColor`
    pub fn with_color(&self, group: &str, color: Option<&str>) -> BTreeMap<String, String> {
        let mut colors = self.colors.clone();
        match color {
            Some(color) => colors.insert(group.to_owned(), color.to_owned()),
            None => colors.remove(group),
        };
        colors
    }

    /// `replaceTagGroupColor`: the colours with a renamed group's moved to
    /// its new name, or none when it had none.
    pub fn renamed_color(&self, previous: &str, next: &str) -> Option<BTreeMap<String, String>> {
        if previous == next {
            return None;
        }
        let color = self.colors.get(previous)?;
        let mut colors = self.colors.clone();
        colors.insert(next.to_owned(), color.clone());
        colors.remove(previous);
        Some(colors)
    }
}

/// Stores a starred list through the settings model, which writes the
/// library's application settings and tells every follower.
fn store_starred(starred: Vec<String>, cx: &mut App) {
    crate::settings::model(cx).update(cx, |model, cx| {
        model.apply_now(|app| app.starred_tags = starred.clone(), cx)
    });
}

/// `setTagStarred`
pub fn set_starred(name: &str, starred: bool, cx: &mut App) {
    let next = Preferences::from_settings(crate::settings::current(cx)).with_star(name, starred);
    store_starred(next, cx);
}

/// `replaceStarredTag`: a renamed tag keeps its star.
pub fn rename_star(previous: &str, next: &str, cx: &mut App) {
    let preferences = Preferences::from_settings(crate::settings::current(cx));
    if let Some(starred) = preferences.renamed_star(previous, next) {
        store_starred(starred, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_leave_out_the_general_group() {
        assert_eq!(name("", "cat"), "cat");
        assert_eq!(name("General", "cat"), "cat");
        assert_eq!(name(" Creator ", "someone"), "creator:someone");
        // "default" is a group of its own here, as `tagName` has it.
        assert_eq!(name("default", "cat"), "default:cat");
        assert_eq!(name_in_group("cat", Some("species")), "species:cat");
        assert_eq!(name_in_group("cat", None), "cat");
        assert_eq!(split("cat"), (None, "cat"));
        assert_eq!(split("species:cat:x"), (Some("species"), "cat:x"));
        assert!(!chip_shows_namespace("default") && chip_shows_namespace("species"));
    }

    #[test]
    fn chips_know_more_namespaces_than_the_group_presets() {
        assert_eq!(chip_color("studio"), 0xff6667);
        assert_eq!(chip_color("Person"), 0x30d159);
        assert_eq!(chip_color("system"), 0xff9f0a);
        assert_eq!(chip_color("meta"), 0xbdbec0);
        let preferences = Preferences::default();
        assert_eq!(group_color("studio", &preferences), 0xbdbec0);
        assert_eq!(group_color("system", &preferences), 0xbdbec0);
    }

    #[test]
    fn groups_take_their_preset_colour_and_order_unless_one_was_chosen() {
        let mut preferences = Preferences::default();
        assert_eq!(group_color("character", &preferences), 0x30d159);
        assert_eq!(group_color("anything", &preferences), 0xbdbec0);
        preferences
            .colors
            .insert("character".into(), "#FA5252".into());
        assert_eq!(group_color("character", &preferences), 0xfa5252);
        assert_eq!(group_order("creator"), 0);
        assert_eq!(group_order("rating"), 4);
        assert_eq!(group_order("meta"), 10);
    }

    #[test]
    fn the_inspector_lists_general_tags_after_every_named_group() {
        assert!(inspector_order("creator") < inspector_order("series"));
        assert!(inspector_order("rating") < inspector_order("somethingelse"));
        assert!(inspector_order("somethingelse") < inspector_order("general"));
        assert_eq!(inspector_order(""), inspector_order("General"));
    }

    #[test]
    fn preferences_read_star_and_rename_as_the_settings_store_them() {
        let settings = crate::settings::model::AppSettings::from_json(&serde_json::json!({
            "starredTags": ["b", "a"],
            "tagGroupColors": {"species": "#40C057"},
        }));
        let preferences = Preferences::from_settings(&settings);
        assert!(preferences.is_starred("a"));
        assert_eq!(preferences.with_star("c", true), ["a", "b", "c"]);
        assert_eq!(preferences.with_star("b", false), ["a"]);
        assert_eq!(
            preferences.renamed_star("b", "species:b"),
            Some(vec!["a".to_owned(), "species:b".to_owned()])
        );
        assert_eq!(preferences.renamed_star("x", "y"), None);
        let moved = preferences.renamed_color("species", "animal").unwrap();
        assert_eq!(moved.get("animal").map(String::as_str), Some("#40C057"));
        assert!(!moved.contains_key("species"));
        assert!(preferences.with_color("species", None).is_empty());
        assert_eq!(
            preferences.colors_json().get("species"),
            Some(&serde_json::json!("#40C057"))
        );
    }
}
