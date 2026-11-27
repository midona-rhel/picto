//! What the synced values show: the visible state of an entity, computed from
//! the records in `sync_value` by the rules of projection. Recording compares
//! the library with this state, and `apply` writes it into the library.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

use super::{Stored, SyncKind, SyncTimestamp};
use crate::Result;

/// A tag that is not visible but was renamed or merged counts as the tag it
/// went to, following at most this many renames.
const FORWARDING_STEPS: usize = 8;

/// The state of one entity in sync keys, as the library holds it or as the
/// synced values show it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct View {
    /// The fields whose values are compared as they are.
    pub fields: BTreeMap<String, Value>,
    /// For items outside a collection and for collections: the tags.
    pub tags: BTreeSet<String>,
    /// For items outside a collection and for collections: the folders that
    /// hold them.
    pub folders: BTreeSet<String>,
    /// For an item, the collection it is a member of; for a folder, its
    /// parent folder.
    pub owner: Option<String>,
}

/// The fields that every item and collection outside a collection has, and
/// the value the projection uses when the synced values do not hold one.
pub(crate) const ROOT_FIELDS: [&str; 6] = [
    "links",
    "imported_at",
    "captured_at",
    "modified_at",
    "lifecycle",
    "rating",
];

fn root_default(field: &str) -> Value {
    match field {
        "links" => json!([]),
        "imported_at" | "modified_at" | "rating" => json!(0),
        "lifecycle" => json!("inbox"),
        _ => Value::Null,
    }
}

pub(crate) type Raw = BTreeMap<String, Stored>;

/// The folders that the synced values show, with the parent and position of
/// each.
#[derive(Default)]
pub(crate) struct FolderTree {
    /// Parent and position key of every visible folder.
    pub place: HashMap<String, (Option<String>, String)>,
    /// The children of each parent (`None` is the top level), in order.
    pub children: BTreeMap<Option<String>, Vec<String>>,
}

/// Reads synced values and computes what they show. Values read once are
/// kept until `forget`, which a writer calls after it changes them.
pub(crate) struct Synced<'a> {
    connection: &'a Connection,
    raws: HashMap<(SyncKind, String), Rc<Raw>>,
    /// Entities that count as visible even though the synced values delete
    /// them: the items and collections that came back in the library under
    /// their old local id, as an undo of Delete Permanently brings them back.
    pub revived: HashSet<(SyncKind, String)>,
    tree: Option<Rc<FolderTree>>,
}

impl<'a> Synced<'a> {
    pub(crate) fn new(connection: &'a Connection) -> Self {
        Self {
            connection,
            raws: HashMap::new(),
            revived: HashSet::new(),
            tree: None,
        }
    }

    pub(crate) fn forget(&mut self) {
        self.raws.clear();
        self.tree = None;
    }

