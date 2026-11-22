//! Tests of cloud sync with two libraries that stand for two devices. Both
//! sync through a plain folder that stands in for the folder of the
//! provider's desktop app. The second library is made from the first one's
//! newest snapshot, as Open from Cloud… makes it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use picto_library::database::WorkPriority;
use picto_library::selection::SelectionTarget;
use picto_library::{Lifecycle, RootId};

use super::provider::DirectoryProvider;
use super::*;

fn write(application: &LibraryApplication, sql: &str, values: &[&str]) {
    application
        .library()
        .database()
        .maintenance_write(WorkPriority::Cloud, |transaction| {
            transaction.execute(sql, rusqlite::params_from_iter(values))?;
            Ok(())
        })
        .unwrap();
}

fn text(application: &LibraryApplication, sql: &str) -> Option<String> {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            use rusqlite::OptionalExtension;
            Ok(connection
                .query_row(sql, [], |row| row.get::<_, Option<String>>(0))
                .optional()?
                .flatten())
        })
        .unwrap()
}

fn count(application: &LibraryApplication, sql: &str) -> i64 {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row(sql, [], |row| row.get(0))?)
        })
        .unwrap()
}

/// The stable key of the item that `image_job` makes for `index`.
fn key(index: u8) -> String {
    format!("cloud-root-{index}")
}

fn root(application: &LibraryApplication, index: u8) -> Option<RootId> {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            use rusqlite::OptionalExtension;
            Ok(connection
                .query_row(
                    "SELECT root.root_id FROM library_root root
                     JOIN library_item item ON item.local_id = root.root_id
                     WHERE item.stable_key = ?1",
                    [key(index)],
                    |row| row.get::<_, u32>(0),
                )
                .optional()?)
        })
        .unwrap()
        .map(RootId)
}

fn target(application: &LibraryApplication, indexes: &[u8]) -> SelectionTarget {
    SelectionTarget::Explicit {
        root_ids: indexes
            .iter()
            .map(|index| root(application, *index).unwrap())
            .collect(),
    }
}

/// Returns the item's status, or `None` when the library does not hold it.
fn status(application: &LibraryApplication, index: u8) -> Option<Lifecycle> {
    let root = root(application, index)?;
    Some(application.details(root).unwrap().lifecycle)
}

/// Returns the content hash of the item's file.
fn hash(application: &LibraryApplication, index: u8) -> String {
    text(
        application,
        &format!(
            "SELECT file.content_hash FROM library_item item
             JOIN media_item media ON media.media_id = item.local_id
             JOIN media_file file ON file.file_id = media.file_id
             WHERE item.stable_key = '{}'",
            key(index)
        ),
    )
    .unwrap()
}

