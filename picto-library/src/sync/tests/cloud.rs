//! Checks of libraries that sync through a cloud folder. A folder in a
//! temporary directory stands in for the folder of the Google Drive or
//! Dropbox app, and the time is moved forward to check what happens after
//! days and weeks.

use std::path::{Path, PathBuf};

use super::*;
use crate::sync::cloud::SyncStop;
use crate::{SyncComputer, SyncReport};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn computer(machine: &str) -> SyncComputer {
    SyncComputer { machine_id: machine.into(), name: format!("computer {machine}"), app_version: "test".into() }
}

/// Moves the time that sync works with forward by `days` days.
fn move_time(days: i64) {
    crate::sync::TIME_SHIFT_MS.with(|shift| shift.set(shift.get() + days * DAY_MS));
}

/// One copy of the library on one computer.
struct Device {
    library: Library,
    path: PathBuf,
    computer: SyncComputer,
}

impl Device {
    /// A new library without sync, in the folder `name` of `directory`.
    fn create(directory: &Path, name: &str) -> Self {
        std::fs::create_dir(directory.join(name)).unwrap();
        let path = directory.join(name).join("library.sqlite");
        Self { library: Library::create(&path).unwrap(), path, computer: computer(name) }
    }

    /// Turns sync on with a new cloud library in `cloud`, and returns its id.
    fn turn_on(&self, cloud: &Path) -> String {
        self.library.turn_on_sync(cloud, "library", &self.computer).unwrap()
    }

    /// A new library, in the folder `name` of `directory`, that opens the
    /// cloud library `library_id` in `cloud`.
    fn open(directory: &Path, name: &str, cloud: &Path, library_id: &str) -> Self {
        let device = Self::create(directory, name);
        device.library.open_from_cloud(cloud, library_id, &device.computer).unwrap();
        assert_recorded(&device.library, "after opening from the cloud");
        device
    }

    /// Syncs. Merging the change files of others records nothing, so a full
    /// comparison afterwards finds nothing to record.
    fn sync(&self, context: &str) -> SyncReport {
        let report = self
            .library
            .sync_with_cloud(&self.computer)
            .unwrap_or_else(|error| panic!("{context}: {error}"))
            .expect("the library is connected to the cloud folder");
        assert_recorded(&self.library, context);
        report
    }

    /// Runs a sync that stops at `stop`, as a crash there would stop it, and
    /// opens the library again.
    fn crash(self, stop: SyncStop) -> Self {
        self.library.sync_with_cloud_until(&self.computer, Some(stop)).unwrap();
        self.reopen()
    }

    fn reopen(self) -> Self {
        self.reopen_after(|| {})
    }

    /// Closes the library, runs `while_closed`, and opens it again.
    fn reopen_after(self, while_closed: impl FnOnce()) -> Self {
        let Self { library, path, computer } = self;
        drop(library);
        while_closed();
        Self { library: Library::open(&path).unwrap(), path, computer }
    }

    fn tag(&self, name: &str) -> crate::TagId {
        self.library.projections().snapshot().tag_ids_by_name[name]
    }
}

/// Every library syncs, one after another, until a round of syncs reads and
/// writes no change file, and then all are the same library, with the same
/// orders.
fn settle(devices: &[Device], context: &str) {
    let mut rounds = 0;
    loop {
        let moved = devices
            .iter()
            .map(|device| {
                let report = device.sync(context);
                report.files_read + report.files_written
            })
            .sum::<usize>();
        if moved == 0 {
            break;
        }
        rounds += 1;
        assert!(rounds <= devices.len() + 2, "{context}: the syncs keep reading or writing change files");
    }
    for (index, device) in devices.iter().enumerate().skip(1) {
        assert_same(&devices[0].library, &device.library, &format!("library {index}, {context}"));
    }
}

