//! The files in the cloud folder that hold the entries the computers of a
//! library send each other.
//!
//! The library keeps one entry per key: what one computer did to a picture,
//! a tag or a folder, and when (`picto-library/src/mutation/shared.rs`).
//! Each computer writes the entries it made into its own folder,
//! `picto/<library id>/entries/<device id>/`, and merges the entries in the
//! folders of all computers. The library decides which entry wins; this file
//! only reads and writes the files.
//!
//! A computer merges its own folder too. Usually that changes nothing. After
//! a snapshot was restored, the library holds older entries than the computer
//! wrote, and the merge brings them back.
//!
//! A computer's entries are spread over 256 files by the first byte of the
//! SHA-256 of an entry's key, so a change rewrites only the files of the keys
//! that changed. A file is named after those two hexadecimal digits and the
//! first 16 hexadecimal digits of the SHA-256 of its lines, with the ending
//! `.jsonl.zst`. It's compressed with zstd and holds a JSON header (library,
//! computer, the two digits and the whole SHA-256 of the lines) and then one
//! entry per line. A file is written whole beside its place and renamed into
//! it, and never changed afterwards: a new state of the same keys is a new
//! file, and the one it replaces is removed.
//!
//! Files don't depend on each other, and merging entries twice or in another
//! order gives the same library. So a computer merges whatever files are
//! there and readable, and a file the provider's desktop app hasn't brought
//! yet is merged when it arrives. A file that doesn't match its checksum
//! isn't merged, and no file is merged in the round that finds it.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{LazyLock, Mutex};

use picto_library::SharedEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::provider::CloudProvider;
use crate::library_application::LibraryApplication;

/// The number of the file format that this version reads and writes.
const FORMAT: u32 = 1;
/// The number of roots whose changes are turned into entries in one write
/// transaction. The library stays usable between the transactions.
const SETTLE_LIMIT: usize = 20_000;

/// The first line of an entry file.
#[derive(Debug, Serialize, Deserialize)]
struct Header {
    format: u32,
    library_id: String,
    device_id: String,
    /// The two hexadecimal digits that every entry of the file has.
    shard: String,
    /// The number of entries in the file.
    entries: usize,
    /// The SHA-256 of everything after the first line, in hexadecimal.
    sha256: String,
}

/// One file of a device that this process has not merged yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Unmerged {
    device_id: String,
    name: String,
    path: String,
}

/// The files that this process has merged since the library was opened,
/// each as the id of the device that merged it and the file's path. A file
/// is never changed, so a file that was merged does not have to be read
/// again. The set only saves work: merging a file a second time changes
/// nothing.
static MERGED: LazyLock<Mutex<HashSet<(String, String)>>> = LazyLock::new(Default::default);

/// Returns whether the device `device_id` has merged the file at `path`.
fn is_merged(device_id: &str, path: &str) -> bool {
    MERGED
        .lock()
        .unwrap_or_else(|held| held.into_inner())
        .contains(&(device_id.to_owned(), path.to_owned()))
}

/// Records that the device `device_id` has merged the file at `path`.
fn set_merged(device_id: &str, path: &str) {
    MERGED
        .lock()
        .unwrap_or_else(|held| held.into_inner())
        .insert((device_id.to_owned(), path.to_owned()));
}

/// Forgets which files were merged. It is called when a library is opened,
/// because the library can be another one, or one whose database was
/// replaced by a snapshot.
pub(super) fn forget_merged() {
    MERGED.lock().unwrap_or_else(|held| held.into_inner()).clear();
}

fn directory(library_id: &str) -> String {
    format!("picto/{library_id}/entries")
}