/// Returns the content of every file below `folder`, by its path inside
/// `folder` written with `/`. The files of a library's database
/// (`library.sqlite` and the two files beside it) are left out: Windows does
/// not let them be read while the library that holds them is open.
fn files_below(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut waiting = vec![folder.to_path_buf()];
    while let Some(directory) = waiting.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            let database = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("library.sqlite"));
            if path.is_dir() {
                waiting.push(path);
            } else if !database {
                let name = path
                    .strip_prefix(folder)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(name, std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

/// Returns every row of every table of the library's database, except the
/// tables in `left_out`.
fn rows(application: &LibraryApplication, left_out: &[&str]) -> BTreeMap<String, Vec<String>> {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            let tables = connection
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut all = BTreeMap::new();
            for table in tables {
                if left_out.contains(&table.as_str()) {
                    continue;
                }
                let mut statement = connection.prepare(&format!("SELECT * FROM \"{table}\""))?;
                let columns = statement.column_count();
                let rows = statement
                    .query_map([], |row| {
                        (0..columns)
                            .map(|column| {
                                row.get::<_, rusqlite::types::Value>(column)
                                    .map(|value| format!("{value:?}"))
                            })
                            .collect::<rusqlite::Result<Vec<_>>>()
                            .map(|values| values.join("|"))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                all.insert(table, rows);
            }
            Ok(all)
        })
        .unwrap()
}

/// Two devices that sync one library through the folder `cloud`, and that
/// folder as a provider. Both hold the items 0, 1 and 2 in All, with their
/// files. Item 0 has the tag `sync:tag`, and item 1 is in the folder
/// `Folder`. Both have synced, and the cloud folder holds no deletion file.
/// The two folders that hold the libraries are returned to keep them.
async fn two_devices(
    cloud: &Path,
) -> (
    LibraryApplication,
    LibraryApplication,
    DirectoryProvider,
    [tempfile::TempDir; 2],
) {
    let folders = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let first = LibraryApplication::create(folders[0].path().join("First.library")).unwrap();
    for index in 0..3 {
        first
            .library()
            .enqueue_ingest_job(
                &super::tests::image_job(folders[0].path(), index),
                "2026-08-28T12:00:00Z",
            )
            .unwrap();
    }
    crate::library_ingest_runtime::run_batch(&first, 64).unwrap();
    first
        .set_lifecycle(&target(&first, &[0, 1, 2]), Lifecycle::Active)
        .unwrap();
    first
        .apply_tags(&target(&first, &[0]), &["sync:tag".to_string()], true)
        .unwrap();
    let (folder, _) = first.library().create_folder("Folder", None).unwrap();
    first
        .library()
        .add_to_folder(&target(&first, &[1]), folder)
        .unwrap();
    let cloud_text = cloud.to_string_lossy().into_owned();
    write(
        &first,
        "UPDATE cloud_state SET provider = 'dropbox', account_label = 'Test',
                remote_root = ?1, state = 'idle', paused = 0",
        &[&cloud_text],
    );
    let provider = DirectoryProvider::open(cloud).unwrap();
    let manifest = snapshot::publish_library(&first, &provider).await.unwrap();

    // Open from Cloud… on the second device.
    let second_root = folders[1].path().join("Second.library");
    let joined = snapshot::prepare_join(&provider, &manifest.library_id, &second_root)
        .await
        .unwrap();
    std::fs::create_dir_all(second_root.join("blobs")).unwrap();
    let database = second_root.join("library.sqlite");
    std::fs::rename(&joined.database_path, &database).unwrap();
    crate::state::initialize_joined_database(&database, "dropbox", "Test", &cloud_text).unwrap();
    let second = LibraryApplication::open(&second_root).unwrap();
    snapshot::publish_library(&second, &provider).await.unwrap();
    assert_eq!(count(&second, "SELECT COUNT(*) FROM library_root"), 3);
    assert_eq!(count(&second, "SELECT COUNT(*) FROM cloud_journal"), 0);
    for index in 0..3 {
        let original = second
            .blobs()
            .original_path_with_ext(&hash(&second, index), Some("png"))
            .unwrap();
        assert!(original.is_file(), "the second device has the file of {index}");
    }
    (first, second, provider, folders)
}

/// The folder of the library's data in the cloud folder.
fn cloud_data(cloud: &Path, application: &LibraryApplication) -> PathBuf {
    cloud
        .join("picto")
        .join(configuration_library(application).unwrap().library_id)
}

/// The deletion file `sequence` that `application` wrote.
fn deletion_file(cloud: &Path, application: &LibraryApplication, sequence: u32) -> PathBuf {
    let configuration = configuration_library(application).unwrap();
    cloud_data(cloud, application)
        .join("deletions")
        .join(configuration.device_id)
        .join(format!("{sequence:010}.jsonl.zst"))
}

fn cloud_original(cloud: &Path, application: &LibraryApplication, hash: &str) -> PathBuf {
    cloud_data(cloud, application)
        .join("blobs/f")
        .join(&hash[..2])
        .join(&hash[2..4])
        .join(format!("{hash}.png"))
}

#[tokio::test]
async fn a_deletion_on_one_device_arrives_on_the_other_and_a_restore_comes_back() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, second, provider, _folders) = two_devices(cloud.path()).await;
    let deleted_hash = hash(&second, 1);
    let tag = first.library().projections().snapshot().tag_ids_by_name["sync:tag"];
    let folder = first.library().folders().unwrap()[0].folder_id;

    first
        .set_lifecycle(&target(&first, &[0, 1]), Lifecycle::Trash)
        .unwrap();
    first.delete_items(&target(&first, &[1]), false).unwrap();
    first.delete_tag(tag).unwrap();
    first.delete_folders(&[folder]).unwrap();
    snapshot::publish_library(&first, &provider).await.unwrap();
    assert!(deletion_file(cloud.path(), &first, 1).is_file());

    let received = receive_library(&second, &provider).await.unwrap();

    assert_eq!(received.files, 1);
    assert_eq!(status(&second, 0), Some(Lifecycle::Trash));
    assert_eq!(status(&second, 1), None);
    assert_eq!(status(&second, 2), Some(Lifecycle::Active));
    assert!(!second
        .library()
        .projections()
        .snapshot()
        .tag_ids_by_name
        .contains_key("sync:tag"));
    assert!(second.library().folders().unwrap().is_empty());
    assert_eq!(status_library(&second).unwrap().state, "idle");
    // Nothing of it is on the second device's undo stack.
    assert!(!second.library().history().state().can_undo);
    // The file of the deleted item leaves the second device's library.
    crate::library_media_runtime::drain_blob_cleanup(&second, 8).unwrap();
    let original = second
        .blobs()
        .original_path_with_ext(&deleted_hash, Some("png"))
        .unwrap();
    assert!(!original.exists());
    assert_eq!(count(&second, "SELECT COUNT(*) FROM media_file"), 2);

    // The second device restores the item, and the first takes that over.
    second
        .set_lifecycle(&target(&second, &[0]), Lifecycle::Active)
        .unwrap();
    snapshot::publish_library(&second, &provider).await.unwrap();
    assert_eq!(receive_library(&first, &provider).await.unwrap().files, 1);
    assert_eq!(status(&first, 0), Some(Lifecycle::Active));
    assert_eq!(status(&first, 2), Some(Lifecycle::Active));
    assert_eq!(count(&first, "SELECT COUNT(*) FROM library_root"), 2);
    assert_eq!(count(&second, "SELECT COUNT(*) FROM library_root"), 2);
}

