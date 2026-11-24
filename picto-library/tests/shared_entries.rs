//! Randomized test of cloud sync between three computers, for pictures that
//! aren't in a collection.
//!
//! Each run makes three real libraries that stand for three computers. They
//! import, edit, tag, file into folders, delete, undo and redo, rename, merge
//! and delete tags, delete folders, and start recording at random moments.
//! In between, random sets of one computer's entries reach another, split
//! into files as the cloud folder holds them, in any order, some twice and
//! some files not readable yet. The receiving computer sometimes makes changes
//! of its own between storing the files and applying them. A computer can be
//! offline for many rounds: it sends and receives nothing and goes on making
//! changes. In half of the runs the computers compact their entries at random
//! moments as if 8 days had passed, so every deletion that is in the cloud
//! folder is old enough. "Keep deleted files" is 0 days, and the cleanup
//! worker removes queued files at random moments. At the end every entry is
//! delivered. Files aren't fetched, as in the app today. A computer gets a
//! picture's file when it imports the file itself, and it keeps the file of a
//! picture that a merge deleted, which brings the picture back when the
//! entries add it again.
//!
//! The test logs what each computer did and when, by that computer's clock,
//! and then checks:
//!
//! - The entries say the same on all three computers, and every picture a
//!   computer holds looks the way its entries say.
//! - What compaction dropped changes nothing: a new library that merges every
//!   entry any computer ever sent says the same as the three.
//! - The undo of a deletion brings back what any computer had done before the
//!   deletion, unless a `compacted` entry ended it: for each field, tag and
//!   folder, the latest change counts when it's earlier than the undone
//!   deletion, later than the floor, and no other deletion lies between.
//! - Nothing is lost that no action removed. A picture nobody deleted exists
//!   by the entries and at least one computer still holds its file, and a tag
//!   nobody took off is still on it. A computer that held a picture and
//!   didn't delete it itself holds it whenever its entries say it exists.
//! - The later action wins, for deletion against import and for each field.
//! - An explicit edit isn't undone by an import or by the start of recording
//!   on another computer, and a deleted tag doesn't come back from a computer
//!   that starts recording later.
//!
//! Each run has a fixed seed that a failure names, and
//! `SHARED_ENTRIES_SEED=<seed>` replays it. Clock times come from the real
//! clock, so a replay makes the same actions but can get other times.

use std::collections::{BTreeMap, BTreeSet};

use picto_library::database::WorkPriority;
use picto_library::selection::SelectionTarget;
use picto_library::{
    FolderId, ImmutableMediaFacts, Library, Lifecycle, PreparedCollectionImport, PreparedImport,
    Rating, RootId, SharedEntry, TagId,
};
use tempfile::TempDir;

const HASHES: u64 = 5;
const TAGS: [&str; 5] = ["red", "green", "blue", "char:red", "person:red"];
/// The tags without a namespace, which the tag rename and merge use.
const PLAIN_TAGS: [&str; 3] = ["red", "green", "blue"];
/// The namespaces of `TAGS`, which the namespace rename and merge use.
const NAMESPACES: [&str; 2] = ["char", "person"];
const FOLDERS: [&str; 2] = ["5d3c0b52-0000-4000-8000-000000000001", "5d3c0b52-0000-4000-8000-000000000002"];
const RUNS_PER_TEST: u64 = 250;

type Time = (i64, i64, String);

/// A small random number generator, so that a seed gives the same run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn below(&mut self, limit: u64) -> u64 {
        self.next() % limit
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

fn hash(number: u64) -> String {
    format!("{number:064x}")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// What a computer did, as the test logs it.
#[derive(Debug, Clone)]
enum Action {
    /// The computer wrote an `add` entry: an import of a new file, or a file
    /// it held when it started recording.
    Add { hash: String, time: Time, start: bool, tags: BTreeSet<String>, folders: BTreeSet<String> },
    Delete { hash: String, time: Time },
    /// The undo of the deletion at `deleted`, with the picture's tags and
    /// folders as the undo brought it back.
    Undo { hash: String, time: Time, deleted: Time, tags: BTreeSet<String>, folders: BTreeSet<String> },
    Edit { hash: String, field: Field, value: String, time: Time },
    Tag { hash: String, tag: String, on: bool, time: Time },
    File { hash: String, folder: String, on: bool, time: Time },
    DeleteTag { tag: String, time: Time },
    DeleteFolder { folder: String, time: Time },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Notes,
    Rating,
    Status,
}

/// A picture as a library shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Picture {
    name: String,
    notes: String,
    rating: String,
    status: String,
    tags: BTreeSet<String>,
    folders: BTreeSet<String>,
}

impl Picture {
    fn field(&self, field: Field) -> &str {
        match field {
            Field::Name => &self.name,
            Field::Notes => &self.notes,
            Field::Rating => &self.rating,
            Field::Status => &self.status,
        }
    }
}

fn status_name(lifecycle: Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Active => "active",
        Lifecycle::Inbox => "inbox",
        Lifecycle::Trash => "trash",
    }
}

fn rating_of(stars: u64) -> Rating {
    Rating::ALL[stars as usize]
}

struct Computer {
    library: Library,
    _directory: TempDir,
    id: String,
    recording: bool,
    imports: u64,
    /// The picture this computer deleted with its last action, and the time
    /// of the deletion, which an undo brings back.
    deleted_last: Option<(String, Time)>,
    /// The picture whose deletion the last action undid, which a redo
    /// deletes again.
    undone_last: Option<String>,
    /// The step of the run up to which this computer sends and receives
    /// nothing.
    offline_until: u64,
    /// The pictures this computer has held, without those that an action on
    /// this computer deleted afterwards.
    held_files: BTreeSet<String>,
}

