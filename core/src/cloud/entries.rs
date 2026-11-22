//! The files in the cloud folder that hold the entries which the devices of
//! a library share, and the copying of the originals that a merged entry
//! names.
//!
//! The library keeps one entry per key: what was done last to an item, a
//! tag, a folder or a smart folder, with the time of the library's hybrid
//! logical clock and the device that did it
//! (`picto-library/src/mutation/shared.rs`). A device writes the entries
//! that it made into its own folder, `picto/<library id>/entries/<device
//! id>/`, and merges the entries in the folders of all devices. The library
//! decides which entry wins. This file reads and writes the files.
//!
//! A device merges its own folder too. Usually that changes nothing, because
//! the library holds those entries. After a snapshot was restored, the
//! library holds older entries than the device wrote, and the merge brings
//! them back.
//!
//! A device's entries are spread over 4096 files by the first 12 bits of the
//! SHA-256 of an entry's kind and key, so that a change rewrites only the
//! files of the keys that changed. A file's name is the three hexadecimal
//! digits of those bits, the first 16 hexadecimal digits of the SHA-256 of
//! its lines, and `.jsonl.zst`. Its content is compressed with zstd: a JSON
//! header that names the library, the device and the three digits and holds
//! the whole SHA-256 of the lines, and then one entry per line. A file is
//! written whole beside its place and renamed into it, and it is never
//! changed: a new state of the same keys is a file with another name, and
//! the file it replaces is removed.
//!
//! No file depends on another one, and merging entries twice or in another
//! order gives the same library. So a device merges whatever files are there
//! and can be read. The library remembers which file of each device it
//! merged last for each three digits (`merged_entry_file`), and reads no
//! file a second time. A file whose content does not match its checksum is
//! not merged, and no file is merged in the round that finds it.
//!
//! # Files that are not on this device yet
//!
//! The desktop app of the provider lists a file before it has downloaded
//! it. While that app is not running, reading such a file waits and then
//! ends with an error; on macOS a file that Google Drive keeps only in the
//! cloud ends with "Operation timed out". Such a file is not delivered yet.
//! The round stops reading at the first one, because each further read
//! would wait as long, merges what it has read, and leaves the rest for a
//! later round. The sync state shows no error.
//!
//! # Originals of merged items
//!
//! An entry that adds an item puts a job into the library's ingest queue
//! that is not prepared. [`fetch_awaited`] copies the originals of such a
//! job from `blobs/` into the library and marks the job prepared, and the
//! ingest worker imports it. An original that is not in the cloud folder,
//! has not its whole size, or cannot be read yet is looked at again a minute
//! later, and the job shows no error. An original of the whole size whose
//! content does not match its hash fails the job.
//!
//! Before this format, a device wrote its entries into
//! `picto/<library id>/tombstones/<device id>/`. A device removes its own
//! folder there when it writes its entries.

use std::collections::{BTreeMap, BTreeSet};

use picto_library::{AwaitedImport, SharedEntry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::provider::{CloudProvider, CloudRead};
use crate::library_application::LibraryApplication;

/// The number of the file format that this version reads and writes.
const FORMAT: u32 = 2;
/// The number of roots whose moves into and out of the Trash are turned
/// into entries in one write transaction.
const SETTLE_LIMIT: usize = 20_000;
/// The number of waiting items whose originals one round looks for.
const AWAITED_PER_ROUND: usize = 2_000;
/// How long an item whose originals were not there waits before a round
/// looks for them again.
const AWAITED_RETRY_SECONDS: i64 = 60;

/// The first line of an entry file.
#[derive(Debug, Serialize, Deserialize)]
struct Header {
    format: u32,
    library_id: String,
    device_id: String,
    /// The three hexadecimal digits that every entry of the file has.
    shard: String,
    /// The number of entries in the file.
    entries: usize,
    /// The SHA-256 of everything after the first line, in hexadecimal.
    sha256: String,
}

/// One file of a device that the library has not merged yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Unmerged {
    device_id: String,
    name: String,
    path: String,
}

