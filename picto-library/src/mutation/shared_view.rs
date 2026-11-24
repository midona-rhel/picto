//! What a picture looks like according to its entries. `file_view` is the
//! only place that decides it; a merge calls it for every picture whose
//! entries changed and changes the library to match (`shared_merge.rs`).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::Lifecycle;

/// The time of an entry: the clock's milliseconds, a counter for entries in
/// the same millisecond, and the id of the computer that wrote it. Comparing
/// two times as tuples tells which is later; the computer id only decides
/// between two entries of exactly the same clock time.
pub type EntryTime = (i64, i64, String);

/// The record that an `add` entry carries: the picture as the computer that
/// wrote the entry had it at that moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub name: String,
    /// The notes, and the empty text for an item without notes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// The number of stars, and zero for an item without a rating.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rating: u8,
    pub status: Lifecycle,
    /// The whole names of the item's tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The stable keys of the folders that hold the item.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<String>,
    /// Whether the entry was written when the library started recording, for
    /// a picture it already had. Its tags then count as older than any
    /// deletion of those tags.
    #[serde(default, skip_serializing_if = "is_false")]
    pub start: bool,
}

impl FileRecord {
    /// Reads a record from the JSON of an `add` entry. A record that a newer
    /// version wrote can have fields or values this version doesn't know; the
    /// fields it can read are kept, an unknown status counts as Active, and
    /// the entry still counts as an add, so the picture isn't deleted.
    pub fn parse(text: &str) -> Self {
        let value = serde_json::from_str::<serde_json::Value>(text).unwrap_or_default();
        let text_of = |key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let list_of = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_array())
                .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
                .unwrap_or_default()
        };
        Self {
            name: text_of("name"),
            notes: text_of("notes"),
            rating: value.get("rating").and_then(|v| v.as_u64()).unwrap_or(0).min(5) as u8,
            status: match text_of("status").as_str() {
                "inbox" => Lifecycle::Inbox,
                "trash" => Lifecycle::Trash,
                _ => Lifecycle::Active,
            },
            tags: list_of("tags"),
            folders: list_of("folders"),
            start: value.get("start").and_then(|v| v.as_bool()).unwrap_or(false),
        }
    }
}

impl Default for FileRecord {
    fn default() -> Self {
        Self {
            name: String::new(),
            notes: String::new(),
            rating: 0,
            status: Lifecycle::Active,
            tags: Vec::new(),
            folders: Vec::new(),
            start: false,
        }
    }
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Writes `time` as text whose order is the order of the times: the
/// milliseconds and the counter with leading zeros, and the computer id,
/// separated by colons. `compacted` entries and undo records hold times this
/// way, so that SQL can compare them as text.
pub fn time_text(time: &EntryTime) -> String {
    format!("{:016}:{:010}:{}", time.0, time.1, time.2)
}

/// Reads a time that `time_text` wrote. The empty text, and text that isn't
/// such a time, is the zero time.
pub fn parse_time_text(text: &str) -> EntryTime {
    let mut parts = text.splitn(3, ':');
    match (
        parts.next().and_then(|ms| ms.parse().ok()),
        parts.next().and_then(|counter| counter.parse().ok()),
        parts.next(),
    ) {
        (Some(ms), Some(counter), Some(device)) => (ms, counter, device.to_owned()),
        _ => zero(),
    }
}

/// The value of a `delete` entry that undoes a deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoRecord {
    /// The cut that the undoing computer had just before it deleted the
    /// picture, as `time_text` writes it, and empty for none. Entries later
    /// than it count again.
    pub before: String,
    /// The picture as the undoing computer had it before the deletion.
    pub record: FileRecord,
}

impl UndoRecord {
    /// Reads the value of a `delete` entry, which is empty for a deletion
    /// and an `UndoRecord` for an undo.
    pub fn parse(value: &str) -> Option<Self> {
        let value = serde_json::from_str::<serde_json::Value>(value).ok()?;
        let record = value.get("record")?;
        Some(Self {
            before: value.get("before").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
            record: FileRecord::parse(&record.to_string()),
        })
    }
}

/// One stored entry of a picture, as `file_view` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldEntry {
    Add { time: EntryTime, record: FileRecord },
    /// A deletion, or with `undo` the undo of one.
    Delete { time: EntryTime, undo: Option<UndoRecord> },
    /// The `compacted` entry, stored at the time `stored`: no entry of the
    /// picture at or before `through` counts.
    Compacted { stored: EntryTime, through: EntryTime },
    Name { time: EntryTime, value: String },
    Notes { time: EntryTime, value: String },
    Rating { time: EntryTime, value: u8 },
    Status { time: EntryTime, value: Lifecycle },
    /// `on` is `true` for `add` and `false` for `remove`.
    ItemTag { time: EntryTime, tag: String, on: bool },
    FolderItem { time: EntryTime, folder: String, on: bool },
}

