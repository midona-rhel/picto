//! Tests of cloud sync with two libraries that stand for two devices. Both
//! sync through a plain folder that stands in for the folder of the
//! provider's desktop app. The second library is made from the first one's
//! newest snapshot, as Open from Cloud… makes it.

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