/// Returns the two hexadecimal digits of the file that holds `entry`.
fn shard_of(entry: &SharedEntry) -> String {
    let mut hasher = Sha256::new();
    hasher.update(entry.kind.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(entry.subject_key.as_bytes());
    hasher.update(b"\n");
    hasher.update(entry.second_key.as_bytes());
    hex::encode(&hasher.finalize()[..1])
}

/// Returns whether `name` is the name of an entry file: two hexadecimal
/// digits, a dot, 16 hexadecimal digits and `.jsonl.zst`.
pub(super) fn is_file_name(name: &str) -> bool {
    let hexadecimal = |text: &str| text.bytes().all(|byte| byte.is_ascii_hexdigit());
    name.strip_suffix(".jsonl.zst")
        .and_then(|stem| stem.split_once('.'))
        .is_some_and(|(shard, digest)| {
            shard.len() == 2 && hexadecimal(shard) && digest.len() == 16 && hexadecimal(digest)
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
    if entries.iter().any(|entry| entry.device_id != device_id) {
        return Err("holds entries of another device".into());
    }
    Ok(entries)
}

/// Writes the entries that this computer made into the cloud folder.
///
/// The first sync starts recording, which gives every picture the library
/// already holds an `add` entry. Then the computer's pending changes are
/// turned into entries. Then it rewrites the files of every key whose entry is later
/// than the clock time up to which the device's entries were written before,
/// removes the files that the new ones replace, and records the new clock
/// time. A run that stops in the middle is repeated by the next one and
/// writes the same files.
pub(super) async fn publish_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<(), String> {
    let library = application.library();
    library.start_recording().map_err(|error| error.to_string())?;
    while library
        .settle_entries(SETTLE_LIMIT)
        .map_err(|error| error.to_string())?
    {}
    let clock = library.entry_clock().map_err(|error| error.to_string())?;
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
    }
    // The files that were just written hold what the library holds, so
    // they do not have to be merged.
    for object in provider.list(&folder).await? {
        set_merged(&clock.device_id, &object.path);
    }
    library
        .set_entries_published(clock.now)
        .map_err(|error| error.to_string())
}

/// Returns the entry files of all computers that this process has not
/// merged yet.
pub(super) async fn unmerged_files(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<Vec<Unmerged>, String> {
    let own_device = application
        .library()
        .entry_clock()
        .map_err(|error| error.to_string())?
        .device_id;
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
            if is_file_name(name) && !is_merged(&own_device, &object.path) {
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

/// Reads one file. Returns `None` when the file cannot be read from the
/// cloud folder now, which is the case for a file that the provider's
/// desktop app lists and has not downloaded, and for a file that its device
/// has replaced meanwhile. Returns an error when the file was read and is
/// not a whole entry file.
async fn read(
    provider: &dyn CloudProvider,
    library_id: &str,
    file: &Unmerged,
) -> Result<Option<Vec<SharedEntry>>, String> {
    let Ok(bytes) = provider.download(&file.path).await else {
        return Ok(None);
    };
    decode(&bytes, library_id, &file.device_id, &file.name)
        .map(Some)
        .map_err(|reason| {
            format!(
                "The entry file {} in the cloud folder {reason}. \
                 Nothing from the cloud folder was merged on this computer.",
                file.path
            )
        })
}

/// Merges `files` into the library and returns the number of files that
/// were merged. Every file is read and checked first. If one of them is
/// damaged, none is merged and the error names it. A file that cannot be
/// read now is left for a later round.
pub(super) async fn merge_files(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
    files: &[Unmerged],
) -> Result<u64, String> {
    for file in files {
        read(provider, library_id, file).await?;
    }
    let own_device = application
        .library()
        .entry_clock()
        .map_err(|error| error.to_string())?
        .device_id;
    let mut merged = 0;
    for file in files {
        let Some(entries) = read(provider, library_id, file).await? else {
            continue;
        };
        application
            .library()
            .merge_entries(&entries)
            .map_err(|error| error.to_string())?;
        set_merged(&own_device, &file.path);
        merged += 1;
    }
    Ok(merged)
}

/// Merges the entry files that this process has not merged yet
/// ([`unmerged_files`], [`merge_files`]).
pub(super) async fn receive_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<u64, String> {
    let files = unmerged_files(application, provider, library_id).await?;
    merge_files(application, provider, library_id, &files).await
}

#[cfg(test)]
mod tests {
    use picto_library::EntryKind;

    use super::*;

    fn entry(subject_key: &str) -> SharedEntry {
        SharedEntry {
            kind: EntryKind::Delete,
            subject_key: subject_key.into(),
            second_key: String::new(),
            value: None,
            clock_ms: 7,
            clock_counter: 1,
            device_id: "device".into(),
        }
    }

    #[test]
    fn a_file_is_read_back_as_it_was_written() {
        let entries = vec![
            entry("one"),
            SharedEntry {
                kind: EntryKind::Status,
                subject_key: "two".into(),
                second_key: String::new(),
                value: Some("trash".into()),
                clock_ms: 9,
                clock_counter: 0,
                device_id: "device".into(),
            },
        ];
        let (name, bytes) = encode("library", "device", "0a", &entries).unwrap();
        assert!(is_file_name(&name), "{name}");
        assert!(name.starts_with("0a."));
        assert_eq!(decode(&bytes, "library", "device", &name).unwrap(), entries);
        // The same entries give the same name, and other entries another.
        assert_eq!(encode("library", "device", "0a", &entries).unwrap().0, name);
        assert_ne!(encode("library", "device", "0a", &entries[..1]).unwrap().0, name);
    }

    #[test]
    fn a_file_that_is_not_whole_or_not_this_one_is_refused() {
        let (name, bytes) = encode("library", "device", "0a", &[entry("one")]).unwrap();
        // Cut short, as a file that was written only in part.
        assert!(decode(&bytes[..bytes.len() - 4], "library", "device", &name).is_err());
        assert!(decode(b"", "library", "device", &name).is_err());
        assert!(decode(b"not an entry file", "library", "device", &name).is_err());
        // The file of another library or device, or under another name.
        assert!(decode(&bytes, "other", "device", &name).is_err());
        assert!(decode(&bytes, "library", "other", &name).is_err());
        assert!(decode(&bytes, "library", "device", "0a.0000000000000000.jsonl.zst").is_err());

        // A changed entry no longer matches the checksum of the header.
        let text = zstd::stream::decode_all(bytes.as_slice()).unwrap();
        let changed = String::from_utf8(text).unwrap().replace("\"one\"", "\"two\"");
        let bytes = zstd::stream::encode_all(changed.as_bytes(), 3).unwrap();
        assert!(decode(&bytes, "library", "device", &name)
            .unwrap_err()
            .contains("checksum"));

        // An entry of a kind that this version does not know.
        let line = "{\"kind\":\"later_kind\",\"subject_key\":\"one\",\"clock_ms\":1,\"clock_counter\":0,\"device_id\":\"device\"}\n";
        let sha256 = hex::encode(Sha256::digest(line.as_bytes()));
        let name = format!("0a.{}.jsonl.zst", &sha256[..16]);
        let header = serde_json::json!({
            "format": FORMAT, "library_id": "library", "device_id": "device",
            "shard": "0a", "entries": 1, "sha256": sha256,
        });
        let bytes =
            zstd::stream::encode_all(format!("{header}\n{line}").as_bytes(), 3).unwrap();
        assert!(decode(&bytes, "library", "device", &name)
            .unwrap_err()
            .contains("cannot read"));
    }

    #[test]
    fn only_the_names_of_entry_files_are_taken_for_them() {
        assert!(is_file_name("0a.0123456789abcdef.jsonl.zst"));
        for name in [
            "0a.0123456789abcdef.jsonl",
            "0a.0123456789abcde.jsonl.zst",
            "0a0123456789abcdef.jsonl.zst",
            "0g.0123456789abcdef.jsonl.zst",
            "世.0123456789abcdef.jsonl.zst",
            "0a.01234567世abcde.jsonl.zst",
            "readme.md",
        ] {
            assert!(!is_file_name(name), "{name}");
        }
    }
}
