//! Builds the lists that the Quick Search panel searches: the actions, the
//! places, the tags and the items. Each row of the panel is one [`Entry`].
use super::Kind;
use crate::app_menu;
use crate::backend::{Navigation, Screen};
use crate::i18n::t;
use crate::keymap;
use crate::session::Session;
use crate::settings::registry;
use crate::tf;
use gpui::{Action, App, KeyBindingContextPredicate, Window};
use picto_library::query::{ItemScope, RootSummary};
use picto_library::{RootId, RootKind, TagRecord};
use serde_json::{Value, json};

/// What choosing a row does.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Runs the action at this position of the panel's list of actions.
    Action(usize),
    /// Opens the sidebar destination with this id (`Navigation::id`).
    Place(String),
    /// Opens the page of the subscription with this id on the Subscriptions
    /// screen.
    Subscription(i64),
    /// Shows the items that have the tag with this id.
    Tag(u32),
    /// Shows this item. `collection` is true when the item is a collection.
    Item { root: RootId, collection: bool },
}

/// The picture of an item's row: the content hash of the file whose
/// thumbnail is drawn, and that file's type.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub hash: String,
    pub mime: String,
}

/// One row of the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    /// A name for the row that stays the same between launches, for example
    /// `action:file.import`, `place:folder:5`, `tag:17` or `item:123`. The
    /// recent choices are remembered by this name.
    pub key: String,
    /// The name that is drawn and that the query is matched against.
    pub label: String,
    /// The text drawn after the name. For an action it is the shortcut, for
    /// a place the parent's name, and for a tag the number of items that
    /// have the tag. For an item it is the line of facts under the name.
    pub detail: String,
    /// The name of the row's icon in the embedded icon set.
    pub icon: String,
    /// The colour of the icon, as `0xRRGGBB`. `None` draws the icon in the
    /// secondary text colour.
    pub tint: Option<u32>,
    pub target: Target,
    /// The thumbnail of an item's row. It is `None` for every other kind.
    pub picture: Option<Picture>,
}

/// The icon of every action row. It is the icon of the Shortcuts page in
/// Settings.
const ACTION_ICON: &str = "settings-nav-command";
/// The icon of every tag row. It is the icon of the sidebar's Tags row.
const TAG_ICON: &str = "IconBookmark";
/// The icon of every subscription row. It is the icon of the sidebar's
/// Subscriptions row.
// i18n: an icon's name
const SUBSCRIPTION_ICON: &str = "download";

/// The two shortcuts that open the panel. They are left out of the panel's
/// own list of actions.
const OWN_SHORTCUTS: [&str; 2] = ["nav.commandPalette", "nav.goToFolder"];

/// Returns the commands of the shortcut registry that can run now, as rows,
/// and the action that each row runs. The action of a row is at the position
/// that the row's [`Target::Action`] holds.
///
/// "Now" means: with the keyboard focus where it is when this function is
/// called. The panel calls it before it takes the focus.
///
/// - A command that has a row in the application menu is listed exactly when
///   the menu enables that row (`app_menu::shortcut_rows`), and it runs the
///   row's action.
/// - A command without a menu row is listed when one of the actions that its
///   keys are bound to (`keymap::targets`) would run: the key context of the
///   binding's scope holds at the focused element, and a handler would run
///   the action (`app_menu::handled`).
pub fn actions(window: &Window, cx: &mut App) -> (Vec<Entry>, Vec<Box<dyn Action>>) {
    let menu_rows = app_menu::shortcut_rows(window, cx);
    let contexts = window.context_stack();
    let scope_holds = |scope: keymap::Scope| match scope.predicate() {
        None => true,
        Some(source) => KeyBindingContextPredicate::parse(&source)
            .is_ok_and(|predicate| predicate.eval(&contexts)),
    };
    let mut entries = Vec::new();
    let mut actions: Vec<Box<dyn Action>> = Vec::new();
    for def in registry::SHORTCUT_DEFS {
        if def.deferred || OWN_SHORTCUTS.contains(&def.id) {
            continue;
        }
        let action = match menu_rows.iter().find(|row| row.id == def.id) {
            Some(row) => row.enabled.then(|| row.action.boxed_clone()),
            None => keymap::targets(def.id).into_iter().find_map(|target| {
                let action = (target.action)();
                (scope_holds(target.scope) && app_menu::handled(action.as_ref(), window, cx))
                    .then_some(action)
            }),
        };
        let Some(action) = action else {
            continue;
        };
        entries.push(Entry {
            kind: Kind::Actions,
            key: format!("action:{}", def.id),
            label: t(def.label).to_owned(),
            detail: keymap::hint(def.id, cx)
                .map(|keys| keys.to_string())
                .unwrap_or_default(),
            icon: ACTION_ICON.into(),
            tint: None,
            target: Target::Action(actions.len()),
            picture: None,
        });
        actions.push(action);
    }
    (entries, actions)
}

