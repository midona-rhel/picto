//! Checks of libraries that sync through a cloud folder. A folder in a
//! temporary directory stands in for the folder of the Google Drive or
//! Dropbox app.

use std::path::{Path, PathBuf};

use super::*;
use crate::sync::cloud::SyncStop;
use crate::{SyncComputer, SyncReport};

const LIBRARY_ID: &str = "6f1c2a3e-8d4b-4f6a-9b2c-1d3e5f7a9b0c";

fn computer(machine: &str) -> SyncComputer {
    SyncComputer { machine_id: machine.into(), name: format!("computer {machine}"), app_version: "test".into() }
}

/// One copy of the library on one computer.
struct Device {
    library: Library,
    path: PathBuf,
    computer: SyncComputer,
}

impl Device {
    /// A new empty library with sync on, in the folder `name` of
    /// `directory`, connected to the cloud library in `cloud`.
    fn new(directory: &Path, name: &str, cloud: &Path) -> Self {
        let library = open(directory, name);
        let computer = computer(name);
        library.connect_sync_folder(cloud, LIBRARY_ID, "library", &computer).unwrap();
        Self { library, path: directory.join(name).join("library.sqlite"), computer }
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
        let Self { library, path, computer } = self;
        drop(library);
        Self { library: Library::open(&path).unwrap(), path, computer }
    }
}

/// `count` libraries make random changes and sync at random times through
/// one cloud folder. Some syncs stop as a crash would stop them: after a
/// change file is written and before the outbox is cleared, or after a
/// change file of another device is merged and before that is committed.
/// At the end the libraries sync one after another until a round of syncs
/// reads and writes no change file, and must then be the same library,
/// with the same orders.
fn sync_through_a_folder(count: usize, seed: u64) {
    let directory = tempfile::tempdir().unwrap();
    let cloud = directory.path().join("cloud");
    std::fs::create_dir(&cloud).unwrap();
    let mut devices = (0..count)
        .map(|index| Device::new(directory.path(), &format!("library-{index}"), &cloud))
        .collect::<Vec<_>>();
    let mut random = Random(seed);
    let (mut next_hash, mut time) = (0, 0);
    let mut log = Vec::new();
    for step in 0..300 {
        let index = random.below(count);
        let context = format!("seed {seed} at step {step} after {log:?}");
        match random.below(12) {
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
            _ => {
                let operation = operate(&devices[index].library, &mut random, &mut next_hash, &mut time);
                log.push(format!("{step}: {index} {operation}"));
                if random.below(2) == 0 {
                    assert_recorded(&devices[index].library, &context);
                }
            }
        }
    }
    let context = format!("seed {seed} at the end after {log:?}");
    let mut rounds = 0;
    loop {
        let moved = devices
            .iter()
            .map(|device| {
                let report = device.sync(&context);
                report.files_read + report.files_written
            })
            .sum::<usize>();
        if moved == 0 {
            break;
        }
        rounds += 1;
        assert!(rounds <= count + 1, "{context}: the syncs keep reading or writing change files");
    }
    for index in 1..count {
        assert_same(&devices[0].library, &devices[index].library, &format!("library {index}, {context}"));
    }
}

#[test]
fn two_and_three_libraries_that_sync_through_a_cloud_folder_end_the_same() {
    for seed in 1..=5u64 {
        sync_through_a_folder(2, seed * 15_485_863);
        sync_through_a_folder(3, seed * 32_452_843);
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
    let first = Device::new(directory.path(), "first", &cloud);
    let other = Device::new(directory.path(), "other", &cloud);
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
    for _ in 0..2 {
        for device in [&first, &copy, &other] {
            device.sync("the syncs after the copy");
        }
    }
    for device in [&first, &copy, &other] {
        assert!(device.library.projections().snapshot().rating(Rating::Four).contains(root_of(&device.library, &hash(1)).0));
        assert_eq!(tag_names(&device.library, root_of(&device.library, &hash(2))), ["b"]);
    }
    assert_same(&first.library, &copy.library, "the original and the copy");
    assert_same(&first.library, &other.library, "the original and the other library");
}
