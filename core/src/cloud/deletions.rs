//! The deletion files in the cloud folder. They carry what the user deleted
//! on one device to the other devices that sync the library.
//!
//! Each device writes its own files, numbered from 1 without a gap:
//! `picto/<library id>/deletions/<device id>/<number>.jsonl.zst`. A file is
//! written once and never changed. Its content is compressed with zstd. The
//! first line is a JSON header that names the library, the device and the
//! number, and holds the SHA-256 of the rest. Every further line is one
//! deletion ([`picto_library::SyncedDeletion`]).
//!
//! A device reads the files of every other device in the order of their
//! numbers and remembers the last number it has applied
//! (`cloud_device_frontier`). It applies a file only when the file with the
//! number before it was applied, so a file that the provider's desktop app
//! has not brought yet holds back the files after it. A file that cannot be
//! read as a whole deletes nothing: every waiting file is checked before
//! the first one is applied.
//!
//! The library decides what a deletion changes
//! (`picto-library/src/mutation/synced.rs`). This file reads and writes the
//! files.

use chrono::Utc;
use picto_library::{OutgoingCursor, ReceivedDeletions, SyncedDeletion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::provider::CloudProvider;
use crate::library_application::LibraryApplication;

/// The number of the file format that this version reads and writes.
const FORMAT: u32 = 1;
/// The largest number of deletions in one file. More deletions are written
/// as several files.
const FILE_LIMIT: usize = 20_000;
/// The largest number of files that one round applies. The next round
/// applies the files after them.
const ROUND_LIMIT: usize = 64;

/// The first line of a deletion file.
#[derive(Debug, Serialize, Deserialize)]
struct Header {
    format: u32,
    library_id: String,
    device_id: String,
    sequence: u32,
    created_at_ms: i64,
    /// The number of deletions in the file.
    deletions: usize,
    /// The SHA-256 of everything after the first line, in hexadecimal.
    sha256: String,
}

/// One file of another device that waits to be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Waiting {
    device_id: String,
    sequence: u32,
    path: String,
}

/// What one round of reading the other devices' files did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Received {
    /// The number of files that were applied.
    pub files: u64,
    /// Whether a file is expected and cannot be read yet: the file with the
    /// next number is missing while a later one is there, or the file is
    /// there and the provider's desktop app has not downloaded it.
    pub incomplete: bool,
}

fn directory(library_id: &str) -> String {
    format!("picto/{library_id}/deletions")
}

fn file_path(library_id: &str, device_id: &str, sequence: u32) -> String {
    format!(
        "{}/{device_id}/{sequence:010}.jsonl.zst",
        directory(library_id)
    )
}

/// Returns the bytes of the deletion file `sequence` of `device_id`.
fn encode(
    library_id: &str,
    device_id: &str,
    sequence: u32,
    created_at_ms: i64,
    deletions: &[SyncedDeletion],
) -> Result<Vec<u8>, String> {
    let mut lines = Vec::new();
    for deletion in deletions {
        serde_json::to_writer(&mut lines, deletion).map_err(|error| error.to_string())?;
        lines.push(b'\n');
    }
    let header = Header {
        format: FORMAT,
        library_id: library_id.to_owned(),
        device_id: device_id.to_owned(),
        sequence,
        created_at_ms,
        deletions: deletions.len(),
        sha256: hex::encode(Sha256::digest(&lines)),
    };
    let mut text = serde_json::to_vec(&header).map_err(|error| error.to_string())?;
    text.push(b'\n');
    text.extend(lines);
    zstd::stream::encode_all(text.as_slice(), 9)
        .map_err(|error| format!("Failed to compress the deletion file: {error}"))
}

/// Returns the time at which the file was written and its deletions. The
/// error says, as the end of a sentence, why the bytes are not the whole
/// file `sequence` of `device_id` in this library.
fn decode(
    bytes: &[u8],
    library_id: &str,
    device_id: &str,
    sequence: u32,
) -> Result<(i64, Vec<SyncedDeletion>), String> {
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
    if header.library_id != library_id
        || header.device_id != device_id
        || header.sequence != sequence
    {
        return Err("belongs to another library, device or position".into());
    }
    let lines = &text[end + 1..];
    if hex::encode(Sha256::digest(lines)) != header.sha256 {
        return Err("does not match its checksum".into());
    }
    let deletions = lines
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice)
        .collect::<Result<Vec<SyncedDeletion>, _>>()
        .map_err(|_| "holds a deletion that this version of Picto cannot read".to_string())?;
    if deletions.len() != header.deletions {
        return Err("does not hold the number of deletions that its header states".into());
    }
    Ok((header.created_at_ms, deletions))
}

