//! Media protocol helpers — MIME resolution, hash parsing, range parsing.
//!
//! The actual protocol handler lives in the host runtime (Electron main.mjs).
//! This module provides shared utilities.

use crate::blob_store::{extension_to_mime, BlobStore};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Parse a strict segment `<hash>.<ext>`.
/// Returns (hash, extension).
pub fn parse_hash_segment(segment: &str) -> Option<(&str, &str)> {
    if segment.len() <= 65 {
        return None;
    }
    let hash_part = &segment[..64];
    if !hash_part.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if segment.as_bytes()[64] != b'.' {
        return None;
    }
    let ext = &segment[65..];
    if ext.is_empty() || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some((hash_part, ext))
}

/// Resolve a media path to a file on disk and its MIME type.
///
/// `route` is "thumb" or "file", `hash` is the 64-char hex hash,
/// `ext` is the file extension.
pub fn resolve_media_path(
    blob_store: &BlobStore,
    route: &str,
    hash: &str,
    ext: &str,
) -> Result<(PathBuf, String), String> {
    match route {
        "thumb" => {
            if ext.to_ascii_lowercase() != "jpg" {
                return Err("Thumbnail extension must be .jpg".to_string());
            }
            let p = blob_store
                .find_thumbnail_path(hash)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "Thumbnail not found".to_string())?;
            Ok((p, "image/jpeg".to_string()))
        }
        "file" => {
            let mime = extension_to_mime(ext).to_string();
            let found = blob_store
                .find_original(hash, Some(ext))
                .map_err(|e| e.to_string())?;
            match found {
                Some((p, _)) => Ok((p, mime)),
                None => Err("File not found".to_string()),
            }
        }
        _ => Err("Unknown route".to_string()),
    }
}

/// Read a byte range from a file. Returns (data, start, end, file_size).
pub fn read_range(
    file: &mut File,
    file_size: u64,
    range_str: &str,
) -> Result<(Vec<u8>, u64, u64, u64), String> {
    let (start, end) = parse_range(range_str, file_size).ok_or_else(|| {
        format!(
            "Range not satisfiable: {} (file size: {})",
            range_str, file_size
        )
    })?;

    let length = end - start + 1;
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; length as usize];
    file.read_exact(&mut buf).map_err(|e| e.to_string())?;

    Ok((buf, start, end, file_size))
}

/// Read the entire file into memory.
pub fn read_full(file: &mut File, file_size: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::with_capacity(file_size as usize);
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Parse an HTTP `Range` header value into (start, end) inclusive byte offsets.
pub fn parse_range(range_str: &str, file_size: u64) -> Option<(u64, u64)> {
    let spec = range_str.strip_prefix("bytes=")?;
    let spec = spec.split(',').next()?.trim();

    if let Some(suffix) = spec.strip_prefix('-') {
        let n: u64 = suffix.parse().ok()?;
        if n == 0 || n > file_size {
            return None;
        }
        Some((file_size - n, file_size - 1))
    } else if let Some(prefix) = spec.strip_suffix('-') {
        let start: u64 = prefix.parse().ok()?;
        if start >= file_size {
            return None;
        }
        Some((start, file_size - 1))
    } else {
        let mut parts = spec.splitn(2, '-');
        let start: u64 = parts.next()?.parse().ok()?;
        let end: u64 = parts.next()?.parse().ok()?;
        if start > end || start >= file_size {
            return None;
        }
        Some((start, end.min(file_size - 1)))
    }
}