/// A picture as its entries describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileView {
    pub name: String,
    pub notes: String,
    pub rating: u8,
    pub status: Lifecycle,
    pub tags: BTreeSet<String>,
    pub folders: BTreeSet<String>,
    /// The record of the earliest `add` entry that counts, or of the undo
    /// when no add counts (`file_view`).
    pub earliest: FileRecord,
}

/// A time that is earlier than every time a clock returns.
fn zero() -> EntryTime {
    (0, 0, String::new())
}

/// The cut of a file and its floor. An entry of the file counts only when
/// it's later than the cut. The floor is the time in its `compacted` entry:
/// entries at or before it were dropped and never count again, so the cut is
/// never earlier than the floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    pub cut: EntryTime,
    pub floor: EntryTime,
}

/// Returns the cut of the file whose entries are `entries`. After a
/// deletion, the cut is the time of the deletion. After the undo of a
/// deletion, it's the cut that the undoing computer had just before it
/// deleted the picture, so the entries that the deletion had made obsolete
/// count again. Without either, it's the zero time.
pub fn cut_of(entries: &[HeldEntry]) -> Cut {
    let mut cut = zero();
    let mut floor = zero();
    for entry in entries {
        match entry {
            HeldEntry::Delete { time, undo: None } => cut = time.clone(),
            HeldEntry::Delete { undo: Some(undo), .. } => cut = parse_time_text(&undo.before),
            HeldEntry::Compacted { through, .. } => floor = through.clone(),
            _ => {}
        }
    }
    Cut { cut: cut.max(floor.clone()), floor }
}