    pub(crate) fn raw(&mut self, kind: SyncKind, key: &str) -> Result<Rc<Raw>> {
        if let Some(raw) = self.raws.get(&(kind, key.to_owned())) {
            return Ok(raw.clone());
        }
        let mut statement = self.connection.prepare_cached(
            "SELECT CASE WHEN value.target = 0 THEN value.field ELSE value.field || ':' || target.key END,
                    value.value, value.clock_ms, value.clock_counter, device.device_id
             FROM sync_identity entity
             JOIN sync_value value ON value.entity = entity.entity
             LEFT JOIN sync_identity target ON target.entity = value.target
             JOIN sync_device device ON device.device = value.device
             WHERE entity.kind = ?1 AND entity.key = ?2",
        )?;
        let rows = statement.query_map(rusqlite::params![kind.as_str(), key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                SyncTimestamp {
                    wall_ms: row.get(2)?,
                    counter: row.get(3)?,
                    device: row.get(4)?,
                },
            ))
        })?;
        let mut raw = Raw::new();
        for row in rows {
            let (field, value, timestamp) = row?;
            raw.insert(
                field,
                Stored {
                    value: serde_json::from_str(&value)?,
                    timestamp,
                },
            );
        }
        let raw = Rc::new(raw);
        self.raws.insert((kind, key.to_owned()), raw.clone());
        Ok(raw)
    }

    /// The position key of an entity in the folder `folder`: the value of its
    /// field `folder:<folder>`, read alone, because an item can be in many
    /// folders.
    pub(crate) fn position(&self, kind: SyncKind, key: &str, folder: &str) -> Result<Option<String>> {
        let value = self
            .connection
            .prepare_cached(
                "SELECT value.value FROM sync_identity entity, sync_identity target
                 JOIN sync_value value ON value.field = 'folder' AND value.target = target.entity
                     AND value.entity = entity.entity
                 WHERE entity.kind = ?1 AND entity.key = ?2 AND target.kind = 'folder' AND target.key = ?3",
            )?
            .query_row(rusqlite::params![kind.as_str(), key, folder], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(value.and_then(|value| serde_json::from_str::<Value>(&value).ok()?.as_str().map(str::to_owned)))
    }

    /// The local ids of the entities whose synced values wait to be written
    /// into the library.
    pub(crate) fn waiting(&self) -> Result<roaring::RoaringBitmap> {
        let mut statement = self.connection.prepare_cached(
            "SELECT identity.local_id FROM sync_pending pending
             JOIN sync_identity identity ON identity.entity = pending.entity WHERE identity.local_id IS NOT NULL",
        )?;
        let ids = statement.query_map([], |row| row.get::<_, u32>(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    /// The entities whose values refer to `kind:key`, with the field that
    /// refers to it. With `until`, `tag:` values recorded after that wall
    /// time and counter are left out.
    pub(crate) fn referrers(
        &self,
        kind: SyncKind,
        key: &str,
        until: Option<(i64, i64)>,
    ) -> Result<Vec<(SyncKind, String, String)>> {
        let (until_ms, until_counter) = until.unwrap_or((i64::MAX, i64::MAX));
        // A `tag:` or `folder:` value names its tag or folder as its target,
        // which leads its key; every other reference is in `reference`.
        let mut statement = self.connection.prepare_cached(
            "SELECT entity.kind, entity.key,
                    CASE WHEN value.target = 0 THEN value.field ELSE value.field || ':' || referred.key END
             FROM sync_identity referred
             JOIN sync_value value ON value.field = ?3 AND value.target = referred.entity
             JOIN sync_identity entity ON entity.entity = value.entity
             WHERE referred.kind = ?1 AND referred.key = ?2
               AND (value.clock_ms, value.clock_counter) <= (?4, ?5)
             UNION ALL
             SELECT entity.kind, entity.key, value.field
             FROM sync_identity referred
             JOIN sync_value value ON value.reference = referred.entity
             JOIN sync_identity entity ON entity.entity = value.entity
             WHERE referred.kind = ?1 AND referred.key = ?2",
        )?;
        let field = match kind {
            SyncKind::Tag => "tag",
            SyncKind::Folder => "folder",
            _ => "",
        };
        let rows = statement.query_map(rusqlite::params![kind.as_str(), key, field, until_ms, until_counter], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut referrers = Vec::new();
        for row in rows {
            let (kind, key, field) = row?;
            if let Some(kind) = SyncKind::parse(&kind) {
                referrers.push((kind, key, field));
            }
        }
        Ok(referrers)
    }

    /// Whether the latest `deleted` record of the entity is `false`, or the
    /// entity is one that an undo brings back.
    fn alive(&mut self, kind: SyncKind, key: &str) -> Result<bool> {
        if self.revived.contains(&(kind, key.to_owned())) {
            return Ok(true);
        }
        Ok(self
            .raw(kind, key)?
            .get("deleted")
            .is_some_and(|stored| stored.value == Value::Bool(false)))
    }

    /// Whether the synced values show the entity. An item needs its `file`
    /// field, a collection needs a visible member, and a tag group is also
    /// shown while a visible tag is in it.
    pub(crate) fn visible(&mut self, kind: SyncKind, key: &str) -> Result<bool> {
        match kind {
            SyncKind::Item => Ok(self.alive(kind, key)?
                && self
                    .raw(kind, key)?
                    .get("file")
                    .is_some_and(|stored| !stored.value.is_null())),
            SyncKind::Collection => {
                Ok(self.alive(kind, key)? && !self.members(key, Some(1))?.is_empty())
            }
            SyncKind::TagGroup => {
                if self.alive(kind, key)? {
                    return Ok(true);
                }
                let visible_tag = if key.is_empty() {
                    self.connection
                        .prepare_cached(
                            "SELECT EXISTS(SELECT 1 FROM sync_identity tag
                             CROSS JOIN sync_value value ON value.entity = tag.entity
                             WHERE tag.kind = 'tag' AND instr(tag.key, ':') = 0
                               AND value.field = 'deleted' AND value.target = 0 AND value.value = 'false')",
                        )?
                        .query_row([], |row| row.get::<_, bool>(0))?
                } else {
                    self.connection
                        .prepare_cached(
                            "SELECT EXISTS(SELECT 1 FROM sync_identity tag
                             CROSS JOIN sync_value value ON value.entity = tag.entity
                             WHERE tag.kind = 'tag' AND tag.key > ?1 AND tag.key < ?2
                               AND value.field = 'deleted' AND value.target = 0 AND value.value = 'false')",
                        )?
                        .query_row([format!("{key}:"), format!("{key};")], |row| {
                            row.get::<_, bool>(0)
                        })?
                };
                Ok(visible_tag)
            }
            SyncKind::Tag | SyncKind::Folder => self.alive(kind, key),
        }
    }

    /// What can change without a record of the entity's own: whether it is
    /// shown; for a tag, the tag it counts as; for an item, the collection it
    /// is shown in; for a collection, its cover, which is its first member
    /// when the stored cover is not a visible member.
    pub(crate) fn state(&mut self, kind: SyncKind, key: &str) -> Result<(bool, Option<String>)> {
        let visible = self.visible(kind, key)?;
        let detail = match kind {
            SyncKind::Tag => self.resolve_tag(key)?,
            SyncKind::Item if visible => {
                let raw = self.raw(kind, key)?;
                self.owner(&raw)?
            }
            SyncKind::Collection if visible => self
                .view(kind, key)?
                .and_then(|view| view.fields.get("cover").and_then(Value::as_str).map(str::to_owned)),
            _ => None,
        };
        Ok((visible, detail))
    }

    /// The visible tag that a tag key counts as: itself when visible, or the
    /// tag it was renamed or merged into, followed while it is not visible.
    pub(crate) fn resolve_tag(&mut self, key: &str) -> Result<Option<String>> {
        let mut key = key.to_owned();
        for _ in 0..FORWARDING_STEPS {
            if self.visible(SyncKind::Tag, &key)? {
                return Ok(Some(key));
            }
            match self
                .raw(SyncKind::Tag, &key)?
                .get("renamed_to")
                .and_then(|stored| stored.value.as_str())
            {
                Some(next) => key = next.to_owned(),
                None => return Ok(None),
            }
        }
        Ok(None)
    }

    /// The visible members of a collection as (item key, position key), in
    /// order, at most `limit` of them.
    pub(crate) fn members(&mut self, collection: &str, limit: Option<usize>) -> Result<Vec<(String, String)>> {
        let mut members = Vec::new();
        for (kind, key, field) in self.referrers(SyncKind::Collection, collection, None)? {
            if kind != SyncKind::Item || field != "collection" || !self.visible(kind, &key)? {
                continue;
            }
            let raw = self.raw(kind, &key)?;
            let Some(value) = raw.get("collection").map(|stored| &stored.value) else {
                continue;
            };
            if value.get(0).and_then(Value::as_str) != Some(collection) {
                continue;
            }
            let position = value.get(1).and_then(Value::as_str).unwrap_or_default();
            members.push((key, position.to_owned()));
            if limit.is_some_and(|limit| members.len() >= limit) {
                break;
            }
        }
        members.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
        Ok(members)
    }

    /// The collection an item is shown in, when its `collection` field names
    /// a collection that is not deleted.
    fn owner(&mut self, raw: &Raw) -> Result<Option<String>> {
        let Some(collection) = raw
            .get("collection")
            .and_then(|stored| stored.value.get(0))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        Ok(self.alive(SyncKind::Collection, &collection)?.then_some(collection))
    }

    /// The fields of an item outside a collection, or of a collection, as
    /// the synced values show them. An item in a collection keeps these
    /// fields; they are used again when it leaves the collection.
    pub(crate) fn root_view(&mut self, kind: SyncKind, key: &str, view: &mut View) -> Result<()> {
        let raw = self.raw(kind, key)?;
        for field in ROOT_FIELDS {
            let value = raw
                .get(field)
                .map(|stored| stored.value.clone())
                .filter(|value| !value.is_null())
                .unwrap_or_else(|| root_default(field));
            view.fields.insert(field.to_owned(), value);
        }
        // For each visible tag, the latest of its own record and the records
        // of the keys that forward to it decides.
        let mut tags: HashMap<String, &Stored> = HashMap::new();
        for (field, stored) in raw.iter() {
            if let Some(tag) = field.strip_prefix("tag:") {
                if let Some(target) = self.resolve_tag(tag)? {
                    let entry = tags.entry(target).or_insert(stored);
                    if stored.timestamp > entry.timestamp {
                        *entry = stored;
                    }
                }
            } else if let Some(folder) = field.strip_prefix("folder:") {
                if !stored.value.is_null() && self.visible(SyncKind::Folder, folder)? {
                    view.folders.insert(folder.to_owned());
                }
            }
        }
        view.tags = tags
            .into_iter()
            .filter(|(_, stored)| stored.value == Value::Bool(true))
            .map(|(tag, _)| tag)
            .collect();
        Ok(())
    }

    /// The keys that forward to the visible tag `tag` in the record of one
    /// entity, among its `tag:` fields with the value `true`.
    pub(crate) fn forwarded_to(&mut self, kind: SyncKind, key: &str, tag: &str) -> Result<Vec<String>> {
        let raw = self.raw(kind, key)?;
        let mut keys = Vec::new();
        for (field, stored) in raw.iter() {
            if let Some(other) = field.strip_prefix("tag:") {
                if other != tag
                    && stored.value == Value::Bool(true)
                    && self.resolve_tag(other)?.as_deref() == Some(tag)
                {
                    keys.push(other.to_owned());
                }
            }
        }
        Ok(keys)
    }

    /// The state the synced values show for an entity, or `None` when they
    /// do not show it.
    pub(crate) fn view(&mut self, kind: SyncKind, key: &str) -> Result<Option<View>> {
        if !self.visible(kind, key)? {
            return Ok(None);
        }
        let raw = self.raw(kind, key)?;
        let text = |field: &str, default: Value| {
            raw.get(field)
                .map(|stored| stored.value.clone())
                .filter(|value| !value.is_null())
                .unwrap_or(default)
        };
        let mut view = View::default();
        match kind {
            SyncKind::Item => {
                for (field, default) in [("file", Value::Null), ("name", json!("")), ("notes", Value::Null)] {
                    view.fields.insert(field.to_owned(), text(field, default));
                }
                view.owner = self.owner(&raw)?;
                if view.owner.is_none() {
                    self.root_view(kind, key, &mut view)?;
                }
            }
            SyncKind::Collection => {
                for (field, default) in [("name", json!("")), ("notes", Value::Null)] {
                    view.fields.insert(field.to_owned(), text(field, default));
                }
                let members = self.members(key, None)?;
                let cover = raw
                    .get("cover")
                    .and_then(|stored| stored.value.as_str())
                    .filter(|cover| members.iter().any(|(member, _)| member == cover))
                    .map_or_else(|| members[0].0.clone(), str::to_owned);
                view.fields.insert("cover".into(), json!(cover));
                self.root_view(kind, key, &mut view)?;
            }
            SyncKind::Folder => {
                for (field, default) in [
                    ("name", json!("")),
                    ("icon", Value::Null),
                    ("color", Value::Null),
                    ("notes", Value::Null),
                    ("watch", Value::Null),
                ] {
                    view.fields.insert(field.to_owned(), text(field, default));
                }
                let mut auto_tags = BTreeSet::new();
                for tag in text("auto_tags", json!([])).as_array().into_iter().flatten() {
                    if let Some(tag) = tag.as_str() {
                        auto_tags.extend(self.resolve_tag(tag)?);
                    }
                }
                view.fields.insert("auto_tags".into(), json!(auto_tags));
                // The cover is shown while it is a visible item outside a
                // collection or a visible collection.
                let cover = text("cover", Value::Null);
                let shown = match (cover.get(0).and_then(Value::as_str), cover.get(1).and_then(Value::as_str)) {
                    (Some("collection"), Some(key)) => self.visible(SyncKind::Collection, key)?,
                    (Some("item"), Some(key)) => {
                        let item = self.raw(SyncKind::Item, key)?;
                        self.visible(SyncKind::Item, key)? && self.owner(&item)?.is_none()
                    }
                    _ => false,
                };
                view.fields.insert("cover".into(), if shown { cover } else { Value::Null });
                view.owner = self.folder_tree()?.place.get(key).and_then(|place| place.0.clone());
            }
            SyncKind::Tag | SyncKind::TagGroup => {}
        }
        Ok(Some(view))
    }

    /// The tree of visible folders. A folder whose parent is not visible is
    /// at the top level. When parents form a cycle, the folder in the cycle
    /// whose `place` has the latest timestamp is at the top level.
    pub(crate) fn folder_tree(&mut self) -> Result<Rc<FolderTree>> {
        if let Some(tree) = &self.tree {
            return Ok(tree.clone());
        }
        let mut statement = self.connection.prepare_cached(
            "SELECT folder.key, value.field, value.value, value.clock_ms, value.clock_counter,
                    device.device_id
             FROM sync_identity folder
             CROSS JOIN sync_value value ON value.entity = folder.entity
             JOIN sync_device device ON device.device = value.device
             WHERE folder.kind = 'folder' AND value.field IN ('deleted', 'place') AND value.target = 0",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                SyncTimestamp {
                    wall_ms: row.get(3)?,
                    counter: row.get(4)?,
                    device: row.get(5)?,
                },
            ))
        })?;
        let mut alive = BTreeSet::new();
        let mut places: HashMap<String, (Value, SyncTimestamp)> = HashMap::new();
        for row in rows {
            let (key, field, value, timestamp) = row?;
            let value: Value = serde_json::from_str(&value)?;
            if field == "deleted" {
                if value == Value::Bool(false) {
                    alive.insert(key);
                }
            } else {
                places.insert(key, (value, timestamp));
            }
        }
        let mut parent = HashMap::new();
        let mut position = HashMap::new();
        for key in &alive {
            let (value, _) = places.get(key).cloned().unwrap_or((Value::Null, SyncTimestamp::default()));
            let up = value
                .get(0)
                .and_then(Value::as_str)
                .filter(|up| alive.contains(*up))
                .map(str::to_owned);
            parent.insert(key.clone(), up);
            position.insert(
                key.clone(),
                value.get(1).and_then(Value::as_str).unwrap_or_default().to_owned(),
            );
        }
        // Cycles: walk up from every folder; a walk that comes back to a
        // folder of its own path found a cycle.
        let mut mark: HashMap<String, usize> = HashMap::new();
        for (walk, start) in alive.iter().enumerate() {
            let mut path = Vec::new();
            let mut current = Some(start.clone());
            while let Some(key) = current.clone() {
                if mark.contains_key(&key) {
                    break;
                }
                mark.insert(key.clone(), walk);
                path.push(key.clone());
                current = parent[&key].clone();
            }
            if let Some(key) = current {
                if mark[&key] == walk {
                    let cycle = &path[path.iter().position(|item| *item == key).unwrap_or(0)..];
                    let latest = cycle
                        .iter()
                        .max_by(|a, b| {
                            let stamp = |key: &String| places.get(key).map(|place| place.1.clone());
                            (stamp(a), *a).cmp(&(stamp(b), *b))
                        })
                        .cloned();
                    if let Some(latest) = latest {
                        parent.insert(latest, None);
                    }
                }
            }
        }
        let mut tree = FolderTree::default();
        for key in &alive {
            tree.children.entry(parent[key].clone()).or_default().push(key.clone());
            tree.place.insert(key.clone(), (parent[key].clone(), position[key].clone()));
        }
        for children in tree.children.values_mut() {
            children.sort_by(|a, b| (&position[a], a).cmp(&(&position[b], b)));
        }
        let tree = Rc::new(tree);
        self.tree = Some(tree.clone());
        Ok(tree)
    }

    /// Every folder key that the synced values hold, deleted or not.
    pub(crate) fn folder_keys(&self) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT key FROM sync_identity folder WHERE kind = 'folder'
                 AND EXISTS (SELECT 1 FROM sync_value WHERE entity = folder.entity)",
            )?;
        let keys = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(keys)
    }
}

impl Default for SyncTimestamp {
    fn default() -> Self {
        Self {
            wall_ms: i64::MIN,
            counter: 0,
            device: String::new(),
        }
    }
}