fn directory(library_id: &str) -> String {
    format!("picto/{library_id}/entries")
}

/// Returns the three hexadecimal digits of the file that holds `entry`.
fn shard_of(entry: &SharedEntry) -> String {
    let mut hasher = Sha256::new();
    hasher.update(entry.kind.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(entry.key.as_bytes());
    hex::encode(&hasher.finalize()[..2])[..3].to_owned()
}

/// Returns whether `name` is the name of an entry file: three hexadecimal
/// digits, a dot, 16 hexadecimal digits and `.jsonl.zst`.
pub(super) fn is_file_name(name: &str) -> bool {
    let hexadecimal = |text: &str| text.bytes().all(|byte| byte.is_ascii_hexdigit());
    name.strip_suffix(".jsonl.zst")
        .and_then(|stem| stem.split_once('.'))
        .is_some_and(|(shard, digest)| {
            shard.len() == 3 && hexadecimal(shard) && digest.len() == 16 && hexadecimal(digest)
        })
}

/// Returns the name and the bytes of the file that holds `entries`, which
/// are all the entries of `device_id` with the digits `shard`.
fn encode(
    library_id: &str,
    device_id: &str,
    shard: &str,
    entries: &[SharedEntry],
) -> Result<(String, Vec<u8>), String> {
    let mut lines = Vec::new();
    for entry in entries {
        serde_json::to_writer(&mut lines, entry).map_err(|error| error.to_string())?;
        lines.push(b'\n');
    }
    let sha256 = hex::encode(Sha256::digest(&lines));
    let name = format!("{shard}.{}.jsonl.zst", &sha256[..16]);
    let header = Header {
        format: FORMAT,
        library_id: library_id.to_owned(),
        device_id: device_id.to_owned(),
        shard: shard.to_owned(),
        entries: entries.len(),
        sha256,
    };
    let mut text = serde_json::to_vec(&header).map_err(|error| error.to_string())?;
    text.push(b'\n');
    text.extend(lines);
    let bytes = zstd::stream::encode_all(text.as_slice(), 9)
        .map_err(|error| format!("Failed to compress the entry file: {error}"))?;
    Ok((name, bytes))
}

/// Returns the entries of the file `name` of `device_id`. The error says, as
/// the end of a sentence, why the bytes are not that whole file of this
/// library.
fn decode(
    bytes: &[u8],
    library_id: &str,
    device_id: &str,
    name: &str,
) -> Result<Vec<SharedEntry>, String> {
    let text = zstd::stream::decode_all(bytes)
        .map_err(|_| "is damaged or was not written completely".to_string())?;
    let end = text
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| "has no header".to_string())?;
    let header: Header = serde_json::from_slice(&text[..end])
        .map_err(|_| "has a header that cannot be read".to_string())?;
    if header.format != FORMAT {
        return Err("was written by a version of Picto that this version cannot read".into());
    }
    if header.library_id != library_id || header.device_id != device_id {
        return Err("belongs to another library or device".into());
    }
    let lines = &text[end + 1..];
    let sha256 = hex::encode(Sha256::digest(lines));
    if sha256 != header.sha256 || name != format!("{}.{}.jsonl.zst", header.shard, &sha256[..16]) {
        return Err("does not match its checksum".into());
    }
    let entries = lines
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice)
        .collect::<Result<Vec<SharedEntry>, _>>()
        .map_err(|_| "holds an entry that this version of Picto cannot read".to_string())?;
    if entries.len() != header.entries {
        return Err("does not hold the number of entries that its header states".into());
    }
    Ok(entries)
}