#[tokio::test]
async fn the_original_of_a_deleted_item_leaves_the_cloud_folder_when_its_keep_time_is_over() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, second, provider, _folders) = two_devices(cloud.path()).await;
    let deleted = cloud_original(cloud.path(), &first, &hash(&first, 1));
    let kept = cloud_original(cloud.path(), &first, &hash(&first, 2));
    assert!(deleted.is_file() && kept.is_file());
    // "Keep deleted files" is 0 days, and the deletion cannot be undone, so
    // nothing holds the file back.
    update_retention_library(&first, &serde_json::json!({"deleted_blobs_days": 0})).unwrap();
    first
        .set_lifecycle(&target(&first, &[1]), Lifecycle::Trash)
        .unwrap();
    first
        .library()
        .permanently_delete(
            &target(&first, &[1]),
            chrono::Utc::now().timestamp_millis() - 1_000,
            0,
            false,
        )
        .unwrap();
    crate::library_media_runtime::drain_blob_cleanup(&first, 8).unwrap();

    snapshot::publish_library(&first, &provider).await.unwrap();
    assert!(!deleted.exists());
    assert!(kept.is_file());

    // The second device deletes the item too and does not upload its file
    // again.
    snapshot::publish_library(&second, &provider).await.unwrap();
    crate::library_media_runtime::drain_blob_cleanup(&second, 8).unwrap();
    snapshot::publish_library(&second, &provider).await.unwrap();
    assert_eq!(status(&second, 1), None);
    assert!(!deleted.exists());
    assert!(kept.is_file());
}

#[tokio::test]
async fn a_device_that_was_away_takes_the_deletions_over_and_keeps_what_it_added() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, second, provider, folders) = two_devices(cloud.path()).await;
    // While the second device is away, the first deletes item 1 in two
    // syncs. The second imports item 7 meanwhile.
    first
        .set_lifecycle(&target(&first, &[1]), Lifecycle::Trash)
        .unwrap();
    snapshot::publish_library(&first, &provider).await.unwrap();
    first.delete_items(&target(&first, &[1]), false).unwrap();
    snapshot::publish_library(&first, &provider).await.unwrap();
    second
        .library()
        .enqueue_ingest_job(
            &super::tests::image_job(folders[1].path(), 7),
            "2026-08-29T12:00:00Z",
        )
        .unwrap();
    crate::library_ingest_runtime::run_batch(&second, 64).unwrap();

    // The second device comes back and syncs. Then the first syncs.
    snapshot::publish_library(&second, &provider).await.unwrap();
    snapshot::publish_library(&first, &provider).await.unwrap();

    assert_eq!(status(&second, 1), None);
    assert_eq!(status(&first, 1), None);
    assert_eq!(status(&second, 7), Some(Lifecycle::Inbox));
    assert_eq!(count(&second, "SELECT COUNT(*) FROM library_root"), 3);
    // The first device lost nothing but the item it deleted.
    assert_eq!(count(&first, "SELECT COUNT(*) FROM library_root"), 2);
    assert_eq!(status(&first, 0), Some(Lifecycle::Active));
    assert_eq!(status(&first, 2), Some(Lifecycle::Active));
}