/// Returns the picture that `entries` describe, or `None` when it doesn't
/// exist. `entries` are all stored entries of one file. `tag_deleted` returns
/// the time of a tag's `tag_delete` entry, and `folder_deleted` whether a
/// folder has a `folder_delete` entry.
///
/// Only entries later than the cut count (`cut_of`). A picture exists when
/// one of its `add` entries counts, or when its `delete` entry is an undo.
/// When no add counts, the record of the undo counts as an add at the time of
/// the undo; that's the case after a floor has ended the entries the undo
/// would have brought back. A field entry counts when it's later than the
/// cut. Without one, the name and status come from the earliest counting add,
/// and the notes and rating from the first counting add that has them.
///
/// A tag is on the picture when its latest counting `item_tag` entry says
/// `add`. When there's no such entry, the tag is on when a counting add has
/// it in its record. The tag still goes when it was deleted at or after the
/// time it was put on: the time of the `item_tag` entry, or the latest add
/// that has it, where an add written at the start of recording counts as time
/// zero. For example, a tag put on at 9:00 and deleted at 10:00 is off, and
/// one put on again at 10:05 is on. Folders follow the same rule, except that
/// a deleted folder never holds anything.
pub fn file_view(
    entries: &[HeldEntry],
    tag_deleted: &mut dyn FnMut(&str) -> Option<EntryTime>,
    folder_deleted: &mut dyn FnMut(&str) -> bool,
) -> Option<FileView> {
    let deleted = cut_of(entries).cut;
    let mut counting = entries
        .iter()
        .filter_map(|entry| match entry {
            HeldEntry::Add { time, record } if *time > deleted => Some((time, record)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if counting.is_empty() {
        counting.extend(entries.iter().filter_map(|entry| match entry {
            HeldEntry::Delete { time, undo: Some(undo) } => Some((time, &undo.record)),
            _ => None,
        }));
    }
    counting.sort_by(|left, right| left.0.cmp(right.0));
    let earliest = counting.first()?.1;

    let mut view = FileView {
        name: earliest.name.clone(),
        notes: counting
            .iter()
            .map(|(_, record)| &record.notes)
            .find(|notes| !notes.is_empty())
            .cloned()
            .unwrap_or_default(),
        rating: counting
            .iter()
            .map(|(_, record)| record.rating)
            .find(|rating| *rating != 0)
            .unwrap_or(0),
        status: earliest.status,
        tags: BTreeSet::new(),
        folders: BTreeSet::new(),
        earliest: earliest.clone(),
    };

    // For each tag in a counting add: the time it was put on, which is the
    // latest such add. An add from the start of recording counts as time
    // zero, so it never moves the time later.
    let mut record_tags = std::collections::BTreeMap::<&str, EntryTime>::new();
    let mut record_folders = BTreeSet::<&str>::new();
    for (time, record) in &counting {
        for tag in &record.tags {
            if record.start {
                record_tags.entry(tag).or_insert_with(zero);
            } else {
                record_tags.insert(tag, (*time).clone());
            }
        }
        record_folders.extend(record.folders.iter().map(String::as_str));
    }

    let mut decided_tags = BTreeSet::<&str>::new();
    let mut decided_folders = BTreeSet::<&str>::new();
    for entry in entries {
        match entry {
            HeldEntry::Name { time, value } if *time > deleted => view.name = value.clone(),
            HeldEntry::Notes { time, value } if *time > deleted => view.notes = value.clone(),
            HeldEntry::Rating { time, value } if *time > deleted => view.rating = *value,
            HeldEntry::Status { time, value } if *time > deleted => view.status = *value,
            HeldEntry::ItemTag { time, tag, on } if *time > deleted => {
                decided_tags.insert(tag);
                if *on && tag_deleted(tag).is_none_or(|at| at < *time) {
                    view.tags.insert(tag.clone());
                }
            }
            HeldEntry::FolderItem { time, folder, on } if *time > deleted => {
                decided_folders.insert(folder);
                if *on && !folder_deleted(folder) {
                    view.folders.insert(folder.clone());
                }
            }
            _ => {}
        }
    }
    for (tag, member_since) in record_tags {
        if !decided_tags.contains(tag) && tag_deleted(tag).is_none_or(|at| at < member_since) {
            view.tags.insert(tag.to_owned());
        }
    }
    for folder in record_folders {
        if !decided_folders.contains(folder) && !folder_deleted(folder) {
            view.folders.insert(folder.to_owned());
        }
    }
    Some(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: i64, device: &str) -> EntryTime {
        (ms, 0, device.to_owned())
    }

    fn record(name: &str, tags: &[&str]) -> FileRecord {
        FileRecord {
            name: name.into(),
            status: Lifecycle::Active,
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            ..FileRecord::default()
        }
    }

    fn view(entries: &[HeldEntry], deleted_tags: &[(&str, i64)]) -> Option<FileView> {
        file_view(
            entries,
            &mut |tag| {
                deleted_tags
                    .iter()
                    .find(|(name, _)| *name == tag)
                    .map(|(_, ms)| at(*ms, "z"))
            },
            &mut |folder| folder == "deleted-folder",
        )
    }

    #[test]
    fn a_file_exists_when_an_add_is_later_than_the_delete() {
        let add = HeldEntry::Add { time: at(5, "a"), record: record("one", &[]) };
        assert!(view(std::slice::from_ref(&add), &[]).is_some());
        assert!(view(&[add.clone(), HeldEntry::Delete { time: at(6, "b"), undo: None }], &[]).is_none());
        assert!(view(&[add, HeldEntry::Delete { time: at(4, "b"), undo: None }], &[]).is_some());
        assert!(view(&[HeldEntry::Name { time: at(9, "a"), value: "x".into() }], &[]).is_none());
    }

    #[test]
    fn the_earliest_counting_add_gives_the_name_and_the_first_filled_value_the_notes_and_rating() {
        let early = HeldEntry::Add { time: at(5, "a"), record: record("early", &[]) };
        let late = HeldEntry::Add {
            time: at(7, "b"),
            record: FileRecord { notes: "note".into(), rating: 4, ..record("late", &[]) },
        };
        let shown = view(&[late.clone(), early.clone()], &[]).unwrap();
        assert_eq!((shown.name.as_str(), shown.notes.as_str(), shown.rating), ("early", "note", 4));
        // After a deletion at 6 only the add at 7 counts.
        let shown = view(&[late, early, HeldEntry::Delete { time: at(6, "a"), undo: None }], &[]).unwrap();
        assert_eq!(shown.name, "late");
    }

    #[test]
    fn an_entry_of_a_field_counts_only_when_it_is_later_than_the_delete() {
        let entries = [
            HeldEntry::Add { time: at(8, "a"), record: record("again", &["kept"]) },
            HeldEntry::Delete { time: at(6, "a"), undo: None },
            HeldEntry::Name { time: at(5, "a"), value: "before".into() },
            HeldEntry::Rating { time: at(9, "a"), value: 3 },
            HeldEntry::ItemTag { time: at(5, "a"), tag: "kept".into(), on: false },
            HeldEntry::ItemTag { time: at(9, "a"), tag: "new".into(), on: true },
        ];
        let shown = view(&entries, &[]).unwrap();
        assert_eq!(shown.name, "again");
        assert_eq!(shown.rating, 3);
        assert_eq!(shown.tags, BTreeSet::from(["kept".to_owned(), "new".to_owned()]));
    }

    #[test]
    fn a_deleted_tag_leaves_only_the_memberships_that_are_later_than_its_deletion() {
        let entries = [
            HeldEntry::Add { time: at(5, "a"), record: record("one", &["old", "both"]) },
            HeldEntry::Add {
                time: at(9, "b"),
                record: FileRecord { start: true, ..record("one", &["start", "both"]) },
            },
            HeldEntry::ItemTag { time: at(12, "a"), tag: "tagged".into(), on: true },
        ];
        let deleted = [("old", 5), ("both", 4), ("start", 1), ("tagged", 12)];
        let shown = view(&entries, &deleted).unwrap();
        // `old` was deleted at the time of its add. `both` is in the record
        // of the add at 5, which is later than the deletion at 4. `start` is
        // only in a record of the start of recording. `tagged` was deleted
        // at the time of its entry.
        assert_eq!(shown.tags, BTreeSet::from(["both".to_owned()]));
        assert_eq!(view(&entries, &[("tagged", 11)]).unwrap().tags.len(), 4);
    }

    #[test]
    fn a_deleted_folder_holds_nothing() {
        let entries = [
            HeldEntry::Add {
                time: at(5, "a"),
                record: FileRecord {
                    folders: vec!["deleted-folder".into(), "kept".into()],
                    ..record("one", &[])
                },
            },
            HeldEntry::FolderItem { time: at(9, "a"), folder: "deleted-folder".into(), on: true },
            HeldEntry::FolderItem { time: at(9, "a"), folder: "kept".into(), on: false },
            HeldEntry::FolderItem { time: at(9, "a"), folder: "other".into(), on: true },
        ];
        assert_eq!(view(&entries, &[]).unwrap().folders, BTreeSet::from(["other".to_owned()]));
    }

    fn undo(at_ms: i64, before: Option<i64>, name: &str) -> HeldEntry {
        HeldEntry::Delete {
            time: at(at_ms, "b"),
            undo: Some(UndoRecord {
                before: before.map(|ms| time_text(&at(ms, "z"))).unwrap_or_default(),
                record: record(name, &["kept"]),
            }),
        }
    }

    #[test]
    fn a_time_is_read_back_as_it_was_written_and_its_text_sorts_like_the_time() {
        let times = [at(5, "a"), at(5, "b"), (5, 1, "a".to_owned()), at(40, "a"), at(1_700_000_000_000, "a")];
        for pair in times.windows(2) {
            assert!(time_text(&pair[0]) < time_text(&pair[1]));
        }
        for time in times {
            assert_eq!(parse_time_text(&time_text(&time)), time);
        }
        assert_eq!(parse_time_text(""), zero());
    }

    /// The undo of a deletion at 6 brings back what the entries said before
    /// it, including the rating at 3 that the undoing computer hadn't seen.
    #[test]
    fn an_undo_brings_back_the_entries_that_the_deletion_had_made_obsolete() {
        let entries = [
            HeldEntry::Add { time: at(1, "a"), record: record("first", &[]) },
            HeldEntry::Rating { time: at(3, "a"), value: 5 },
            undo(8, None, "as the undoing computer had it"),
        ];
        let shown = view(&entries, &[]).unwrap();
        assert_eq!((shown.name.as_str(), shown.rating), ("first", 5));

        // The cut goes back only to the one before the deletion.
        let entries = [
            HeldEntry::Add { time: at(1, "a"), record: record("first", &[]) },
            HeldEntry::Rating { time: at(3, "a"), value: 5 },
            HeldEntry::Add { time: at(5, "c"), record: record("imported again", &[]) },
            undo(8, Some(4), "as the undoing computer had it"),
        ];
        let shown = view(&entries, &[]).unwrap();
        assert_eq!((shown.name.as_str(), shown.rating), ("imported again", 0));
    }

    /// After a floor, the entries before it don't count, and the record of the
    /// undo is the picture.
    #[test]
    fn behind_a_floor_the_undo_brings_back_its_own_record() {
        let entries = [
            HeldEntry::Rating { time: at(3, "a"), value: 5 },
            undo(8, None, "as the undoing computer had it"),
            HeldEntry::Compacted { stored: at(20, "b"), through: at(6, "b") },
        ];
        assert_eq!(cut_of(&entries), Cut { cut: at(6, "b"), floor: at(6, "b") });
        let shown = view(&entries, &[]).unwrap();
        assert_eq!((shown.name.as_str(), shown.rating), ("as the undoing computer had it", 0));
        assert_eq!(shown.tags, BTreeSet::from(["kept".to_owned()]));
        // A floor earlier than the deletion leaves a deleted picture deleted.
        let entries = [
            HeldEntry::Add { time: at(1, "a"), record: record("first", &[]) },
            HeldEntry::Delete { time: at(6, "b"), undo: None },
            HeldEntry::Compacted { stored: at(20, "b"), through: at(2, "b") },
        ];
        assert_eq!(cut_of(&entries).cut, at(6, "b"));
        assert!(view(&entries, &[]).is_none());
    }
}