/// Writes the entries that this device made into the cloud folder.
///
/// It first turns the device's pending moves into and out of the Trash into
/// entries. Then it rewrites the files of every key whose entry is later
/// than the clock time up to which the device's entries were written before,
/// removes the files that the new ones replace, and records the new clock
/// time. A run that stops in the middle is repeated by the next one and
/// writes the same files. It also removes the folder that an earlier version
/// wrote this device's entries into.
pub(super) async fn publish_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<(), String> {
    let library = application.library();
    while library
        .settle_trash_moves(SETTLE_LIMIT)
        .map_err(|error| error.to_string())?
    {}
    let clock = library.entry_clock().map_err(|error| error.to_string())?;
    remove_earlier_folder(provider, library_id, &clock.device_id).await?;
    let mut changed = BTreeSet::new();
    library
        .visit_own_entries(clock.published, &mut |entry| {
            changed.insert(shard_of(&entry));
        })
        .map_err(|error| error.to_string())?;
    if changed.is_empty() {
        return Ok(());
    }
    let mut files = changed
        .into_iter()
        .map(|shard| (shard, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    library
        .visit_own_entries(None, &mut |entry| {
            if let Some(entries) = files.get_mut(&shard_of(&entry)) {
                entries.push(entry);
            }
        })
        .map_err(|error| error.to_string())?;
    let folder = format!("{}/{}", directory(library_id), clock.device_id);
    let existing = provider.list(&folder).await?;
    for (shard, entries) in files {
        let (name, bytes) = encode(library_id, &clock.device_id, &shard, &entries)?;
        let path = format!("{folder}/{name}");
        if !provider.exists(&path).await? {
            let checksum = hex::encode(Sha256::digest(&bytes));
            provider.upload(&path, bytes, &checksum).await?;
        }
        let replaced = format!("{folder}/{shard}.");
        for object in &existing {
            if object.path.starts_with(&replaced) && object.path != path {
                provider.delete(&object.path).await?;
            }
        }
        // The file that was just written holds what the library holds, so
        // it does not have to be merged.
        library
            .set_entry_file_merged(&clock.device_id, &shard, &name)
            .map_err(|error| error.to_string())?;
    }
    library
        .set_entries_published(clock.now)
        .map_err(|error| error.to_string())
}

/// Removes the files that an earlier version wrote for this device into
/// `picto/<library id>/tombstones/<device id>/`, then that folder, and then
/// `tombstones/` when nothing is left in it. The entries that those files
/// held are entries of the library, and this version writes them into
/// `entries/`.
async fn remove_earlier_folder(
    provider: &dyn CloudProvider,
    library_id: &str,
    device_id: &str,
) -> Result<(), String> {
    let earlier = format!("picto/{library_id}/tombstones");
    let folder = format!("{earlier}/{device_id}");
    for object in provider.list(&folder).await? {
        let name = object.path.rsplit('/').next().unwrap_or_default();
        if super::provider::is_tombstone_file(name) {
            provider.delete(&object.path).await?;
        }
    }
    provider.remove_empty_folder(&folder).await?;
    provider.remove_empty_folder(&earlier).await
}

/// Returns the three digits at the start of the name of an entry file.
fn shard_of_name(name: &str) -> &str {
    name.split('.').next().unwrap_or_default()
}

/// Returns the entry files of all devices that the library has not merged
/// yet.
pub(super) async fn unmerged_files(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<Vec<Unmerged>, String> {
    let merged = application
        .library()
        .merged_entry_files()
        .map_err(|error| error.to_string())?;
    let mut devices = provider.children(&directory(library_id)).await?;
    devices.sort();
    let mut files = Vec::new();
    for device_id in devices {
        // A device's folder has the device's id as its name. Any other
        // folder was not written by the sync.
        if uuid::Uuid::parse_str(&device_id).is_err() {
            continue;
        }
        let folder = format!("{}/{device_id}", directory(library_id));
        for object in provider.list(&folder).await? {
            let Some(name) = object
                .path
                .strip_prefix(&folder)
                .and_then(|rest| rest.strip_prefix('/'))
            else {
                continue;
            };
            if is_file_name(name) && !merged.contains(&(device_id.clone(), name.to_owned())) {
                files.push(Unmerged {
                    device_id: device_id.clone(),
                    name: name.to_owned(),
                    path: object.path.clone(),
                });
            }
        }
    }
    Ok(files)
}

/// What reading one entry file gave.
enum FileRead {
    Entries(Vec<SharedEntry>),
    /// The file is gone: its device has replaced it meanwhile.
    Replaced,
    /// The file is listed and its content is not on this device yet.
    NotDelivered,
}

/// Reads one file. Returns an error when the file was read and is not a
/// whole entry file.
async fn read(
    provider: &dyn CloudProvider,
    library_id: &str,
    file: &Unmerged,
) -> Result<FileRead, String> {
    let bytes = match provider.read(&file.path).await? {
        CloudRead::Read(bytes) => bytes,
        CloudRead::Missing => return Ok(FileRead::Replaced),
        CloudRead::NotDelivered(reason) => {
            tracing::debug!(path = %file.path, %reason, "An entry file is not delivered yet");
            return Ok(FileRead::NotDelivered);
        }
    };
    decode(&bytes, library_id, &file.device_id, &file.name)
        .map(FileRead::Entries)
        .map_err(|reason| {
            format!(
                "The entry file {} in the cloud folder {reason}. \
                 Nothing was merged on this device.",
                file.path
            )
        })
}

/// Merges `files` into the library and returns the number of files that
/// were merged. Every file is read and checked first. If one of them is
/// damaged, none is merged and the error names it. The first file that is
/// not delivered yet ends the reading, and that file and the files after it
/// are left for a later round.
pub(super) async fn merge_files(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
    files: &[Unmerged],
) -> Result<u64, String> {
    let mut readable = files.len();
    for (index, file) in files.iter().enumerate() {
        if matches!(read(provider, library_id, file).await?, FileRead::NotDelivered) {
            readable = index;
            break;
        }
    }
    let mut merged = 0;
    for file in &files[..readable] {
        let FileRead::Entries(entries) = read(provider, library_id, file).await? else {
            continue;
        };
        let library = application.library();
        library
            .apply_shared_entries(&file.device_id, &entries)
            .map_err(|error| error.to_string())?;
        library
            .set_entry_file_merged(&file.device_id, shard_of_name(&file.name), &file.name)
            .map_err(|error| error.to_string())?;
        merged += 1;
    }
    Ok(merged)
}

/// Merges the entry files that the library has not merged yet
/// ([`unmerged_files`], [`merge_files`]), and copies the originals of the
/// items that wait for them ([`fetch_awaited`]). Returns the number of files
/// that were merged.
pub(super) async fn receive_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<u64, String> {
    let files = unmerged_files(application, provider, library_id).await?;
    let merged = merge_files(application, provider, library_id, &files).await?;
    fetch_awaited(application, provider, library_id).await?;
    Ok(merged)
}

/// What looking for the originals of one waiting item gave.
enum Fetched {
    /// The paths of the originals in the library, in the order of the
    /// record's files.
    Here(Vec<String>),
    /// An original is not in the cloud folder, or not whole yet.
    Waiting,
    /// An original is listed and could not be read.
    NotDelivered,
}

/// Copies the originals of the items that wait for them from the cloud
/// folder into the library, and marks the jobs of the items whose originals
/// are all there as prepared. Returns the number of those items.
///
/// One call looks at the items that have waited longest, at most
/// [`AWAITED_PER_ROUND`]. An item whose originals are not there is looked at
/// again after [`AWAITED_RETRY_SECONDS`]. An original that could not be read
/// ends the call, because each further read would wait as long.
pub(super) async fn fetch_awaited(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<u64, String> {
    let library = application.library();
    let now = chrono::Utc::now();
    let awaited = library
        .awaited_shared_imports(AWAITED_PER_ROUND, &now.to_rfc3339())
        .map_err(|error| error.to_string())?;
    let mut waiting = Vec::new();
    let mut fetched = 0;
    for item in &awaited {
        match fetch_item(application, provider, library_id, item).await {
            Ok(Fetched::Here(paths)) => {
                library
                    .prepare_shared_import(item, paths, &chrono::Utc::now().to_rfc3339())
                    .map_err(|error| error.to_string())?;
                fetched += 1;
            }
            Ok(Fetched::Waiting) => waiting.push(item.ingest_job_id),
            Ok(Fetched::NotDelivered) => {
                waiting.push(item.ingest_job_id);
                break;
            }
            Err(error) => {
                library
                    .fail_shared_import(item.ingest_job_id, &error, &now.to_rfc3339())
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    library
        .postpone_shared_imports(
            &waiting,
            &(now + chrono::Duration::seconds(AWAITED_RETRY_SECONDS)).to_rfc3339(),
        )
        .map_err(|error| error.to_string())?;
    Ok(fetched)
}

/// Makes sure that every original of the waiting item is in the library. An
/// original that the library already holds is not copied. The error is the
/// reason why the item cannot be imported at all.
async fn fetch_item(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
    item: &AwaitedImport,
) -> Result<Fetched, String> {
    let staging = application
        .root()
        .join("cloud")
        .join("staging")
        .join("blobs");
    let mut paths = Vec::with_capacity(item.import.record.files.len());
    for file in &item.import.record.files {
        let hash = file.facts.content_hash.as_str();
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("The record of the item names a file without a content hash".into());
        }
        let extension = crate::blob_store::mime_to_extension(&file.facts.mime);
        let stored = application
            .blobs()
            .original_path_with_ext(hash, Some(extension))
            .map_err(|error| format!("Failed to resolve local original: {error}"))?;
        if !stored.is_file() {
            // The original is under its content hash. Its extension is the
            // one that the device which wrote it stored it with.
            let folder = format!(
                "picto/{library_id}/blobs/f/{}/{}",
                &hash[..2],
                &hash[2..4]
            );
            let prefix = format!("{folder}/{hash}.");
            let Some(remote) = provider
                .list(&folder)
                .await?
                .into_iter()
                .find(|object| object.path.starts_with(&prefix))
            else {
                return Ok(Fetched::Waiting);
            };
            let download = staging.join(format!("{hash}-{}.download", uuid::Uuid::new_v4()));
            let fetched = provider
                .fetch_file(&remote.path, download.clone(), file.facts.size_bytes, hash)
                .await;
            let written = match fetched {
                Ok(CloudRead::Read(())) => application
                    .blobs()
                    .write_original_from_path(hash, &download, Some(extension))
                    .map_err(|error| format!("Failed to store cloud original: {error}")),
                Ok(CloudRead::Missing) => {
                    return Ok(Fetched::Waiting);
                }
                Ok(CloudRead::NotDelivered(reason)) => {
                    let _ = std::fs::remove_file(&download);
                    tracing::debug!(path = %remote.path, %reason, "An original is not delivered yet");
                    // A file that has not its whole size is being written.
                    // Any other reason is a read that could not finish.
                    return Ok(if remote.size_bytes == file.facts.size_bytes {
                        Fetched::NotDelivered
                    } else {
                        Fetched::Waiting
                    });
                }
                Err(error) => Err(error),
            };
            let _ = std::fs::remove_file(&download);
            written?;
        }
        paths.push(stored.to_string_lossy().into_owned());
    }
    Ok(Fetched::Here(paths))
}

#[cfg(test)]
mod tests {
    use picto_library::{EntryKind, EntryState};

    use super::*;

    fn entry(key: &str) -> SharedEntry {
        SharedEntry {
            kind: EntryKind::Item,
            key: key.into(),
            state: EntryState::Delete,
            at_ms: 7,
            logical: 1,
            value: Some(serde_json::json!({ "file": "0f".repeat(32) })),
        }
    }

    #[test]
    fn a_file_is_read_back_as_it_was_written() {
        let entries = vec![
            entry("one"),
            SharedEntry {
                kind: EntryKind::Status,
                key: "two".into(),
                state: EntryState::Trash,
                at_ms: 9,
                logical: 0,
                value: None,
            },
        ];
        let (name, bytes) = encode("library", "device", "0a1", &entries).unwrap();
        assert!(is_file_name(&name), "{name}");
        assert!(name.starts_with("0a1."));
        assert_eq!(shard_of_name(&name), "0a1");
        assert_eq!(decode(&bytes, "library", "device", &name).unwrap(), entries);
        // The same entries give the same name, and other entries another.
        assert_eq!(encode("library", "device", "0a1", &entries).unwrap().0, name);
        assert_ne!(encode("library", "device", "0a1", &entries[..1]).unwrap().0, name);
        // An entry is always in the file of its own three digits.
        let shard = shard_of(&entries[0]);
        assert_eq!(shard.len(), 3);
        assert_eq!(shard, shard_of(&entry("one")));
    }

    #[test]
    fn a_file_that_is_not_whole_or_not_this_one_is_refused() {
        let (name, bytes) = encode("library", "device", "0a1", &[entry("one")]).unwrap();
        // Cut short, as a file that was written only in part.
        assert!(decode(&bytes[..bytes.len() - 4], "library", "device", &name).is_err());
        assert!(decode(b"", "library", "device", &name).is_err());
        assert!(decode(b"not an entry file", "library", "device", &name).is_err());
        // The file of another library or device, or under another name.
        assert!(decode(&bytes, "other", "device", &name).is_err());
        assert!(decode(&bytes, "library", "other", &name).is_err());
        assert!(decode(&bytes, "library", "device", "0a1.0000000000000000.jsonl.zst").is_err());

        // A changed entry no longer matches the checksum of the header.
        let text = zstd::stream::decode_all(bytes.as_slice()).unwrap();
        let changed = String::from_utf8(text).unwrap().replace("\"one\"", "\"two\"");
        let bytes = zstd::stream::encode_all(changed.as_bytes(), 3).unwrap();
        assert!(decode(&bytes, "library", "device", &name)
            .unwrap_err()
            .contains("checksum"));

        // An entry of a kind that this version does not know, and a file of
        // another format.
        let line = "{\"kind\":\"later_kind\",\"key\":\"one\",\"state\":\"delete\",\"at_ms\":1,\"logical\":0}\n";
        let sha256 = hex::encode(Sha256::digest(line.as_bytes()));
        let name = format!("0a1.{}.jsonl.zst", &sha256[..16]);
        for (format, reason) in [(FORMAT, "cannot read"), (FORMAT + 1, "version of Picto")] {
            let header = serde_json::json!({
                "format": format, "library_id": "library", "device_id": "device",
                "shard": "0a1", "entries": 1, "sha256": sha256,
            });
            let bytes =
                zstd::stream::encode_all(format!("{header}\n{line}").as_bytes(), 3).unwrap();
            assert!(decode(&bytes, "library", "device", &name)
                .unwrap_err()
                .contains(reason));
        }
    }

    #[test]
    fn only_the_names_of_entry_files_are_taken_for_them() {
        assert!(is_file_name("0a1.0123456789abcdef.jsonl.zst"));
        for name in [
            "0a1.0123456789abcdef.jsonl",
            "0a1.0123456789abcde.jsonl.zst",
            "0a10123456789abcdef.jsonl.zst",
            "0a.0123456789abcdef.jsonl.zst",
            "0g1.0123456789abcdef.jsonl.zst",
            "世.0123456789abcdef.jsonl.zst",
            "0a1.01234567世abcde.jsonl.zst",
            "readme.md",
        ] {
            assert!(!is_file_name(name), "{name}");
        }
    }
}