#[tokio::test]
async fn a_damaged_or_missing_deletion_file_deletes_nothing_and_is_reported() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, second, provider, _folders) = two_devices(cloud.path()).await;
    first
        .set_lifecycle(&target(&first, &[0]), Lifecycle::Trash)
        .unwrap();
    snapshot::publish_library(&first, &provider).await.unwrap();
    first.delete_items(&target(&first, &[0]), false).unwrap();
    first
        .set_lifecycle(&target(&first, &[1]), Lifecycle::Trash)
        .unwrap();
    snapshot::publish_library(&first, &provider).await.unwrap();
    let file = deletion_file(cloud.path(), &first, 1);
    let whole = std::fs::read(&file).unwrap();
    assert!(deletion_file(cloud.path(), &first, 2).is_file());
    let unchanged = |application: &LibraryApplication| {
        assert_eq!(count(application, "SELECT COUNT(*) FROM library_root"), 3);
        for index in 0..3 {
            assert_eq!(status(application, index), Some(Lifecycle::Active));
        }
    };

    // The first file was written only in part. The second file is whole,
    // and it is not applied either.
    std::fs::write(&file, &whole[..whole.len() / 2]).unwrap();
    let error = snapshot::publish_library(&second, &provider)
        .await
        .unwrap_err();
    assert!(error.contains("0000000001.jsonl.zst"), "{error}");
    assert!(error.contains("Nothing was deleted on this device"), "{error}");
    let state = status_library(&second).unwrap();
    assert_eq!((state.state.as_str(), state.message), ("error", error));
    unchanged(&second);

    // The first file is missing and the second is there.
    std::fs::remove_file(&file).unwrap();
    snapshot::publish_library(&second, &provider).await.unwrap();
    let state = status_library(&second).unwrap();
    assert_eq!(
        (state.state.as_str(), state.message.as_str()),
        ("idle", WAITING_FOR_DELETIONS)
    );
    unchanged(&second);

    // The whole first file arrives. Both files are applied in order.
    std::fs::write(&file, &whole).unwrap();
    assert_eq!(receive_library(&second, &provider).await.unwrap().files, 2);
    assert_eq!(status(&second, 0), None);
    assert_eq!(status(&second, 1), Some(Lifecycle::Trash));
    assert_eq!(status(&second, 2), Some(Lifecycle::Active));
    assert_eq!(status_library(&second).unwrap().message, "");
}