impl Computer {
    fn new(template: &std::path::Path) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        std::fs::copy(template, &path).unwrap();
        let id = uuid_like();
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection
                .execute("UPDATE cloud_state SET device_id = ?1", [&id])
                .unwrap();
        }
        let library = Library::open(&path).unwrap();
        for (index, key) in FOLDERS.iter().enumerate() {
            library
                .create_folder_with_stable_key(&format!("Folder {index}"), None, (*key).to_owned())
                .unwrap();
        }
        Self {
            library,
            _directory: directory,
            id,
            recording: false,
            imports: 0,
            deleted_last: None,
            undone_last: None,
            offline_until: 0,
            held_files: BTreeSet::new(),
        }
    }

    /// Adds the pictures this computer holds now to `held_files`.
    fn note_held(&mut self) {
        let roots = self.roots();
        self.held_files.extend(roots.into_keys());
    }

    fn clock(&self) -> (i64, i64) {
        self.library.entry_clock().unwrap().now
    }

    /// Returns the time of the action that moved the clock from `before`,
    /// or `None` when the action recorded nothing.
    fn action_time(&self, before: (i64, i64)) -> Option<Time> {
        let now = self.clock();
        (self.recording && now != before).then(|| (now.0, now.1, self.id.clone()))
    }

    fn rows<T>(
        &self,
        sql: &str,
        map: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> Vec<T> {
        self.library
            .database()
            .read(WorkPriority::VisibleRead, |connection| {
                let mut statement = connection.prepare(sql)?;
                let rows = statement.query_map([], |row| map(row))?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    /// The roots of the pictures outside collections, by content hash.
    fn roots(&self) -> BTreeMap<String, RootId> {
        self.rows(
            "SELECT file.content_hash, root.root_id FROM library_root root
             JOIN media_item media ON media.media_id = root.root_id
             JOIN media_file file ON file.file_id = media.file_id",
            |row| Ok((row.get::<_, String>(0)?, RootId(row.get(1)?))),
        )
        .into_iter()
        .collect()
    }

    fn tag_ids(&self) -> BTreeMap<String, TagId> {
        self.rows(
            "SELECT tag.tag_id, namespace.display_name, tag.subname FROM tag_definition tag
             JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id",
            |row| {
                let namespace: String = row.get(1)?;
                let subname: String = row.get(2)?;
                let name = if namespace.is_empty() { subname } else { format!("{namespace}:{subname}") };
                Ok((name, TagId(row.get(0)?)))
            },
        )
        .into_iter()
        .collect()
    }

    fn folder_ids(&self) -> BTreeMap<String, FolderId> {
        self.rows("SELECT stable_key, folder_id FROM folder_definition", |row| {
            Ok((row.get::<_, String>(0)?, FolderId(row.get(1)?)))
        })
        .into_iter()
        .collect()
    }

    fn pictures(&self) -> BTreeMap<String, Picture> {
        let tags = self
            .tag_ids()
            .into_iter()
            .map(|(name, id)| (id, name))
            .collect::<BTreeMap<_, _>>();
        let folders = self
            .folder_ids()
            .into_iter()
            .map(|(key, id)| (id, key))
            .collect::<BTreeMap<_, _>>();
        let texts = self
            .rows(
                "SELECT root.root_id, root.name, COALESCE(root.notes, '') FROM library_root root",
                |row| Ok((row.get::<_, u32>(0)?, (row.get::<_, String>(1)?, row.get::<_, String>(2)?))),
            )
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        self.roots()
            .into_iter()
            .map(|(hash, root)| {
                let details = self.library.details(root).unwrap();
                let (name, notes) = texts[&root.0].clone();
                let picture = Picture {
                    name,
                    notes,
                    rating: details.rating.bitmap_key().to_string(),
                    status: status_name(details.lifecycle).to_owned(),
                    tags: details.tag_ids.iter().map(|id| tags[id].clone()).collect(),
                    folders: details.folder_ids.iter().map(|id| folders[id].clone()).collect(),
                };
                (hash, picture)
            })
            .collect()
    }

    fn target(&self, root: RootId) -> SelectionTarget {
        SelectionTarget::Explicit { root_ids: vec![root] }
    }

    fn import(&mut self, hash: &str, name: &str, tags: Vec<String>, folders: Vec<FolderId>) {
        let input = self.prepared(hash, name, tags, folders);
        self.library.ingest(&input).unwrap();
    }

    fn prepared(
        &mut self,
        hash: &str,
        name: &str,
        tags: Vec<String>,
        folders: Vec<FolderId>,
    ) -> PreparedImport {
        self.imports += 1;
        let key = format!("{}-{}", self.id, self.imports);
        PreparedImport {
            stable_key: key.clone(),
            media_name: name.to_owned(),
            file_path: format!("/picto-test/{key}.png"),
            facts: ImmutableMediaFacts {
                mime: "image/png".into(),
                size_bytes: 1_024,
                width: Some(10),
                height: Some(10),
                duration_ms: None,
                frame_count: Some(1),
                content_hash: hash.to_owned(),
                perceptual_hash: None,
                palette: Vec::new(),
            },
            lifecycle: Lifecycle::Active,
            rating: Rating::Unrated,
            notes: None,
            tags,
            folders,
            source_urls: Vec::new(),
            source_identity: None,
            imported_at_ms: now_ms(),
            captured_at_ms: None,
        }
    }

    fn settle(&self) {
        while self.library.settle_entries(3).unwrap() {}
    }

    fn own_entries(&self) -> Vec<SharedEntry> {
        self.settle();
        let mut entries = Vec::new();
        self.library
            .visit_own_entries(None, &mut |entry| entries.push(entry))
            .unwrap();
        entries
    }

    /// Returns all of this computer's entries and records them as written to
    /// the cloud folder, as a sync does.
    fn publish(&self) -> Vec<SharedEntry> {
        let entries = self.own_entries();
        let now = self.clock();
        self.library.set_entries_published(now).unwrap();
        entries
    }

    /// Deletes the picture `hash` permanently, from the Trash.
    fn delete(&self, hash: &str) {
        let target = self.target(self.roots()[hash]);
        self.library.set_lifecycle(&target, Lifecycle::Trash).unwrap();
        self.library.permanently_delete(&target, now_ms(), 500, false).unwrap();
    }

    /// The kinds of the entries of `hash` that this computer stores.
    fn entry_kinds(&self, hash: &str) -> Vec<String> {
        self.rows(
            &format!(
                "SELECT kind || CASE WHEN second_key = '' THEN '' ELSE ' ' || second_key END
                 FROM shared_entry WHERE subject_key = '{hash}' ORDER BY 1"
            ),
            |row| row.get::<_, String>(0),
        )
    }

    /// Starts recording and logs an `add` for every picture held now.
    fn start(&mut self, log: &mut Vec<Action>) {
        let before = self.clock();
        assert!(self.library.start_recording().unwrap());
        self.recording = true;
        self.deleted_last = None;
        self.undone_last = None;
        let time = self.action_time(before).unwrap();
        for (hash, picture) in self.pictures() {
            log.push(Action::Add {
                hash,
                time: time.clone(),
                start: true,
                tags: picture.tags,
                folders: picture.folders,
            });
        }
    }

    /// The pictures as this computer's entries describe them.
    fn views(&self) -> BTreeMap<String, Picture> {
        (1..=HASHES)
            .filter_map(|number| {
                let view = self.library.entry_view(&hash(number)).unwrap()?;
                Some((
                    hash(number),
                    Picture {
                        name: view.name,
                        notes: view.notes,
                        rating: view.rating.to_string(),
                        status: status_name(view.status).to_owned(),
                        tags: view.tags,
                        folders: view.folders,
                    },
                ))
            })
            .collect()
    }

    /// Imports the files that the entries say exist and that this computer
    /// doesn't hold, the way a sync will fetch them.
    fn fetch_wanted(&mut self) {
        for wanted in self.library.wanted_files(None, 1_000).unwrap() {
            self.import(&wanted.content_hash, &wanted.record.name, Vec::new(), Vec::new());
        }
    }
}

fn uuid_like() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{:08x}-0000-4000-8000-{:012x}", now_ms() as u32, number)
}

/// Does one random action on `computer` and logs it when it recorded
/// something.
fn act(random: &mut Random, computer: &mut Computer, log: &mut Vec<Action>) {
    computer.note_held();
    let roots = computer.roots();
    let held = roots.keys().cloned().collect::<Vec<_>>();
    let choice = random.below(115);
    let undo = computer.deleted_last.take();
    let redo = computer.undone_last.take();
    if let (Some(hash), true) = (redo, choice < 10) {
        let before = computer.clock();
        computer.library.redo().unwrap();
        if !computer.roots().contains_key(&hash) {
            computer.held_files.remove(&hash);
        }
        if let Some(time) = computer.action_time(before) {
            computer.deleted_last = Some((hash.clone(), time.clone()));
            log.push(Action::Delete { hash, time });
        }
        return;
    }
    if let (Some((hash, deleted)), true) = (undo, choice < 10) {
        let before = computer.clock();
        computer.library.undo().unwrap();
        if let Some(time) = computer.action_time(before) {
            computer.undone_last = Some(hash.clone());
            let picture = computer.pictures().remove(&hash).unwrap();
            log.push(Action::Undo {
                hash,
                time,
                deleted,
                tags: picture.tags,
                folders: picture.folders,
            });
        }
        return;
    }
    if held.is_empty() || choice < 30 {
        let hash = hash(random.below(HASHES) + 1);
        if roots.contains_key(&hash) {
            return;
        }
        let exists = computer.library.entry_view(&hash).unwrap().is_some();
        let name = format!("name-{}", random.below(1_000));
        let tags = TAGS.iter().filter(|_| random.chance(40)).map(|tag| (*tag).to_owned()).collect::<Vec<_>>();
        let folder_ids = computer.folder_ids();
        let folders = folder_ids
            .iter()
            .filter(|_| random.chance(30))
            .map(|(key, id)| (key.clone(), *id))
            .collect::<Vec<_>>();
        let before = computer.clock();
        computer.import(&hash, &name, tags.clone(), folders.iter().map(|(_, id)| *id).collect());
        // An import of a file that already exists by the entries writes
        // nothing; it only brings the file to this computer.
        if let (Some(time), false) = (computer.action_time(before), exists) {
            log.push(Action::Add {
                hash,
                time,
                start: false,
                tags: tags.into_iter().collect(),
                folders: folders.into_iter().map(|(key, _)| key).collect(),
            });
        }
        return;
    }
    let hash = random.pick(&held).clone();
    let root = roots[&hash];
    let target = computer.target(root);
    let before = computer.clock();
    let logged = match choice {
        30..=37 => {
            // Delete Permanently, after moving the picture to the Trash.
            computer.library.set_lifecycle(&target, Lifecycle::Trash).unwrap();
            if let Some(time) = computer.action_time(before) {
                log.push(Action::Edit { hash: hash.clone(), field: Field::Status, value: "trash".into(), time });
            }
            let before = computer.clock();
            computer.library.permanently_delete(&target, now_ms(), 500, false).unwrap();
            computer.held_files.remove(&hash);
            let time = computer.action_time(before);
            // Only an undo of a recorded deletion is an action of the rules.
            computer.deleted_last = time.clone().map(|time| (hash.clone(), time));
            time.map(|time| Action::Delete { hash: hash.clone(), time })
        }
        38..=45 => {
            let name = format!("renamed-{}", random.below(1_000));
            computer.library.rename_root(root, &name, now_ms()).unwrap();
            computer
                .action_time(before)
                .map(|time| Action::Edit { hash: hash.clone(), field: Field::Name, value: name, time })
        }
        46..=50 => {
            let notes = format!("notes-{}", random.below(1_000));
            computer.library.set_notes(&target, Some(notes.clone()), now_ms()).unwrap();
            computer
                .action_time(before)
                .map(|time| Action::Edit { hash: hash.clone(), field: Field::Notes, value: notes, time })
        }
        51..=57 => {
            let stars = random.below(6);
            computer.library.set_rating(&target, rating_of(stars)).unwrap();
            computer.action_time(before).map(|time| Action::Edit {
                hash: hash.clone(),
                field: Field::Rating,
                value: stars.to_string(),
                time,
            })
        }
        58..=63 => {
            let lifecycle = *random.pick(&Lifecycle::ALL);
            computer.library.set_lifecycle(&target, lifecycle).unwrap();
            computer.action_time(before).map(|time| Action::Edit {
                hash: hash.clone(),
                field: Field::Status,
                value: status_name(lifecycle).into(),
                time,
            })
        }
        64..=79 => {
            let tag = (*random.pick(&TAGS)).to_owned();
            let on = random.chance(60);
            computer.library.apply_tags(&target, std::slice::from_ref(&tag), on).unwrap();
            computer
                .action_time(before)
                .map(|time| Action::Tag { hash: hash.clone(), tag, on, time })
        }
        80..=91 => {
            let folders = computer.folder_ids();
            let Some((folder, folder_id)) =
                folders.iter().nth(random.below(FOLDERS.len() as u64) as usize)
            else {
                return;
            };
            let on = random.chance(60);
            if on {
                computer.library.add_to_folder(&target, *folder_id).unwrap();
            } else {
                computer.library.remove_from_folder(&target, *folder_id).unwrap();
            }
            computer
                .action_time(before)
                .map(|time| Action::File { hash: hash.clone(), folder: folder.clone(), on, time })
        }
        92..=96 => {
            let tag = (*random.pick(&PLAIN_TAGS)).to_owned();
            let ids = computer.tag_ids();
            let Some(tag_id) = ids.get(&tag).copied() else {
                return;
            };
            let unused = PLAIN_TAGS.iter().find(|name| !ids.contains_key(**name));
            let other = PLAIN_TAGS.iter().find(|name| **name != tag && ids.contains_key(**name));
            match (random.below(3), unused, other) {
                (1, Some(new), _) => {
                    // Renaming a tag deletes the old name and puts the new
                    // name on the pictures that had it.
                    computer.library.rename_tag(tag_id, new).unwrap();
                    if let Some(time) = computer.action_time(before) {
                        log.push(Action::DeleteTag { tag, time: time.clone() });
                        for (hash, picture) in computer.pictures() {
                            if picture.tags.contains(*new) {
                                log.push(Action::Tag { hash, tag: (*new).to_owned(), on: true, time: time.clone() });
                            }
                        }
                    }
                    None
                }
                (2, _, Some(into)) => {
                    // Merging a tag into another moves it on the pictures
                    // that had it.
                    let had = computer
                        .pictures()
                        .into_iter()
                        .filter(|(_, picture)| picture.tags.contains(&tag))
                        .map(|(hash, picture)| (hash, picture.tags.contains(*into)))
                        .collect::<Vec<_>>();
                    computer.library.merge_tags(tag_id, ids[*into], now_ms()).unwrap();
                    if let Some(time) = computer.action_time(before) {
                        for (hash, already) in had {
                            log.push(Action::Tag { hash: hash.clone(), tag: tag.clone(), on: false, time: time.clone() });
                            // A picture that had both tags gets no new entry.
                            if !already {
                                log.push(Action::Tag { hash, tag: (*into).to_owned(), on: true, time: time.clone() });
                            }
                        }
                    }
                    None
                }
                _ => {
                    computer.library.delete_tag(tag_id, now_ms()).unwrap();
                    computer.action_time(before).map(|time| Action::DeleteTag { tag, time })
                }
            }
        }
        100..=104 => {
            // Renaming a tag group, or merging it into one that exists.
            let from = (*random.pick(&NAMESPACES)).to_owned();
            let into = NAMESPACES.iter().find(|name| **name != from).unwrap();
            let namespaces = computer.library.tag_namespaces().unwrap();
            let Some(namespace) = namespaces.iter().find(|namespace| namespace.name == from) else {
                return;
            };
            let ids = computer.tag_ids();
            let before_pictures = computer.pictures();
            computer
                .library
                .rename_or_merge_tag_namespace(namespace.namespace_id, into)
                .unwrap();
            if let Some(time) = computer.action_time(before) {
                for name in TAGS.iter().filter(|name| name.starts_with(&format!("{from}:"))) {
                    let moved = name.replacen(&from, into, 1);
                    // A tag that the other group has too is merged into it.
                    // Any other tag is renamed, which deletes the old name.
                    let merged = ids.contains_key(&moved);
                    if !ids.contains_key(*name) {
                        continue;
                    }
                    if !merged {
                        log.push(Action::DeleteTag { tag: (*name).to_owned(), time: time.clone() });
                    }
                    for (hash, picture) in &before_pictures {
                        if picture.tags.contains(*name) {
                            if merged {
                                log.push(Action::Tag { hash: hash.clone(), tag: (*name).to_owned(), on: false, time: time.clone() });
                            }
                            if !picture.tags.contains(&moved) {
                                log.push(Action::Tag { hash: hash.clone(), tag: moved.clone(), on: true, time: time.clone() });
                            }
                        }
                    }
                }
            }
            None
        }
        105..=109 => {
            // Merging a duplicate into another picture that stays.
            let Some(winner) = held.iter().find(|other| **other != hash).cloned() else {
                return;
            };
            let file_of = |hash: &str| computer.library.details(roots[hash]).unwrap().media[0].file_id;
            let (loser_file, winner_file) = (file_of(&hash), file_of(&winner));
            let pictures = computer.pictures();
            // A pair that was resolved before, and undone, isn't offered again.
            if computer.library.record_duplicate_pair(loser_file, winner_file, 1, now_ms()).unwrap().is_none() {
                return;
            }
            let before = computer.clock();
            computer
                .library
                .resolve_duplicate(
                    loser_file,
                    winner_file,
                    picto_library::DuplicateResolutionChoice::KeepFile { winner_file_id: winner_file },
                    now_ms(),
                )
                .unwrap();
            computer.held_files.remove(&hash);
            if let Some(time) = computer.action_time(before) {
                log.push(Action::Delete { hash: hash.clone(), time: time.clone() });
                let was = &pictures[&winner];
                let now = computer.pictures().remove(&winner).unwrap();
                log_differences(log, &winner, was, &now, &time);
            }
            None
        }
        110..=114 => {
            // Repairing a video replaces its file with one of another hash.
            let files = computer.rows("SELECT content_hash FROM media_file", |row| row.get::<_, String>(0));
            let Some(new_hash) = (1..=HASHES)
                .map(|number| format!("{number:064x}"))
                .find(|other| !files.contains(other))
            else {
                return;
            };
            let media_id = computer.library.details(root).unwrap().media[0].media_id;
            computer
                .library
                .repair_media_content(
                    media_id,
                    &picto_library::RepairedMediaContent {
                        expected_hash: hash.clone(),
                        content_hash: new_hash.clone(),
                        file_path: format!("/picto-test/repaired-{new_hash}.png"),
                        size_bytes: 2_048,
                    },
                    now_ms(),
                )
                .unwrap();
            computer.held_files.remove(&hash);
            if let Some(time) = computer.action_time(before) {
                let picture = computer.pictures().remove(&new_hash).unwrap();
                log.push(Action::Delete { hash: hash.clone(), time: time.clone() });
                log.push(Action::Add {
                    hash: new_hash,
                    time,
                    start: false,
                    tags: picture.tags,
                    folders: picture.folders,
                });
            }
            None
        }
        _ => {
            // Folder definitions don't sync, so a folder deleted before
            // recording starts would be missing on this computer only.
            if !computer.recording {
                return;
            }
            let folders = computer.folder_ids();
            let Some((folder, folder_id)) =
                folders.iter().nth(random.below(FOLDERS.len() as u64) as usize)
            else {
                return;
            };
            computer.library.delete_folders(&[*folder_id]).unwrap();
            computer
                .action_time(before)
                .map(|time| Action::DeleteFolder { folder: folder.clone(), time })
        }
    };
    if choice >= 38 {
        computer.deleted_last = None;
    }
    computer.undone_last = None;
    log.extend(logged);
}

/// Logs what changed on the picture `hash` from `was` to `now` as edits at
/// `time`.
fn log_differences(log: &mut Vec<Action>, hash: &str, was: &Picture, now: &Picture, time: &Time) {
    for field in [Field::Name, Field::Notes, Field::Rating, Field::Status] {
        if was.field(field) != now.field(field) {
            log.push(Action::Edit {
                hash: hash.to_owned(),
                field,
                value: now.field(field).to_owned(),
                time: time.clone(),
            });
        }
    }
    for tag in now.tags.difference(&was.tags) {
        log.push(Action::Tag { hash: hash.to_owned(), tag: tag.clone(), on: true, time: time.clone() });
    }
    for folder in now.folders.difference(&was.folders) {
        log.push(Action::File { hash: hash.to_owned(), folder: folder.clone(), on: true, time: time.clone() });
    }
}

/// Sends a random part of one computer's entries to another, in random order
/// and with some entries twice. The entries come in four files, as the cloud
/// folder spreads them, and some files can't be read in this round. Between
/// storing the files and applying them, the receiving computer sometimes
/// makes changes of its own, and afterwards its cleanup worker sometimes
/// runs.
fn deliver(
    random: &mut Random,
    computers: &mut [Computer],
    from: usize,
    to: usize,
    log: &mut Vec<Action>,
    sent: &mut Vec<SharedEntry>,
) {
    // All of the sender's entries are in the cloud folder; a random part of
    // them has reached the receiver.
    let published = computers[from].publish();
    sent.extend(published.iter().cloned());
    let mut entries = published
        .into_iter()
        .filter(|_| random.chance(50))
        .collect::<Vec<_>>();
    let repeated = entries.iter().filter(|_| random.chance(10)).cloned().collect::<Vec<_>>();
    entries.extend(repeated);
    for index in (1..entries.len()).rev() {
        entries.swap(index, random.below(index as u64 + 1) as usize);
    }
    let mut files = vec![Vec::new(); 4];
    for entry in entries {
        let key = format!("{:?}{}{}", entry.kind, entry.subject_key, entry.second_key);
        let file = key.bytes().map(usize::from).sum::<usize>() % files.len();
        files[file].push(entry);
    }
    let mut unread = false;
    for file in files {
        if random.chance(20) {
            unread = true;
            continue;
        }
        computers[to].library.store_entries(&file).unwrap();
        if random.chance(30) {
            act(random, &mut computers[to], log);
        }
    }
    computers[to].library.apply_stored(unread).unwrap();
    if random.chance(50) {
        clean_files(&computers[to]);
    }
}

fn run(seed: u64, template: &std::path::Path) {
    let mut random = Random(seed);
    let mut computers = (0..3).map(|_| Computer::new(template)).collect::<Vec<_>>();
    for computer in &computers {
        set_keep_days(computer, 0);
    }
    let mut log = Vec::new();
    let mut sent = Vec::new();
    let compacting = random.chance(50);
    let steps = 15 + random.below(25);
    for step in 0..steps {
        let index = random.below(3) as usize;
        if !computers[index].recording && random.chance(25) {
            computers[index].start(&mut log);
        } else if computers[index].recording && random.chance(20) {
            let from = random.below(3) as usize;
            let online = |computer: &Computer| computer.offline_until <= step;
            if from != index
                && computers[from].recording
                && online(&computers[from])
                && online(&computers[index])
            {
                deliver(&mut random, &mut computers, from, index, &mut log, &mut sent);
            }
        } else if random.chance(4) {
            computers[index].offline_until = step + 5 + random.below(20);
        } else if compacting && computers[index].recording && random.chance(15) {
            computers[index].library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
        } else {
            act(&mut random, &mut computers[index], &mut log);
        }
        for computer in &mut computers {
            computer.note_held();
        }
        if random.chance(30) {
            clean_files(&computers[random.below(3) as usize]);
        }
    }
    for computer in &mut computers {
        if !computer.recording {
            computer.start(&mut log);
        }
    }
    for round in 0..4 {
        // In the last round every computer compacts as well, and what that
        // drops is delivered too.
        if compacting && round == 3 {
            for computer in &computers {
                computer.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
            }
        }
        for to in 0..3 {
            for from in 0..3 {
                if from != to {
                    let entries = computers[from].publish();
                    sent.extend(entries.iter().cloned());
                    computers[to].library.merge_entries(&entries).unwrap();
                }
            }
        }
        for computer in &mut computers {
            computer.settle();
            computer.library.merge_entries(&[]).unwrap();
        }
    }
    let mut problems = check(&computers, &log);
    let witness = Computer::new(template);
    witness.library.merge_entries(&sent).unwrap();
    if witness.views() != computers[0].views() {
        problems.push("what compaction dropped changed the library".to_owned());
    }
    assert!(
        problems.is_empty(),
        "seed {seed}: {problems:?}\nlog: {log:#?}\nviews: {:#?}\nlibraries: {:#?}",
        computers.iter().map(Computer::views).collect::<Vec<_>>(),
        computers.iter().map(Computer::pictures).collect::<Vec<_>>()
    );
}

fn check(computers: &[Computer], log: &[Action]) -> Vec<String> {
    let mut problems = Vec::new();
    // Files aren't fetched, so a computer can lack a picture that its entries
    // say exists. The entries must be the same everywhere, and every picture
    // a computer holds must look the way its entries say.
    let libraries = computers.iter().map(Computer::views).collect::<Vec<_>>();
    if libraries.iter().any(|library| *library != libraries[0]) {
        problems.push("the entries of the computers differ".to_owned());
    }
    let held = computers.iter().map(Computer::pictures).collect::<Vec<_>>();
    for (number, pictures) in held.iter().enumerate() {
        for (hash, picture) in pictures {
            if libraries[number].get(hash) != Some(picture) {
                problems.push(format!("computer {number}: {hash} doesn't match its entries"));
            }
        }
        // A merge that deleted a picture kept its file, so the picture is
        // back when the entries add it again.
        for hash in &computers[number].held_files {
            if libraries[number].contains_key(hash) && !pictures.contains_key(hash) {
                problems.push(format!(
                    "computer {number}: {hash} exists by the entries and isn't held"
                ));
            }
        }
    }
    let zero: Time = (0, 0, String::new());
    for number in 1..=HASHES {
        let hash = hash(number);
        // An undo makes the picture exist and puts its tags and folders on, as
        // an add does.
        let adds = log
            .iter()
            .filter_map(|action| match action {
                Action::Add { hash: of, time, start, tags, folders } if *of == hash => {
                    Some((time.clone(), *start, tags, folders))
                }
                Action::Undo { hash: of, time, tags, folders, .. } if *of == hash => {
                    Some((time.clone(), false, tags, folders))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let deletes = log
            .iter()
            .filter_map(|action| match action {
                Action::Delete { hash: of, time } if *of == hash => Some(time.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let deleted = deletes.iter().max().cloned().unwrap_or_else(|| zero.clone());
        let added = adds.iter().map(|add| add.0.clone()).max().unwrap_or_else(|| zero.clone());
        let shown = libraries.iter().map(|library| library.get(&hash)).collect::<Vec<_>>();
        if !adds.is_empty()
            && deletes.is_empty()
            && (shown.iter().any(Option::is_none) || held.iter().all(|pictures| !pictures.contains_key(&hash)))
        {
            problems.push(format!("{hash}: lost although nobody deleted it"));
        }
        if !adds.is_empty() || !deletes.is_empty() {
            let exists = added > deleted;
            if shown.iter().any(|picture| picture.is_some() != exists) {
                problems.push(format!("{hash}: the later of import and deletion did not win"));
            }
        }
        let shown = shown.into_iter().flatten().collect::<Vec<_>>();
        check_revival(computers, log, &hash, &deletes, &shown, &mut problems);
        for field in [Field::Name, Field::Notes, Field::Rating, Field::Status] {
            let last = log
                .iter()
                .filter_map(|action| match action {
                    Action::Edit { hash: of, field: edited, value, time }
                        if *of == hash && *edited == field && *time > deleted =>
                    {
                        Some((time, value))
                    }
                    _ => None,
                })
                .max();
            if let Some((_, value)) = last {
                if shown.iter().any(|picture| picture.field(field) != value) {
                    problems.push(format!("{hash}: the latest {field:?} edit did not win"));
                }
            }
        }
        for (is_tag, names) in [(true, &TAGS[..]), (false, &FOLDERS[..])] {
            for name in names.iter() {
                let explicit = log
                    .iter()
                    .filter_map(|action| match action {
                        Action::Tag { hash: of, tag, on, time } if is_tag && *of == hash && tag == name => {
                            Some((time.clone(), *on))
                        }
                        Action::File { hash: of, folder, on, time }
                            if !is_tag && *of == hash && folder == name =>
                        {
                            Some((time.clone(), *on))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let removals = log
                    .iter()
                    .filter_map(|action| match action {
                        Action::DeleteTag { tag, time } if is_tag && tag == name => Some(time.clone()),
                        Action::DeleteFolder { folder, time } if !is_tag && folder == name => {
                            Some(time.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let has = |picture: &Picture| {
                    if is_tag {
                        picture.tags.contains(*name)
                    } else {
                        picture.folders.contains(*name)
                    }
                };
                let in_record = |tags: &BTreeSet<String>, folders: &BTreeSet<String>| {
                    if is_tag { tags.contains(*name) } else { folders.contains(*name) }
                };
                // Nothing lost: nobody deleted the picture, the tag or the
                // folder, and nobody took it off the picture.
                if deletes.is_empty()
                    && removals.is_empty()
                    && explicit.iter().all(|(_, on)| *on)
                    && !adds.is_empty()
                    && (!explicit.is_empty()
                        || adds.iter().any(|(_, _, tags, folders)| in_record(tags, folders)))
                    && shown.iter().any(|picture| !has(picture))
                {
                    problems.push(format!("{hash}: {name} was lost"));
                }
                // The latest explicit change after the last deletion wins,
                // unless the tag or folder was deleted after it.
                if let Some((time, on)) = explicit.iter().filter(|(time, _)| *time > deleted).max() {
                    if !on {
                        if shown.iter().any(|picture| has(picture)) {
                            problems.push(format!("{hash}: {name} came back after it was taken off"));
                        }
                    } else {
                        let removed = if is_tag {
                            removals.iter().any(|removal| removal >= time)
                        } else {
                            !removals.is_empty()
                        };
                        if !removed && shown.iter().any(|picture| !has(picture)) {
                            problems.push(format!("{hash}: {name} was put on and is missing"));
                        }
                    }
                }
                // A deleted tag stays off unless something put it on again
                // after the deletion; the start of recording doesn't count.
                if is_tag {
                    if let Some(removed) = removals.iter().max() {
                        let again = explicit.iter().any(|(time, on)| *on && time > removed)
                            || adds.iter().any(|(time, start, tags, folders)| {
                                !start && time > removed && in_record(tags, folders)
                            });
                        if !again && shown.iter().any(|picture| has(picture)) {
                            problems.push(format!("{hash}: deleted tag {name} came back"));
                        }
                    }
                }
            }
        }
    }
    problems
}

/// The time below which the entries of `hash` were dropped on any computer,
/// from the `compacted` entries the computers hold.
fn floor_of(computers: &[Computer], hash: &str) -> Time {
    computers
        .iter()
        .flat_map(|computer| {
            computer.rows(
                &format!(
                    "SELECT value FROM shared_entry
                     WHERE subject_key = '{hash}' AND kind = 'compacted'"
                ),
                |row| row.get::<_, String>(0),
            )
        })
        .map(|text| {
            let mut parts = text.splitn(3, ':');
            (
                parts.next().unwrap().parse::<i64>().unwrap(),
                parts.next().unwrap().parse::<i64>().unwrap(),
                parts.next().unwrap().to_owned(),
            )
        })
        .max()
        .unwrap_or((0, 0, String::new()))
}

/// Checks that the latest undo of a deletion of `hash` brought back what any
/// computer had done before the deletion. For each field, tag and folder,
/// the latest change counts when it's earlier than the undone deletion,
/// later than the floor, and no other deletion lies between the two.
fn check_revival(
    computers: &[Computer],
    log: &[Action],
    hash: &str,
    deletes: &[Time],
    shown: &[&Picture],
    problems: &mut Vec<String>,
) {
    let latest_undo = log
        .iter()
        .filter_map(|action| match action {
            Action::Undo { hash: of, time, deleted, .. } if of == hash => Some((time, deleted)),
            _ => None,
        })
        .max();
    let Some((undone_at, deleted)) = latest_undo else {
        return;
    };
    if deletes.iter().any(|time| time > undone_at) {
        return;
    }
    let floor = floor_of(computers, hash);
    let counts = |time: &Time| {
        time < deleted && *time > floor && !deletes.iter().any(|other| time < other && other < deleted)
    };
    for field in [Field::Name, Field::Notes, Field::Rating, Field::Status] {
        let last = log
            .iter()
            .filter_map(|action| match action {
                Action::Edit { hash: of, field: edited, value, time } if of == hash && *edited == field => {
                    Some((time, value))
                }
                _ => None,
            })
            .max();
        if let Some((time, value)) = last {
            if counts(time) && shown.iter().any(|picture| picture.field(field) != value) {
                problems.push(format!("{hash}: the undo didn't bring back the {field:?} edit"));
            }
        }
    }
    for (is_tag, names) in [(true, &TAGS[..]), (false, &FOLDERS[..])] {
        for name in names {
            let last = log
                .iter()
                .filter_map(|action| match action {
                    Action::Tag { hash: of, tag, on, time } if is_tag && of == hash && tag == name => {
                        Some((time, *on))
                    }
                    Action::File { hash: of, folder, on, time }
                        if !is_tag && of == hash && folder == name =>
                    {
                        Some((time, *on))
                    }
                    _ => None,
                })
                .max();
            let Some((time, on)) = last else {
                continue;
            };
            if !counts(time) {
                continue;
            }
            let removed = log.iter().any(|action| match action {
                Action::DeleteTag { tag, time: at } => is_tag && tag == name && at >= time,
                Action::DeleteFolder { folder, .. } => !is_tag && folder == name,
                _ => false,
            });
            let want = on && !removed;
            let has = |picture: &Picture| {
                if is_tag { picture.tags.contains(*name) } else { picture.folders.contains(*name) }
            };
            if shown.iter().any(|picture| has(picture) != want) {
                problems.push(format!("{hash}: the undo didn't bring back {name}"));
            }
        }
    }
}

fn template() -> (TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("template.sqlite");
    drop(Library::create(&path).unwrap());
    (directory, path)
}

fn runs(first: u64) {
    let (_directory, template) = template();
    if let Some(seed) = std::env::var("SHARED_ENTRIES_SEED").ok().and_then(|seed| seed.parse().ok()) {
        if first == 0 {
            run(seed, &template);
        }
        return;
    }
    for seed in first..first + RUNS_PER_TEST {
        let outcome = std::panic::catch_unwind(|| run(seed, &template));
        assert!(outcome.is_ok(), "seed {seed} failed");
    }
}

#[test]
fn random_runs_0() {
    runs(0);
}

#[test]
fn random_runs_1() {
    runs(RUNS_PER_TEST);
}

#[test]
fn random_runs_2() {
    runs(2 * RUNS_PER_TEST);
}

#[test]
fn random_runs_3() {
    runs(3 * RUNS_PER_TEST);
}

#[test]
fn random_runs_4() {
    runs(4 * RUNS_PER_TEST);
}

#[test]
fn random_runs_5() {
    runs(5 * RUNS_PER_TEST);
}

#[test]
fn random_runs_6() {
    runs(6 * RUNS_PER_TEST);
}

#[test]
fn random_runs_7() {
    runs(7 * RUNS_PER_TEST);
}

/// Collections don't sync yet. A picture that is in a collection on one
/// computer keeps its place there, whatever another computer does to the
/// same file outside a collection, and it gets no entries of its own.
#[test]
fn a_picture_in_a_collection_is_left_alone() {
    let (_directory, template) = template();
    let mut log = Vec::new();
    let mut loose = Computer::new(&template);
    let mut grouped = Computer::new(&template);
    loose.start(&mut log);
    grouped.start(&mut log);
    loose.import(&hash(1), "loose", vec!["red".into()], Vec::new());
    let members = vec![
        grouped.prepared(&hash(1), "member", Vec::new(), Vec::new()),
        grouped.prepared(&hash(2), "other member", Vec::new(), Vec::new()),
    ];
    grouped
        .library
        .ingest_collection(&PreparedCollectionImport {
            members,
            cover_index: 0,
            existing_root_id: None,
            name: Some("set".into()),
            modified_at_ms: now_ms(),
        })
        .unwrap();
    let held_files = |computer: &Computer| {
        computer.rows(
            "SELECT file.content_hash FROM media_item media
             JOIN media_file file ON file.file_id = media.file_id ORDER BY 1",
            |row| row.get::<_, String>(0),
        )
    };
    let before = held_files(&grouped);

    // The other computer renames the file, then deletes it.
    let root = loose.roots()[&hash(1)];
    loose.library.rename_root(root, "renamed", now_ms()).unwrap();
    grouped.library.merge_entries(&loose.own_entries()).unwrap();
    assert_eq!(held_files(&grouped), before);
    let target = loose.target(root);
    loose.library.set_lifecycle(&target, Lifecycle::Trash).unwrap();
    loose.library.permanently_delete(&target, now_ms(), 500, false).unwrap();
    grouped.library.merge_entries(&loose.own_entries()).unwrap();

    assert_eq!(held_files(&grouped), before);
    assert!(grouped.roots().is_empty());
    assert!(grouped.own_entries().is_empty());
    assert!(grouped.library.wanted_files(None, 10).unwrap().is_empty());
}

#[test]
fn deleting_a_collection_writes_no_delete_for_its_pictures() {
    let (_directory, template) = template();
    let mut log = Vec::new();
    let mut loose = Computer::new(&template);
    let mut grouped = Computer::new(&template);
    loose.start(&mut log);
    grouped.start(&mut log);
    loose.import(&hash(1), "loose", Vec::new(), Vec::new());
    let members = vec![
        grouped.prepared(&hash(1), "member", Vec::new(), Vec::new()),
        grouped.prepared(&hash(2), "other member", Vec::new(), Vec::new()),
    ];
    grouped
        .library
        .ingest_collection(&PreparedCollectionImport {
            members,
            cover_index: 0,
            existing_root_id: None,
            name: Some("set".into()),
            modified_at_ms: now_ms(),
        })
        .unwrap();
    let collection = grouped.rows(
        "SELECT root.root_id FROM library_root root
         WHERE NOT EXISTS (SELECT 1 FROM media_item media WHERE media.media_id = root.root_id)",
        |row| row.get::<_, u32>(0),
    );
    assert_eq!(collection.len(), 1, "the import made one collection");
    let target = grouped.target(RootId(collection[0]));
    grouped.library.set_lifecycle(&target, Lifecycle::Trash).unwrap();
    grouped.library.permanently_delete(&target, now_ms(), 500, false).unwrap();
    grouped.settle();

    let deletes: Vec<_> = grouped
        .own_entries()
        .into_iter()
        .filter(|entry| entry.kind.as_str() == "delete")
        .collect();
    assert!(deletes.is_empty(), "a picture in a collection wrote a delete: {deletes:?}");
    loose.library.merge_entries(&grouped.own_entries()).unwrap();
    assert!(loose.roots().contains_key(&hash(1)), "the other computer lost its picture");
}

/// Two computers that both hold `hash(1)`, named "first", and have synced.
fn two_synced(template: &std::path::Path) -> (Computer, Computer) {
    let mut log = Vec::new();
    let mut one = Computer::new(template);
    let mut two = Computer::new(template);
    one.start(&mut log);
    two.start(&mut log);
    one.import(&hash(1), "first", vec!["cat".into()], Vec::new());
    two.library.merge_entries(&one.own_entries()).unwrap();
    two.fetch_wanted();
    two.settle();
    two.library.merge_entries(&[]).unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    (one, two)
}

/// A rename made after another computer's entries were stored and before
/// they were applied stays.
#[test]
fn a_local_edit_between_storing_and_applying_is_kept() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    let root = one.roots()[&hash(1)];
    one.library.set_rating(&one.target(root), Rating::Three).unwrap();
    two.library.store_entries(&one.own_entries()).unwrap();
    let root = two.roots()[&hash(1)];
    two.library.rename_root(root, "renamed here", now_ms()).unwrap();
    while two.library.apply_merged(1_000).unwrap().more {}
    let picture = two.pictures().remove(&hash(1)).unwrap();
    assert_eq!((picture.name.as_str(), picture.rating.as_str()), ("renamed here", "3"));
    one.library.merge_entries(&two.own_entries()).unwrap();
    assert_eq!(one.pictures()[&hash(1)].name, "renamed here");
}

/// Merging the tag "cat" into "animal" takes "cat" off the pictures on the
/// other computers too, and the merge doesn't bring "cat" back.
#[test]
fn merging_a_tag_into_another_moves_it_on_every_computer() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    let root = one.roots()[&hash(1)];
    one.library.apply_tags(&one.target(root), &["animal".into()], true).unwrap();
    let ids = one.tag_ids();
    one.library.merge_tags(ids["cat"], ids["animal"], now_ms()).unwrap();
    two.library.merge_entries(&one.own_entries()).unwrap();
    let tags = two.pictures()[&hash(1)].tags.clone();
    assert_eq!(tags, BTreeSet::from(["animal".to_owned()]));
}

/// An `add` entry whose record this version can't fully read, for example
/// one with a status from a newer version, still keeps the picture.
#[test]
fn an_add_with_a_record_from_a_newer_version_still_counts() {
    let (_directory, template) = template();
    let (one, _two) = two_synced(&template);
    let later = now_ms() + 600_000;
    let entry = |kind, second_key: &str, value: Option<&str>, clock_counter| SharedEntry {
        kind,
        subject_key: hash(1),
        second_key: second_key.to_owned(),
        value: value.map(str::to_owned),
        clock_ms: later,
        clock_counter,
        device_id: "ffffffff-0000-4000-8000-000000000099".into(),
    };
    one.library
        .merge_entries(&[
            entry(picto_library::EntryKind::Delete, "", None, 0),
            entry(
                picto_library::EntryKind::Add,
                "ffffffff-0000-4000-8000-000000000099",
                Some(r#"{"name":"from a newer version","status":"archived","stars":{"x":1}}"#),
                1,
            ),
        ])
        .unwrap();
    let picture = one.pictures().remove(&hash(1)).expect("the picture is kept");
    assert_eq!(picture.status, "active");
}

/// Undoing an import task after its pictures were recorded moves them to the
/// Trash, so the other computers see it too.
#[test]
fn undoing_an_import_task_after_its_pictures_were_recorded_trashes_them() {
    let (_directory, template) = template();
    let (mut one, two) = two_synced(&template);
    let task = one.library.create_task("import", "Import", "{}", true, false, now_ms()).unwrap();
    let input = one.prepared(&hash(2), "from a task", Vec::new(), Vec::new());
    one.library.ingest_batch_for_task(&[input], task, false).unwrap();
    two.library.merge_entries(&one.own_entries()).unwrap();
    one.library.stop_task(task, true, now_ms()).unwrap();
    one.library.revert_task(task, now_ms()).unwrap();
    assert_eq!(one.pictures()[&hash(2)].status, "trash");
    two.library.merge_entries(&one.own_entries()).unwrap();
    assert_eq!(
        two.library.entry_view(&hash(2)).unwrap().map(|view| view.status),
        Some(Lifecycle::Trash)
    );
}

/// A rename that isn't an entry yet when a repair replaces the file stays
/// with the old file; it isn't sent as a rename of the new one.
#[test]
fn a_pending_rename_before_a_repair_stays_with_the_old_file() {
    let (_directory, template) = template();
    let (one, _two) = two_synced(&template);
    let root = one.roots()[&hash(1)];
    one.library.rename_root(root, "renamed before the repair", now_ms()).unwrap();
    let media_id = one.library.details(root).unwrap().media[0].media_id;
    one.library
        .repair_media_content(
            media_id,
            &picto_library::RepairedMediaContent {
                expected_hash: hash(1),
                content_hash: hash(2),
                file_path: "/picto-test/repaired.png".into(),
                size_bytes: 2_048,
            },
            now_ms(),
        )
        .unwrap();
    let renames = one
        .own_entries()
        .into_iter()
        .filter(|entry| entry.kind == picto_library::EntryKind::Name)
        .map(|entry| entry.subject_key)
        .collect::<Vec<_>>();
    assert_eq!(renames, [hash(1)]);
}

/// Merging the tag group "char" into "person" moves its tags on every
/// computer, both a tag that "person" has too and one that it doesn't have.
#[test]
fn merging_a_tag_group_into_another_moves_its_tags_on_every_computer() {
    for existing in ["person:red", "person:other"] {
        merge_tag_group(existing);
    }
}

fn merge_tag_group(existing: &str) {
    let (_directory, template) = template();
    let (mut one, two) = two_synced(&template);
    one.import(&hash(2), "second", vec![existing.into()], Vec::new());
    let root = one.roots()[&hash(1)];
    one.library.apply_tags(&one.target(root), &["char:red".into()], true).unwrap();
    two.library.merge_entries(&one.own_entries()).unwrap();
    let char_group = one
        .library
        .tag_namespaces()
        .unwrap()
        .into_iter()
        .find(|namespace| namespace.name == "char")
        .unwrap();
    one.library.rename_or_merge_tag_namespace(char_group.namespace_id, "person").unwrap();
    let entries = one.own_entries();
    two.library.merge_entries(&entries).unwrap();
    assert_eq!(
        two.library.entry_view(&hash(1)).unwrap().unwrap().tags,
        BTreeSet::from(["cat".to_owned(), "person:red".to_owned()]),
        "{existing}: {entries:#?}"
    );
}

/// A repair that turns a picture into a file another computer added earlier
/// makes the picture look the way the entries say.
#[test]
fn a_repair_into_a_file_that_exists_follows_the_entries() {
    let (_directory, template) = template();
    let (mut one, mut two) = two_synced(&template);
    one.import(&hash(2), "from one", Vec::new(), Vec::new());
    two.import(&hash(3), "from two", Vec::new(), Vec::new());
    two.library.merge_entries(&one.own_entries()).unwrap();
    let root = two.roots()[&hash(3)];
    let media_id = two.library.details(root).unwrap().media[0].media_id;
    two.library
        .repair_media_content(
            media_id,
            &picto_library::RepairedMediaContent {
                expected_hash: hash(3),
                content_hash: hash(2),
                file_path: "/picto-test/repaired.png".into(),
                size_bytes: 2_048,
            },
            now_ms(),
        )
        .unwrap();
    two.settle();
    two.library.merge_entries(&[]).unwrap();
    assert_eq!(Some(&two.pictures()[&hash(2)]), two.views().get(&hash(2)));
    assert_eq!(two.pictures()[&hash(2)].name, "from one");
}

/// Undoing an import task keeps a picture whose file another computer has
/// added, because this computer's import only brought the file here.
#[test]
fn undoing_an_import_task_keeps_a_picture_another_computer_added() {
    let (_directory, template) = template();
    let (mut one, mut two) = two_synced(&template);
    two.import(&hash(2), "from two", Vec::new(), Vec::new());
    one.library.merge_entries(&two.own_entries()).unwrap();
    let task = one.library.create_task("import", "Import", "{}", true, false, now_ms()).unwrap();
    let input = one.prepared(&hash(2), "fetched", Vec::new(), Vec::new());
    one.library.ingest_batch_for_task(&[input], task, false).unwrap();
    one.settle();
    one.library.stop_task(task, true, now_ms()).unwrap();
    one.library.revert_task(task, now_ms()).unwrap();
    assert!(one.roots().contains_key(&hash(2)));
}

/// The file of `hash` as `computer` holds it: its path, and whether the
/// file is waiting to be removed from the disk.
fn file_on_disk(computer: &Computer, hash: &str) -> Option<(String, bool)> {
    computer
        .rows(
            &format!(
                "SELECT file.file_path,
                        EXISTS (SELECT 1 FROM blob_cleanup_queue queue
                                WHERE queue.file_id = file.file_id)
                 FROM media_file file WHERE file.content_hash = '{hash}'"
            ),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .pop()
}

/// Another computer deletes a picture and then adds it again, and this
/// computer merges the deletion first. The deletion removes the picture here,
/// and the later entries bring it back with the file this computer had,
/// without fetching it. They are either the undo of the deletion or an import
/// on a third computer.
#[test]
fn a_picture_that_a_merge_deleted_comes_back_with_its_file() {
    for undo in [true, false] {
        let (_directory, template) = template();
        let (one, two) = two_synced(&template);
        let mut log = Vec::new();
        let mut three = Computer::new(&template);
        three.start(&mut log);
        let file = file_on_disk(&one, &hash(1)).unwrap();

        let root = two.roots()[&hash(1)];
        two.library.set_lifecycle(&two.target(root), Lifecycle::Trash).unwrap();
        two.library.permanently_delete(&two.target(root), now_ms(), 500, false).unwrap();
        let (deletion, rest): (Vec<_>, Vec<_>) = if undo {
            // The undo replaces the deletion in the same entry, so the
            // deletion is taken before the undo.
            let deletion = two
                .own_entries()
                .into_iter()
                .filter(|entry| entry.kind == picto_library::EntryKind::Delete)
                .collect();
            two.library.undo().unwrap();
            (deletion, two.own_entries())
        } else {
            // The third computer has the deletion and imports the file again.
            let entries = two.own_entries();
            three.library.merge_entries(&entries).unwrap();
            three.import(&hash(1), "from three", Vec::new(), Vec::new());
            (entries, three.own_entries())
        };

        one.library.store_entries(&deletion).unwrap();
        one.library.apply_stored(false).unwrap();
        assert!(!one.roots().contains_key(&hash(1)), "undo {undo}");
        assert_eq!(file_on_disk(&one, &hash(1)), Some(file.clone()), "undo {undo}");
        let entries_before = one.own_entries();

        one.library.store_entries(&rest).unwrap();
        one.library.apply_stored(false).unwrap();
        assert!(one.roots().contains_key(&hash(1)), "undo {undo}: the picture is not back");
        assert_eq!(file_on_disk(&one, &hash(1)), Some(file.clone()), "undo {undo}");
        assert_eq!(one.pictures().get(&hash(1)), one.views().get(&hash(1)), "undo {undo}");
        assert!(one.library.wanted_files(None, 10).unwrap().is_empty(), "undo {undo}");
        // Bringing the picture back is part of the merge and writes no entry.
        assert_eq!(one.own_entries(), entries_before, "undo {undo}");
    }
}

/// Sets "Keep deleted files" of `computer` to `days`.
fn set_keep_days(computer: &Computer, days: i64) {
    computer
        .library
        .database()
        .maintenance_write(WorkPriority::Maintenance, |transaction| {
            transaction.execute(
                "UPDATE cloud_state
                 SET retention_json = json_set(retention_json, '$.deleted_blobs_days', ?1)",
                [days],
            )?;
            Ok(())
        })
        .unwrap();
}

/// Runs the removal of queued files the way the cleanup worker does, and
/// returns the hashes of the files it removed.
fn clean_files(computer: &Computer) -> Vec<String> {
    let mut removed = Vec::new();
    computer
        .library
        .clean_pending_blobs(64, |pending| {
            removed.push(pending.content_hash.clone());
            Ok(())
        })
        .unwrap();
    removed
}

/// "Keep deleted files" is about the originals in the cloud folder. The file
/// that a merge keeps on this computer stays for at least 7 days even when
/// the setting is 0 days, so a later add still brings the picture back.
#[test]
fn a_merge_keeps_the_file_for_seven_days_whatever_the_setting() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    set_keep_days(&one, 0);
    let file = file_on_disk(&one, &hash(1)).unwrap();
    let root = two.roots()[&hash(1)];
    two.library.set_lifecycle(&two.target(root), Lifecycle::Trash).unwrap();
    two.library.permanently_delete(&two.target(root), now_ms(), 500, false).unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    assert!(one.roots().is_empty());
    clean_files(&one);
    two.library.undo().unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    assert!(one.roots().contains_key(&hash(1)), "the picture is not back");
    assert_eq!(file_on_disk(&one, &hash(1)), Some(file));
}

/// The file that a merge kept is removed from the disk once its keep time is
/// over and the picture doesn't exist by the entries.
#[test]
fn a_kept_file_is_removed_after_the_keep_time() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    let root = two.roots()[&hash(1)];
    two.library.set_lifecycle(&two.target(root), Lifecycle::Trash).unwrap();
    two.library.permanently_delete(&two.target(root), now_ms(), 500, false).unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    assert!(clean_files(&one).is_empty());
    assert!(file_on_disk(&one, &hash(1)).is_some());

    // The keep time is over.
    one.library
        .database()
        .maintenance_write(WorkPriority::Maintenance, |transaction| {
            transaction.execute("UPDATE kept_deleted_file SET remove_after_ms = 0", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(clean_files(&one), [hash(1)]);
    assert_eq!(file_on_disk(&one, &hash(1)), None);
}

/// A file that a merge kept isn't one of the library's files in its
/// statistics.
#[test]
fn a_kept_file_is_not_counted_in_the_statistics() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    let root = two.roots()[&hash(1)];
    two.library.set_lifecycle(&two.target(root), Lifecycle::Trash).unwrap();
    two.library.permanently_delete(&two.target(root), now_ms(), 500, false).unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    assert!(file_on_disk(&one, &hash(1)).is_some());
    let statistics = one.library.library_statistics().unwrap();
    assert_eq!((statistics.physical_files, statistics.original_bytes), (0, 0));
}

/// A file that a merge kept is no candidate for duplicate review, so its
/// pairs don't take the place of the pairs of pictures in the library.
#[test]
fn a_kept_file_is_not_a_duplicate_candidate() {
    let (_directory, template) = template();
    let (mut one, two) = two_synced(&template);
    one.import(&hash(2), "second", Vec::new(), Vec::new());
    let file_of = |computer: &Computer, hash: &str| {
        computer.library.details(computer.roots()[hash]).unwrap().media[0].file_id
    };
    one.library
        .record_duplicate_pair(file_of(&one, &hash(1)), file_of(&one, &hash(2)), 1, now_ms())
        .unwrap();
    assert_eq!(
        one.library.duplicate_pairs(Some(picto_library::DuplicateStatus::Detected), 10).unwrap().len(),
        1
    );
    let root = two.roots()[&hash(1)];
    two.library.set_lifecycle(&two.target(root), Lifecycle::Trash).unwrap();
    two.library.permanently_delete(&two.target(root), now_ms(), 500, false).unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    assert!(file_on_disk(&one, &hash(1)).is_some());
    assert!(one
        .library
        .duplicate_pairs(Some(picto_library::DuplicateStatus::Detected), 10)
        .unwrap()
        .is_empty());
}

/// Lets the wall clock move on, so that the next action is later than the
/// last one on every computer.
fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(3));
}

const EIGHT_DAYS_MS: i64 = 8 * 24 * 60 * 60 * 1_000;

/// One computer gives a picture 5 stars. Before that reaches the other
/// computer, the other deletes the picture and undoes the deletion. The
/// picture comes back with the 5 stars on both.
#[test]
fn undoing_a_deletion_brings_back_a_change_that_had_not_arrived() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    one.library.set_rating(&one.target(one.roots()[&hash(1)]), Rating::Five).unwrap();
    pause();
    two.delete(&hash(1));
    two.library.undo().unwrap();
    two.library.merge_entries(&one.own_entries()).unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    for computer in [&one, &two] {
        assert_eq!(computer.pictures()[&hash(1)].rating, "5");
        assert_eq!(computer.views()[&hash(1)].rating, "5");
    }
}

/// One computer deletes a picture, the other deletes it later without having
/// seen that, and the first undoes its deletion after both. The undo is the
/// later action, so the picture comes back on both.
#[test]
fn an_undo_wins_over_an_earlier_deletion_on_another_computer() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    two.delete(&hash(1));
    pause();
    one.delete(&hash(1));
    pause();
    two.library.undo().unwrap();
    two.library.merge_entries(&one.own_entries()).unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    assert!(two.roots().contains_key(&hash(1)));
    assert_eq!(one.views(), two.views());
    assert!(one.views().contains_key(&hash(1)));
}

/// 7 days after a deletion, the computer that deleted the picture writes a
/// `compacted` entry and drops the picture's other entries. It drops its own
/// entries only once the `compacted` entry is in the cloud folder. The other
/// computer drops the same entries when it merges that entry. An old entry
/// that arrives afterwards isn't stored.
#[test]
fn compaction_drops_what_a_deletion_made_obsolete_on_every_computer() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    let root = one.roots()[&hash(1)];
    one.library.set_rating(&one.target(root), Rating::Four).unwrap();
    one.library.rename_root(root, "renamed", now_ms()).unwrap();
    let old = one.own_entries();
    one.delete(&hash(1));
    two.library.merge_entries(&one.publish()).unwrap();
    assert!(two.entry_kinds(&hash(1)).len() > 2);

    // Less than 7 days: nothing is dropped.
    one.library.compact_entries(now_ms()).unwrap();
    assert!(one.entry_kinds(&hash(1)).len() > 2);
    one.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
    assert!(one.entry_kinds(&hash(1)).contains(&"compacted".to_owned()));
    assert!(one.entry_kinds(&hash(1)).contains(&"rating".to_owned()));
    assert!(one.library.removed_own_entries().unwrap().is_empty());
    one.publish();
    one.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
    assert_eq!(one.entry_kinds(&hash(1)), ["compacted", "delete"]);
    let removed = one.library.removed_own_entries().unwrap();
    assert!(removed.iter().any(|(kind, _, _)| *kind == picto_library::EntryKind::Rating));
    assert!(removed.iter().all(|(_, subject_key, _)| *subject_key == hash(1)));

    two.library.merge_entries(&one.publish()).unwrap();
    assert_eq!(two.entry_kinds(&hash(1)), ["compacted", "delete"]);
    // The entries from before the deletion come again, as from a computer
    // that was away for weeks.
    two.library.merge_entries(&old).unwrap();
    assert_eq!(two.entry_kinds(&hash(1)), ["compacted", "delete"]);
    assert_eq!(one.views(), two.views());
    assert!(two.views().is_empty());
}

/// A computer writes no `compacted` entry for its own deletion before it has
/// written the deletion to the cloud folder, and drops its own entries only
/// after it has written the `compacted` entry there too.
#[test]
fn compaction_waits_until_the_deletion_is_in_the_cloud_folder() {
    let (_directory, template) = template();
    let (one, _two) = two_synced(&template);
    one.delete(&hash(1));
    one.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
    assert!(one.entry_kinds(&hash(1)).iter().any(|kind| kind.starts_with("add ")));
    one.publish();
    one.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
    one.publish();
    one.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
    assert_eq!(one.entry_kinds(&hash(1)), ["compacted", "delete"]);
}

/// One computer gives a picture 5 stars, a second deletes it without having
/// seen that and undoes the deletion, but its undo reaches the others only
/// after the first has compacted. The third computer never compacted and
/// still has the 5 stars. All three end with the same picture: the one the
/// undoing computer had, without the 5 stars.
#[test]
fn an_undo_that_arrives_after_compaction_gives_the_same_picture_everywhere() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    let mut log = Vec::new();
    let mut three = Computer::new(&template);
    three.start(&mut log);
    three.library.merge_entries(&one.publish()).unwrap();
    one.library.set_rating(&one.target(one.roots()[&hash(1)]), Rating::Five).unwrap();
    pause();
    two.delete(&hash(1));
    let deletion = two.publish();
    two.library.undo().unwrap();
    for computer in [&one, &three] {
        computer.library.merge_entries(&deletion).unwrap();
    }
    three.library.merge_entries(&one.publish()).unwrap();
    one.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
    one.publish();
    one.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
    assert_eq!(one.entry_kinds(&hash(1)), ["compacted", "delete"]);

    for _ in 0..2 {
        for (to, from) in [
            (&one, &two),
            (&one, &three),
            (&two, &one),
            (&two, &three),
            (&three, &one),
            (&three, &two),
        ] {
            to.library.merge_entries(&from.publish()).unwrap();
        }
    }
    let views = [one.views(), two.views(), three.views()];
    assert_eq!(views[0], views[1]);
    assert_eq!(views[0], views[2]);
    assert_eq!(views[0][&hash(1)].rating, "0");
    assert_eq!(two.pictures()[&hash(1)], views[0][&hash(1)]);
}

/// A rating that isn't an entry yet when the picture is deleted becomes one
/// before the deletion, so the undo of the deletion brings it back, here and
/// on the other computer.
#[test]
fn an_edit_just_before_a_deletion_comes_back_with_the_undo() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    one.library.set_rating(&one.target(one.roots()[&hash(1)]), Rating::Four).unwrap();
    one.delete(&hash(1));
    one.settle();
    one.library.undo().unwrap();
    one.library.merge_entries(&[]).unwrap();
    two.library.merge_entries(&one.own_entries()).unwrap();
    for computer in [&one, &two] {
        assert_eq!(computer.views()[&hash(1)].rating, "4");
    }
    assert_eq!(one.pictures()[&hash(1)].rating, "4");
}

/// An undo that arrives after compaction brings the picture back with the
/// undoing computer's record. A tag in that record that another computer
/// deleted after the undo stays off.
#[test]
fn a_tag_deleted_after_a_late_undo_stays_off() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    two.delete(&hash(1));
    one.library.merge_entries(&two.publish()).unwrap();
    one.library.compact_entries(now_ms() + EIGHT_DAYS_MS).unwrap();
    two.library.undo().unwrap();
    pause();
    let cat = one.tag_ids()["cat"];
    one.library.delete_tag(cat, now_ms()).unwrap();
    for _ in 0..2 {
        two.library.merge_entries(&one.publish()).unwrap();
        one.library.merge_entries(&two.publish()).unwrap();
    }
    for computer in [&one, &two] {
        assert_eq!(computer.views()[&hash(1)].tags, BTreeSet::new());
    }
    assert_eq!(two.pictures()[&hash(1)].tags, BTreeSet::new());
}

/// Closes the library of `computer` and opens it again, as a restart of the
/// app does.
fn reopen(computer: Computer) -> Computer {
    let Computer {
        library,
        _directory,
        id,
        recording,
        imports,
        deleted_last,
        undone_last,
        offline_until,
        held_files,
    } = computer;
    drop(library);
    let library = Library::open(_directory.path().join("library.sqlite")).unwrap();
    Computer {
        library,
        _directory,
        id,
        recording,
        imports,
        deleted_last,
        undone_last,
        offline_until,
        held_files,
    }
}

/// The app closes right after the undo of a deletion, before a sync. The
/// undo still brings back the 5 stars that another computer gave the picture
/// and that hadn't arrived before the deletion.
#[test]
fn an_undo_brings_back_unseen_changes_after_a_restart() {
    let (_directory, template) = template();
    let (one, two) = two_synced(&template);
    one.library.set_rating(&one.target(one.roots()[&hash(1)]), Rating::Five).unwrap();
    pause();
    two.delete(&hash(1));
    two.library.undo().unwrap();
    let two = reopen(two);
    two.library.merge_entries(&one.own_entries()).unwrap();
    one.library.merge_entries(&two.own_entries()).unwrap();
    for computer in [&one, &two] {
        assert_eq!(computer.views()[&hash(1)].rating, "5");
    }
    assert_eq!(two.pictures()[&hash(1)].rating, "5");
}

/// A computer deletes a picture, imports its file again in a task, and
/// undoes the task before the import became an entry. Its old `add` entry is
/// older than its deletion, so no other computer has the picture: the undo
/// removes the picture instead of moving it to the Trash.
#[test]
fn undoing_an_import_task_removes_a_picture_that_this_computer_deleted_before() {
    let (_directory, template) = template();
    let (mut one, _two) = two_synced(&template);
    one.delete(&hash(1));
    one.settle();
    pause();
    let task = one.library.create_task("import", "Import", "{}", true, false, now_ms()).unwrap();
    let input = one.prepared(&hash(1), "imported again", Vec::new(), Vec::new());
    one.library.ingest_batch_for_task(&[input], task, false).unwrap();
    one.library.stop_task(task, true, now_ms()).unwrap();
    one.library.revert_task(task, now_ms()).unwrap();
    assert!(!one.roots().contains_key(&hash(1)), "the picture is in the Trash");
}