/// Returns this device's id and, for each device, the number of its last
/// deletion file that this library holds.
fn positions(
    application: &LibraryApplication,
) -> Result<(String, std::collections::HashMap<String, u32>), String> {
    application
        .library()
        .auxiliary_read(picto_library::database::WorkPriority::Cloud, |connection| {
            let device_id = connection.query_row(
                "SELECT device_id FROM cloud_state WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )?;
            let mut statement =
                connection.prepare("SELECT device_id, hlc_logical FROM cloud_device_frontier")?;
            let positions = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u32))
                })?
                .collect::<rusqlite::Result<_>>()?;
            Ok((device_id, positions))
        })
        .map_err(|error| error.to_string())
}

/// Writes the deletions that this device made up to `through_revision` into
/// the cloud folder, as one file for every `FILE_LIMIT` deletions.
///
/// A number is used for one file only. When a file with the next number is
/// already there, an earlier run wrote it and stopped before the library
/// recorded it, and the next free number is taken. The deletions of that
/// file are then written a second time, which changes nothing on a device
/// that applies both files.
pub(super) async fn write_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
    through_revision: u64,
) -> Result<(), String> {
    let (device_id, positions) = positions(application)?;
    let mut sequence = positions.get(&device_id).copied().unwrap_or(0);
    let mut cursor = Some(OutgoingCursor::default());
    while let Some(after) = cursor {
        let page = application
            .library()
            .outgoing_deletions(through_revision, after, FILE_LIMIT)
            .map_err(|error| error.to_string())?;
        cursor = page.next;
        if page.deletions.is_empty() {
            continue;
        }
        sequence += 1;
        while provider
            .exists(&file_path(library_id, &device_id, sequence))
            .await?
        {
            sequence += 1;
        }
        let created_at_ms = Utc::now().timestamp_millis();
        let bytes = encode(
            library_id,
            &device_id,
            sequence,
            created_at_ms,
            &page.deletions,
        )?;
        let checksum = hex::encode(Sha256::digest(&bytes));
        provider
            .upload(
                &file_path(library_id, &device_id, sequence),
                bytes,
                &checksum,
            )
            .await?;
        application
            .library()
            .record_written_deletions(sequence, created_at_ms, &page.deletions)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Returns the files of the other devices that can be applied now, in the
/// order in which they must be applied, and whether a file is missing in
/// front of a file that is there.
pub(super) async fn waiting_files(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<(Vec<Waiting>, bool), String> {
    let (own_device, positions) = positions(application)?;
    let mut devices = provider.children(&directory(library_id)).await?;
    devices.sort();
    let mut waiting = Vec::new();
    let mut gap = false;
    for device_id in devices {
        // A device's folder has the device's id as its name. Any other
        // folder was not written by the sync.
        if device_id == own_device || uuid::Uuid::parse_str(&device_id).is_err() {
            continue;
        }
        let mut sequence = positions.get(&device_id).copied().unwrap_or(0) + 1;
        while waiting.len() < ROUND_LIMIT
            && provider
                .exists(&file_path(library_id, &device_id, sequence))
                .await?
        {
            waiting.push(Waiting {
                path: file_path(library_id, &device_id, sequence),
                device_id: device_id.clone(),
                sequence,
            });
            sequence += 1;
        }
        if waiting.len() < ROUND_LIMIT
            && provider
                .exists(&file_path(library_id, &device_id, sequence + 1))
                .await?
        {
            gap = true;
        }
    }
    Ok((waiting, gap))
}

/// Reads one waiting file. Returns `None` when the file cannot be read from
/// the cloud folder now, which is the case for a file that the provider's
/// desktop app lists and has not downloaded. Returns an error when the file
/// was read and is not a whole deletion file.
async fn read(
    provider: &dyn CloudProvider,
    library_id: &str,
    file: &Waiting,
) -> Result<Option<(i64, Vec<SyncedDeletion>)>, String> {
    let Ok(bytes) = provider.download(&file.path).await else {
        return Ok(None);
    };
    decode(&bytes, library_id, &file.device_id, file.sequence)
        .map(Some)
        .map_err(|reason| {
            format!(
                "The deletion file {} in the cloud folder {reason}. \
                 Nothing was deleted on this device.",
                file.path
            )
        })
}

/// Applies the deletion files of the other devices that wait in the cloud
/// folder ([`waiting_files`], [`apply_files`]).
pub(super) async fn receive_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<Received, String> {
    let (waiting, gap) = waiting_files(application, provider, library_id).await?;
    apply_files(application, provider, library_id, waiting, gap).await
}

/// Applies the files `waiting`, which [`waiting_files`] returned together
/// with `gap`. Every file is read and checked first. If one of them is
/// damaged, none is applied and the error names it.
pub(super) async fn apply_files(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
    mut waiting: Vec<Waiting>,
    gap: bool,
) -> Result<Received, String> {
    let mut received = Received {
        files: 0,
        incomplete: gap,
    };
    // A file that cannot be read yet holds back the later files of its
    // device. The files of the other devices are still applied.
    let mut held_back = std::collections::HashSet::new();
    for file in &waiting {
        if held_back.contains(&file.device_id) {
            continue;
        }
        if read(provider, library_id, file).await?.is_none() {
            held_back.insert(file.device_id.clone());
        }
    }
    received.incomplete |= !held_back.is_empty();
    waiting.retain(|file| !held_back.contains(&file.device_id));
    for file in &waiting {
        let Some((created_at_ms, deletions)) = read(provider, library_id, file).await? else {
            // The file could be read a moment ago and cannot be read now.
            // The round ends here, and the next round goes on from this
            // file.
            received.incomplete = true;
            break;
        };
        application
            .library()
            .apply_synced_deletions(&ReceivedDeletions {
                device_id: &file.device_id,
                sequence: file.sequence,
                created_at_ms,
                deletions: &deletions,
            })
            .map_err(|error| error.to_string())?;
        received.files += 1;
    }
    Ok(received)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trash(key: &str) -> SyncedDeletion {
        SyncedDeletion::ItemTrash {
            key: key.into(),
            files: vec!["hash".into()],
            at_ms: 7,
        }
    }

    #[test]
    fn a_file_is_read_back_as_it_was_written() {
        let deletions = vec![
            trash("one"),
            SyncedDeletion::TagDelete {
                key: "tag".into(),
                at_ms: 9,
            },
        ];
        let bytes = encode("library", "device", 3, 42, &deletions).unwrap();
        assert_eq!(
            decode(&bytes, "library", "device", 3).unwrap(),
            (42, deletions)
        );
    }

    #[test]
    fn a_file_that_is_not_whole_or_not_this_one_is_refused() {
        let bytes = encode("library", "device", 3, 42, &[trash("one")]).unwrap();
        // Cut short, as a file that was written only in part.
        assert!(decode(&bytes[..bytes.len() - 4], "library", "device", 3).is_err());
        assert!(decode(b"", "library", "device", 3).is_err());
        assert!(decode(b"not a deletion file", "library", "device", 3).is_err());
        // The file of another library, device or position.
        assert!(decode(&bytes, "other", "device", 3).is_err());
        assert!(decode(&bytes, "library", "other", 3).is_err());
        assert!(decode(&bytes, "library", "device", 4).is_err());

        // A changed deletion no longer matches the checksum of the header.
        let text = zstd::stream::decode_all(bytes.as_slice()).unwrap();
        let changed = String::from_utf8(text).unwrap().replace("\"one\"", "\"two\"");
        let bytes = zstd::stream::encode_all(changed.as_bytes(), 3).unwrap();
        assert!(decode(&bytes, "library", "device", 3)
            .unwrap_err()
            .contains("checksum"));

        // A deletion of a kind that this version does not know.
        let line = "{\"kind\":\"later_kind\",\"key\":\"one\",\"at_ms\":1}\n";
        let header = serde_json::json!({
            "format": FORMAT, "library_id": "library", "device_id": "device",
            "sequence": 3, "created_at_ms": 1, "deletions": 1,
            "sha256": hex::encode(Sha256::digest(line.as_bytes())),
        });
        let bytes =
            zstd::stream::encode_all(format!("{header}\n{line}").as_bytes(), 3).unwrap();
        assert!(decode(&bytes, "library", "device", 3).is_err());
    }
}