#[tokio::test]
async fn delete_cloud_data_removes_only_what_the_sync_wrote_and_leaves_the_library_as_it_is() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, second, provider, _folders) = two_devices(cloud.path()).await;
    first
        .set_lifecycle(&target(&first, &[0]), Lifecycle::Trash)
        .unwrap();
    snapshot::publish_library(&first, &provider).await.unwrap();
    snapshot::publish_library(&second, &provider).await.unwrap();
    let data = cloud_data(cloud.path(), &first);
    assert!(deletion_file(cloud.path(), &first, 1).is_file());
    // An upload that was interrupted left a file of the sync behind.
    std::fs::write(
        data.join("snapshots")
            .join(format!("x.picto-uploading-{}", uuid::Uuid::new_v4())),
        b"part",
    )
    .unwrap();
    // Files that the sync did not write, in and beside the library's data.
    let foreign = [
        "notes.txt",
        "snapshots/readme.md",
        "blobs/f/aa/bb/picture.txt",
        "deletions/not-a-device/0000000001.jsonl.zst",
        "extra/file.bin",
    ];
    for name in foreign {
        std::fs::create_dir_all(data.join(name).parent().unwrap()).unwrap();
        std::fs::write(data.join(name), b"not written by the sync").unwrap();
    }
    let other_library = cloud.path().join("picto").join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(other_library.join("snapshots")).unwrap();
    std::fs::write(other_library.join("library.json"), b"{}").unwrap();
    std::fs::write(cloud.path().join("picto/beside.txt"), b"beside").unwrap();
    std::fs::write(cloud.path().join("Holiday.jpg"), b"the user's own file").unwrap();

    // The library's own files and rows before the removal. The database
    // is compared by its rows, because switching sync off changes the row of
    // `cloud_state` and the revision in `library_meta`.
    let library_files = |application: &LibraryApplication| files_below(application.root());
    let files_before = library_files(&first);
    let rows_before = rows(&first, &["cloud_state", "library_meta"]);
    assert!(files_before.keys().any(|name| name.starts_with("blobs/")));

    let removal = delete_cloud_data_library(&first).await.unwrap();

    // The result names each entry that was left: a file, or a folder that
    // the sync does not know and therefore did not look into.
    assert!(!removal.folder_removed);
    let mut kept = removal.kept.clone();
    kept.sort();
    assert_eq!(
        kept,
        [
            "blobs/f/aa/bb/picture.txt",
            "deletions/not-a-device",
            "extra",
            "notes.txt",
            "snapshots/readme.md",
        ]
    );
    assert_eq!(removal.kept_entries, 5);
    // The library's folder in the cloud holds exactly the foreign files.
    let mut expected = foreign.map(str::to_owned).to_vec();
    expected.sort();
    assert_eq!(
        files_below(&data).into_keys().collect::<Vec<_>>(),
        expected
    );
    // Nothing outside the library's folder was touched.
    assert_eq!(std::fs::read(other_library.join("library.json")).unwrap(), b"{}");
    assert!(other_library.join("snapshots").is_dir());
    assert_eq!(std::fs::read(cloud.path().join("picto/beside.txt")).unwrap(), b"beside");
    assert!(cloud.path().join("Holiday.jpg").is_file());

    // Sync is off, and the library on the device is as it was.
    let configuration = configuration_library(&first).unwrap();
    assert_eq!(configuration.provider, None);
    assert_eq!(configuration.root_path, None);
    assert_eq!(status_library(&first).unwrap().state, "disabled");
    assert_eq!(library_files(&first), files_before);
    assert_eq!(rows(&first, &["cloud_state", "library_meta"]), rows_before);
    assert_eq!(status(&first, 0), Some(Lifecycle::Trash));
    assert_eq!(count(&first, "SELECT COUNT(*) FROM library_root"), 3);
}

#[tokio::test]
async fn delete_cloud_data_removes_the_whole_folder_when_only_the_sync_wrote_into_it() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, _second, _provider, _folders) = two_devices(cloud.path()).await;
    let data = cloud_data(cloud.path(), &first);
    assert!(data.join("library.json").is_file());

    let removal = delete_cloud_data_library(&first).await.unwrap();

    assert!(removal.folder_removed);
    assert_eq!((removal.kept_entries, removal.kept.len()), (0, 0));
    assert!(removal.removed_files >= 8, "{removal:?}");
    assert!(!data.exists());
    assert!(cloud.path().join("picto").is_dir());
    // A second removal finds nothing and removes nothing.
    write(
        &first,
        "UPDATE cloud_state SET provider = 'dropbox', remote_root = ?1",
        &[&cloud.path().to_string_lossy()],
    );
    let again = delete_cloud_data_library(&first).await.unwrap();
    assert_eq!((again.removed_files, again.folder_removed), (0, true));
}

#[tokio::test]
async fn cloud_data_of_another_library_or_behind_a_link_is_not_removed() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, _second, provider, _folders) = two_devices(cloud.path()).await;
    let data = cloud_data(cloud.path(), &first);
    let before = files_below(&data);

    // The folder has this library's name and another library's manifest.
    let manifest = std::fs::read(data.join("library.json")).unwrap();
    std::fs::write(
        data.join("library.json"),
        serde_json::json!({ "library_id": uuid::Uuid::new_v4().to_string() }).to_string(),
    )
    .unwrap();
    let error = delete_cloud_data_library(&first).await.unwrap_err();
    assert!(error.contains("nothing was removed"), "{error}");
    std::fs::write(data.join("library.json"), manifest).unwrap();
    assert_eq!(files_below(&data), before);
    assert!(configuration_library(&first).unwrap().provider.is_some());

    // The library's folder is a link to another folder.
    #[cfg(unix)]
    {
        let elsewhere = tempfile::tempdir().unwrap();
        let moved = elsewhere.path().join("data");
        std::fs::rename(&data, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &data).unwrap();
        assert!(delete_cloud_data_library(&first).await.is_err());
        assert_eq!(files_below(&moved), before);
        assert!(provider
            .remove_library(&configuration_library(&first).unwrap().library_id)
            .is_err());
    }
}