/// Returns the icon of a sidebar destination, chosen as the sidebar's Quick
/// Access rows choose it: the destination's own icon when the embedded icon
/// set has it, and otherwise the folder icon.
fn place_icon(entry: &Navigation) -> String {
    match &entry.icon {
        Some(name) if crate::assets::has_icon(name) => name.clone(),
        _ => "IconFolder".into(),
    }
}

/// Returns the sidebar's destinations as rows, in the sidebar's order: the
/// system rows, the folders and the smart folders. A smart folder group is
/// left out, because it cannot be opened, and so is a destination that has
/// no sidebar row (Items with This Media).
///
/// The text after a nested folder's name is its parent's name. The text
/// after a top-level folder or smart folder is "Folders" or "Smart Folders",
/// so that the two can be told apart.
pub fn places(session: &Session) -> Vec<Entry> {
    session
        .nav
        .iter()
        .filter(|entry| !entry.group && !matches!(entry.scope, ItemScope::MediaMatches { .. }))
        .map(|entry| {
            let detail = match (
                entry.parent.and_then(|parent| session.nav.get(parent)),
                &entry.scope,
            ) {
                (Some(parent), _) => parent.label.clone(),
                (None, ItemScope::Folder { .. } | ItemScope::FolderTree { .. }) => {
                    t("Folders").to_owned()
                }
                (None, ItemScope::SmartFolder { .. }) => t("Smart Folders").to_owned(),
                _ => String::new(),
            };
            Entry {
                kind: Kind::Places,
                key: format!("place:{}", entry.id()),
                label: entry.label.clone(),
                detail,
                icon: place_icon(entry),
                tint: entry.tint,
                target: Target::Place(entry.id()),
                picture: None,
            }
        })
        .collect()
}

/// Returns the subscriptions as rows, from the answer of the core's
/// `subscriptions.list` command. The text after each name is
/// "Subscriptions".
pub fn subscriptions(list: &Value) -> Vec<Entry> {
    list.get("subscriptions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|subscription| {
            let id = subscription.get("subscription_id")?.as_i64()?;
            let name = subscription.get("name")?.as_str()?;
            Some(Entry {
                kind: Kind::Places,
                key: format!("place:subscription:{id}"),
                label: name.to_owned(),
                detail: t("Subscriptions").to_owned(),
                icon: SUBSCRIPTION_ICON.into(),
                tint: None,
                target: Target::Subscription(id),
                picture: None,
            })
        })
        .collect()
}

/// Returns the position of the Subscriptions screen in the sidebar's
/// destinations.
pub fn subscriptions_screen(session: &Session) -> Option<usize> {
    session
        .nav
        .iter()
        .position(|entry| entry.screen == Some(Screen::Subscriptions))
}

/// Returns the tags in `records` as rows. The tags on the most items come
/// first, and tags on the same number of items are in the order of their
/// names. The text after a tag's name is the number of items that have the
/// tag. The icon of a tag in a group has the colour that the tag's chips
/// have. The icon of a tag without a group has the secondary text colour,
/// as the icons of the other rows have.
pub fn tags(records: Vec<TagRecord>) -> Vec<Entry> {
    let mut named: Vec<(String, TagRecord)> = records
        .into_iter()
        .map(|tag| (crate::tags::record_name(&tag), tag))
        .collect();
    named.sort_by(|(left_name, left), (right_name, right)| {
        right
            .active_count
            .cmp(&left.active_count)
            .then_with(|| crate::text::natural_order(left_name, right_name))
    });
    named
        .into_iter()
        .map(|(name, tag)| Entry {
            kind: Kind::Tags,
            key: format!("tag:{}", tag.tag_id.0),
            label: name,
            detail: crate::i18n::format::grouped(tag.active_count),
            icon: TAG_ICON.into(),
            tint: crate::tags::group_of(&tag.namespace)
                .map(|_| crate::tags::chip_color(&tag.namespace)),
            target: Target::Tag(tag.tag_id.0),
            picture: None,
        })
        .collect()
}