/// `count` libraries make random changes and sync at random times through
/// one cloud folder. The first one has some content when it turns sync on;
/// the others open the cloud library. Some syncs stop as a crash would stop
/// them: after a change file is written and before the outbox is cleared,
/// or after a change file of another device is merged and before that is
/// committed. Now and then the time moves forward by eight days, so that the
/// syncs write snapshots, leave out what was deleted long ago, and delete
/// covered snapshots and change files; a library that does not sync for a
/// while catches up through the snapshots. At the end all libraries must be
/// the same.
fn sync_through_a_folder(count: usize, seed: u64) {
    crate::sync::TIME_SHIFT_MS.with(|shift| shift.set(0));
    let directory = tempfile::tempdir().unwrap();
    let cloud = directory.path().join("cloud");
    std::fs::create_dir(&cloud).unwrap();
    let mut random = Random(seed);
    let (mut next_hash, mut time) = (0, 0);
    let mut log = Vec::new();
    let first = Device::create(directory.path(), "library-0");
    for _ in 0..20 {
        log.push(format!("before turning on: {}", operate(&first.library, &mut random, &mut next_hash, &mut time)));
    }
    let library_id = first.turn_on(&cloud);
    let mut devices = vec![first];
    for index in 1..count {
        devices.push(Device::open(directory.path(), &format!("library-{index}"), &cloud, &library_id));
    }
    for step in 0..300 {
        let index = random.below(count);
        let context = format!("seed {seed} at step {step} after {log:?}");
        match random.below(13) {
            0..=2 => {
                let report = devices[index].sync(&context);
                log.push(format!("{step}: {index} syncs: {report:?}"));
            }
            choice @ (3 | 4) => {
                let stop = if choice == 3 { SyncStop::AfterChangeFileWritten } else { SyncStop::BeforeReadIsSaved };
                log.push(format!("{step}: {index} stops its sync at {stop:?}"));
                let device = devices.remove(index);
                devices.insert(index, device.crash(stop));
                assert_recorded(&devices[index].library, &context);
            }
            5 => {
                log.push(format!("{step}: eight days pass"));
                move_time(8);
            }
            _ => {
                let operation = operate(&devices[index].library, &mut random, &mut next_hash, &mut time);
                log.push(format!("{step}: {index} {operation}"));
                if random.below(2) == 0 {
                    assert_recorded(&devices[index].library, &context);
                }
            }
        }
    }
    settle(&devices, &format!("seed {seed} at the end after {log:?}"));
}

#[test]
fn two_and_three_libraries_that_sync_through_a_cloud_folder_end_the_same() {
    for seed in 1..=5u64 {
        sync_through_a_folder(2, seed * 15_485_863);
        sync_through_a_folder(3, seed * 32_452_843);
    }
}