#[tokio::test]
async fn another_device_notices_that_the_cloud_data_is_gone_and_switches_its_sync_off() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, second, provider, _folders) = two_devices(cloud.path()).await;
    let data = cloud_data(cloud.path(), &second);
    let files_before = files_below(second.root().join("blobs").as_path());
    let rows_before = rows(&second, &["cloud_state", "library_meta"]);
    delete_cloud_data_library(&first).await.unwrap();
    assert!(!data.exists());

    // The worker's check between two snapshots finds the data gone.
    assert!(receive_due_library(&second).unwrap());
    let error = receive_library(&second, &provider).await.unwrap_err();

    assert_eq!(error, CLOUD_DATA_REMOVED);
    let state = status_library(&second).unwrap();
    assert_eq!(
        (state.state.as_str(), state.message.as_str()),
        ("disabled", CLOUD_DATA_REMOVED)
    );
    assert_eq!(configuration_library(&second).unwrap().provider, None);
    assert!(!receive_due_library(&second).unwrap());
    assert!(!snapshot_due_library(&second, i64::MAX, 0).unwrap());
    // Nothing was written into the cloud folder again, and nothing was
    // deleted on the device.
    assert!(!data.exists());
    assert_eq!(files_below(second.root().join("blobs").as_path()), files_before);
    assert_eq!(rows(&second, &["cloud_state", "library_meta"]), rows_before);
    assert_eq!(count(&second, "SELECT COUNT(*) FROM library_root"), 3);
}

#[tokio::test]
async fn a_sync_that_finds_the_cloud_data_gone_writes_nothing_and_switches_off() {
    let cloud = tempfile::tempdir().unwrap();
    let (first, second, provider, _folders) = two_devices(cloud.path()).await;
    let data = cloud_data(cloud.path(), &second);
    delete_cloud_data_library(&first).await.unwrap();
    // The second device has a change that makes a snapshot due.
    second
        .set_lifecycle(&target(&second, &[2]), Lifecycle::Trash)
        .unwrap();

    let error = snapshot::publish_library(&second, &provider)
        .await
        .unwrap_err();

    assert_eq!(error, CLOUD_DATA_REMOVED);
    assert_eq!(status_library(&second).unwrap().state, "disabled");
    assert!(!data.exists());
    assert_eq!(status(&second, 2), Some(Lifecycle::Trash));
}

/// The only test that sets `PICTO_GOOGLE_DRIVE_ROOT`, which `configure_library`
/// reads to accept the folder.
#[tokio::test]
async fn sync_that_is_enabled_again_starts_from_new_cloud_data() {
    let cloud = tempfile::tempdir().unwrap();
    std::env::set_var("PICTO_GOOGLE_DRIVE_ROOT", cloud.path());
    let folder = tempfile::tempdir().unwrap();
    let application = LibraryApplication::create(folder.path().join("Again.library")).unwrap();
    for index in 0..2 {
        application
            .library()
            .enqueue_ingest_job(
                &super::tests::image_job(folder.path(), index),
                "2026-08-28T12:00:00Z",
            )
            .unwrap();
    }
    crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
    let input = ConfigureCloudInput {
        provider: "google_drive".into(),
        account_label: "Test".into(),
        root_path: cloud.path().to_string_lossy().into_owned(),
    };
    configure_library(&application, &input).unwrap();
    let provider = directory_provider_library(&application).unwrap();
    application
        .set_lifecycle(&target(&application, &[0]), Lifecycle::Trash)
        .unwrap();
    snapshot::publish_library(&application, &provider).await.unwrap();
    assert!(deletion_file(cloud.path(), &application, 1).is_file());
    let data = cloud_data(cloud.path(), &application);

    delete_cloud_data_library(&application).await.unwrap();
    assert!(!data.exists());
    // A deletion made while sync is off is not sent later.
    application
        .set_lifecycle(&target(&application, &[1]), Lifecycle::Trash)
        .unwrap();

    configure_library(&application, &input).unwrap();
    assert_eq!(status_library(&application).unwrap().last_sync_at, None);
    snapshot::publish_library(&application, &provider).await.unwrap();
    assert!(data.join("library.json").is_file());
    assert!(!data.join("deletions").exists());

    // The first deletion after that is the new cloud data's file 1.
    application
        .set_lifecycle(&target(&application, &[0]), Lifecycle::Active)
        .unwrap();
    snapshot::publish_library(&application, &provider).await.unwrap();
    assert!(deletion_file(cloud.path(), &application, 1).is_file());
    assert!(!deletion_file(cloud.path(), &application, 2).exists());
}