/// Returns the line of facts under an item's name. For a file it is the
/// type, the dimensions when the file has them, and the size, for example
/// "PNG · 1920 × 1080 · 2.4 MB". For a collection it is "Group" and the
/// number of files, for example "Group · 12 files". A collection of one
/// file has only "Group", because the translations have no text for one
/// file.
fn facts(item: &RootSummary) -> String {
    if item.kind == RootKind::Collection {
        let group = t("Group");
        return match item.media_count {
            1 => group.to_owned(),
            count => format!("{group} · {}", tf!("{value0} files", count)),
        };
    }
    let mut parts = vec![crate::text::inspector_type(&item.mime)];
    if let (Some(width), Some(height)) = (item.width, item.height) {
        parts.push(format!("{width} × {height}"));
    }
    parts.push(crate::text::size_to_gigabytes(item.total_size_bytes));
    parts.join(" · ")
}

/// Returns the row of one item of the library.
pub fn item(item: &RootSummary) -> Entry {
    Entry {
        kind: Kind::Items,
        key: format!("item:{}", item.root_id.0),
        label: item.name.clone(),
        detail: facts(item),
        icon: String::new(),
        tint: None,
        target: Target::Item {
            root: item.root_id,
            collection: item.kind == RootKind::Collection,
        },
        picture: Some(Picture {
            hash: item.content_hash.clone(),
            mime: item.mime.clone(),
        }),
    }
}

impl Entry {
    /// Returns what the state file keeps for this row when the user chooses
    /// it. For most rows that is the row's key, because the row is found
    /// again in the lists when the panel next opens. An item is not in any
    /// list until it is searched for, so its record also holds what is
    /// needed to draw its row.
    pub fn to_recent(&self) -> Value {
        match (&self.target, &self.picture) {
            (Target::Item { root, collection }, Some(picture)) => json!({
                "key": self.key,
                "root": root.0,
                "collection": collection,
                "name": self.label,
                "facts": self.detail,
                "hash": picture.hash,
                "mime": picture.mime,
            }),
            _ => json!({ "key": self.key }),
        }
    }

    /// Returns the key of a record that [`Entry::to_recent`] wrote.
    pub fn recent_key(recent: &Value) -> Option<&str> {
        recent.get("key")?.as_str()
    }

    /// Returns the row of an item from a record that [`Entry::to_recent`]
    /// wrote. Returns `None` when the record is not an item's.
    pub fn item_from_recent(recent: &Value) -> Option<Entry> {
        let text = |field: &str| recent.get(field)?.as_str().map(str::to_owned);
        let root = u32::try_from(recent.get("root")?.as_u64()?).ok()?;
        Some(Entry {
            kind: Kind::Items,
            key: text("key")?,
            label: text("name")?,
            detail: text("facts").unwrap_or_default(),
            icon: String::new(),
            tint: None,
            target: Target::Item {
                root: RootId(root),
                collection: recent.get("collection").and_then(Value::as_bool) == Some(true),
            },
            picture: Some(Picture {
                hash: text("hash")?,
                mime: text("mime").unwrap_or_default(),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Kind, Picture, Target, subscriptions};
    use picto_library::RootId;
    use serde_json::json;

    #[test]
    fn an_items_recent_record_gives_back_its_row() {
        let row = Entry {
            kind: Kind::Items,
            key: "item:42".into(),
            label: "harbour at night.png".into(),
            detail: "PNG · 1920 × 1080 · 2.4 MB".into(),
            icon: String::new(),
            tint: None,
            target: Target::Item {
                root: RootId(42),
                collection: false,
            },
            picture: Some(Picture {
                hash: "ab12".into(),
                mime: "image/png".into(),
            }),
        };
        let record = row.to_recent();
        assert_eq!(Entry::recent_key(&record), Some("item:42"));
        assert_eq!(Entry::item_from_recent(&record), Some(row));
        // A record of another kind holds only its key.
        let tag = json!({ "key": "tag:7" });
        assert_eq!(Entry::recent_key(&tag), Some("tag:7"));
        assert_eq!(Entry::item_from_recent(&tag), None);
    }

    #[test]
    fn subscriptions_are_read_from_the_cores_list() {
        let list = json!({
            "subscriptions": [
                { "subscription_id": 3, "name": "harbour photos" },
                { "name": "a subscription without an id is left out" },
            ],
            "global_paused": false,
        });
        let rows = subscriptions(&list);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "place:subscription:3");
        assert_eq!(rows[0].target, Target::Subscription(3));
        assert!(subscriptions(&json!({})).is_empty());
    }
}