#[test]
fn a_library_opened_from_the_cloud_ends_the_same_as_the_one_that_turned_sync_on() {
    let directory = tempfile::tempdir().unwrap();
    let cloud = directory.path().join("cloud");
    std::fs::create_dir(&cloud).unwrap();
    let first = Device::create(directory.path(), "first");
    let (folder, _) = first.library.create_folder("Folder", None).unwrap();
    first.library.ingest(&prepared(hash(1), Lifecycle::Active, vec!["a".into()], vec![folder])).unwrap();
    first.library.ingest(&prepared(hash(2), Lifecycle::Inbox, vec!["g:b".into()], Vec::new())).unwrap();
    first.library.set_rating(&one(root_of(&first.library, &hash(2))), Rating::Three).unwrap();
    let library_id = first.turn_on(&cloud);

    let second = Device::open(directory.path(), "second", &cloud, &library_id);
    assert_same(&first.library, &second.library, "after opening from the cloud");
    assert_eq!(tag_names(&second.library, root_of(&second.library, &hash(1))), ["a"]);
    second.library.apply_tags(&one(root_of(&second.library, &hash(2))), &["c".into()], true).unwrap();
    settle(&[first, second], "after a change of the second library");

    // A library with content is not joined with the cloud library.
    let full = Device::create(directory.path(), "full");
    full.library.ingest(&prepared(hash(3), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
    assert!(full.library.open_from_cloud(&cloud, &library_id, &full.computer).is_err());
    assert!(!full.library.sync_on());
}

/// The files of the cloud library: the snapshot ids, and for each device
/// the sequence numbers of its change files.
fn cloud_files(cloud: &Path, library_id: &str) -> (Vec<String>, Vec<Vec<i64>>) {
    let root = cloud.join("picto").join(library_id);
    let names = |path: PathBuf| -> Vec<String> {
        let mut names = std::fs::read_dir(path)
            .map(|listing| listing.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>())
            .unwrap_or_default();
        names.sort();
        names
    };
    let changes = names(root.join("devices"))
        .into_iter()
        .map(|device| {
            let mut seqs = names(root.join("devices").join(device).join("changes"))
                .iter()
                .filter_map(|name| name.strip_suffix(".jsonl.zst")?.parse().ok())
                .collect::<Vec<i64>>();
            seqs.sort();
            seqs
        })
        .collect();
    (names(root.join("snapshots")), changes)
}

#[test]
fn after_the_cleanup_every_library_ends_the_same_and_one_away_for_weeks_brings_nothing_deleted_back() {
    crate::sync::TIME_SHIFT_MS.with(|shift| shift.set(0));
    let directory = tempfile::tempdir().unwrap();
    let cloud = directory.path().join("cloud");
    std::fs::create_dir(&cloud).unwrap();
    let first = Device::create(directory.path(), "first");
    for n in 1..=3 {
        first.library.ingest(&prepared(hash(n), Lifecycle::Active, vec!["a".into(), "b".into()], Vec::new())).unwrap();
    }
    first.library.ingest(&prepared(hash(4), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
    let library_id = first.turn_on(&cloud);
    let second = Device::open(directory.path(), "second", &cloud, &library_id);
    let away = Device::open(directory.path(), "away", &cloud, &library_id);
    let (first_snapshot, _) = cloud_files(&cloud, &library_id);

    // The first library deletes item 1 and the tag `a`, and renames `b` to
    // `d`. The library that is away does not see that, and changes item 1
    // and tags item 4 with `b` in the meantime.
    let root = root_of(&first.library, &hash(1));
    first.library.set_lifecycle(&one(root), Lifecycle::Trash).unwrap();
    first.library.permanently_delete(&one(root), 0, 0, false).unwrap();
    first.library.delete_tag(first.tag("a")).unwrap();
    first.library.rename_tag(first.tag("b"), "d").unwrap();
    first.sync("the first library sends its changes");
    second.sync("the second library reads them");
    away.library.set_rating(&one(root_of(&away.library, &hash(1))), Rating::Five).unwrap();
    away.library.apply_tags(&one(root_of(&away.library, &hash(4))), &["b".into()], true).unwrap();
    away.library.record_sync_changes().unwrap();

    // Forty days later the first library writes a snapshot, which leaves out
    // the fields of item 1 and of `a`, and moves `b` to `d`. Eight days after
    // that the first library writes another snapshot, because the newest one
    // is more than seven days old, and the cleanup deletes the first snapshot
    // and the change files that the others cover.
    move_time(40);
    first.sync("the sync that writes a snapshot");
    move_time(8);
    first.sync("the sync that deletes the old files");
    second.sync("the second library adopts the snapshot");
    let (snapshots, changes) = cloud_files(&cloud, &library_id);
    assert_eq!(snapshots.len(), 2);
    assert!(!snapshots.contains(&first_snapshot[0]), "the covered snapshot is deleted");
    assert!(changes.iter().flatten().count() <= 1, "the covered change files are deleted: {changes:?}");

    // The library that was away catches up through the snapshot. Its changes
    // to the deleted item do not bring it back, and its tag `b` ends under `d`.
    let devices = [first, second, away];
    settle(&devices, "after the library that was away synced");
    for device in &devices {
        let snapshot = device.library.projections().snapshot();
        assert!(!snapshot.tag_ids_by_name.contains_key("a") && !snapshot.tag_ids_by_name.contains_key("b"));
        assert_eq!(tag_names(&device.library, root_of(&device.library, &hash(3))), ["d"]);
        assert_eq!(tag_names(&device.library, root_of(&device.library, &hash(4))), ["d"]);
        assert_eq!(roots(&device.library).len(), 3, "item 1 stays deleted");
    }
}
/// Copies every file in `from` into `to`, which is made.
fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &to.join(entry.file_name()));
        } else {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn a_library_copied_to_another_computer_takes_a_new_device_id_and_both_end_the_same() {
    let directory = tempfile::tempdir().unwrap();
    let cloud = directory.path().join("cloud");
    std::fs::create_dir(&cloud).unwrap();
    let first = Device::create(directory.path(), "first");
    let library_id = first.turn_on(&cloud);
    let other = Device::open(directory.path(), "other", &cloud, &library_id);
    first.library.ingest(&prepared(hash(1), Lifecycle::Active, vec!["a".into()], Vec::new())).unwrap();
    first.library.ingest(&prepared(hash(2), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
    first.sync("the first sync");
    other.sync("the first sync of the other library");

    // The library folder is copied while the library is closed, and the
    // copy is opened on another computer.
    let Device { library, path, computer: first_computer } = first;
    let original_id = library.sync_device_id().unwrap();
    drop(library);
    copy_folder(&directory.path().join("first"), &directory.path().join("copy"));
    let first = Device { library: Library::open(&path).unwrap(), path, computer: first_computer };
    let copy = Device {
        library: Library::open(directory.path().join("copy").join("library.sqlite")).unwrap(),
        path: directory.path().join("copy").join("library.sqlite"),
        computer: computer("copy"),
    };

    // Both change the library before the copy syncs for the first time.
    first.library.set_rating(&one(root_of(&first.library, &hash(1))), Rating::Four).unwrap();
    copy.library.apply_tags(&one(root_of(&copy.library, &hash(2))), &["b".into()], true).unwrap();
    assert!(!first.sync("the sync of the original").new_device);
    assert!(copy.sync("the first sync of the copy").new_device);
    assert_eq!(first.library.sync_device_id().unwrap(), original_id);
    assert_ne!(copy.library.sync_device_id().unwrap(), original_id);
    let devices = [first, copy, other];
    settle(&devices, "the syncs after the copy");
    for device in &devices {
        assert!(device.library.projections().snapshot().rating(Rating::Four).contains(root_of(&device.library, &hash(1)).0));
        assert_eq!(tag_names(&device.library, root_of(&device.library, &hash(2))), ["b"]);
    }
}

#[test]
fn a_restored_older_copy_of_a_library_is_found_by_the_snapshot_that_covers_more_of_it() {
    crate::sync::TIME_SHIFT_MS.with(|shift| shift.set(0));
    let directory = tempfile::tempdir().unwrap();
    let cloud = directory.path().join("cloud");
    std::fs::create_dir(&cloud).unwrap();
    let first = Device::create(directory.path(), "first");
    let library_id = first.turn_on(&cloud);
    let other = Device::open(directory.path(), "other", &cloud, &library_id);
    first.library.ingest(&prepared(hash(1), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
    first.sync("the sync before the backup");

    // A backup of the library folder, taken while the library is closed.
    let first = first.reopen_after(|| copy_folder(&directory.path().join("first"), &directory.path().join("backup")));
    let original_id = first.library.sync_device_id().unwrap();
    for n in 2..=3 {
        first.library.ingest(&prepared(hash(n), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
        first.sync("a sync after the backup");
    }
    // A snapshot covers the change files written after the backup, and the
    // cleanup deletes them, so only the snapshot shows that the backup is
    // older than the library.
    move_time(8);
    first.sync("the sync that writes a snapshot");
    move_time(8);
    first.sync("the sync that deletes the old files");
    other.sync("the other library adopts the snapshot");
    let (_, changes) = cloud_files(&cloud, &library_id);
    assert!(changes.iter().flatten().next().is_none(), "the covered change files are deleted: {changes:?}");

    // The backup is restored in place of the library.
    let Device { library, path, computer: first_computer } = first;
    drop(library);
    std::fs::remove_dir_all(directory.path().join("first")).unwrap();
    copy_folder(&directory.path().join("backup"), &directory.path().join("first"));
    let restored = Device { library: Library::open(&path).unwrap(), path, computer: first_computer };
    assert!(restored.sync("the first sync of the restored backup").new_device);
    assert_ne!(restored.library.sync_device_id().unwrap(), original_id);
    let devices = [restored, other];
    settle(&devices, "after the restored backup synced");
    assert_eq!(roots(&devices[0].library).len(), 3, "the restored library has the items added after the backup");
}
