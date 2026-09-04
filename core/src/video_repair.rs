//! Explicit repair of full-range VP9 encodes, retaining original bytes for recovery.
use std::{path::Path, process::Output, time::Duration};

use picto_library::{database::WorkPriority, MediaId, RepairedMediaContent};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::{library_application::LibraryApplication, media_processing::ffmpeg_path};

#[derive(Debug, Serialize)]
pub struct VideoRepairResult {
    pub original_hash: String,
    pub repaired_hash: String,
    pub backup_path: String,
    pub receipt: picto_library::MutationReceipt,
}

async fn run(command: &mut Command, seconds: u64) -> Result<Output, String> {
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(seconds), command.output())
        .await
        .map_err(|_| "Video repair process timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Video repair failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output)
}

async fn probe(path: &Path) -> Result<Value, String> {
    let output = run(
        Command::new(ffmpeg_path::ffprobe_path()?)
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(path),
        30,
    )
    .await?;
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

fn video(probe: &Value) -> Result<&Value, String> {
    let streams = probe["streams"].as_array().ok_or("Missing media streams")?;
    if streams.len() != 1 || streams[0]["codec_name"] != "vp9" {
        return Err("Repair only supports single-stream VP9 animations".into());
    }
    Ok(&streams[0])
}

fn verify_repair(before: &Value, after: &Value) -> Result<(), String> {
    let original = video(before)?;
    let repaired = video(after)?;
    if repaired["color_range"] != "tv"
        || repaired["pix_fmt"] != "yuv420p"
        || original["width"] != repaired["width"]
        || original["height"] != repaired["height"]
        || original["avg_frame_rate"] != repaired["avg_frame_rate"]
    {
        return Err(
            "Repaired video changed geometry/timing or has an incompatible color range".into(),
        );
    }
    let duration = |value: &Value| {
        value["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
    };
    match (duration(before), duration(after)) {
        (Some(before), Some(after))
            if before.is_finite() && after.is_finite() && (before - after).abs() < 0.002 =>
        {
            Ok(())
        }
        _ => Err("Repaired video changed duration".into()),
    }
}

pub async fn repair(
    application: &LibraryApplication,
    hash: &str,
) -> Result<VideoRepairResult, String> {
    let (media_id, source, mime) = application.library().auxiliary_read(WorkPriority::VisibleRead, |connection| {
        connection.query_row(
            "SELECT media.media_id, file.file_path, file.mime FROM media_file file JOIN media_item media ON media.file_id = file.file_id WHERE file.content_hash = ?1 LIMIT 1",
            [hash], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).map_err(Into::into)
    }).map_err(|error| error.to_string())?;
    if mime != "video/webm" {
        return Err("Repair requires a WebM animation".into());
    }
    let source = Path::new(&source);
    let actual = hex::encode(
        crate::media_processing::get_hash_from_path(source).map_err(|e| e.to_string())?,
    );
    if actual != hash {
        return Err("Original file no longer matches its library hash".into());
    }
    let before = probe(source).await?;
    if video(&before)?["color_range"] != "pc" {
        return Err("Video does not need a full-range VP9 repair".into());
    }

    let work = tempfile::tempdir().map_err(|error| error.to_string())?;
    let converted = work.path().join("repaired.webm");
    run(
        Command::new(ffmpeg_path::ffmpeg_path()?)
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(source)
            .args([
                "-map",
                "0:v:0",
                "-vf",
                "scale=in_range=pc:out_range=tv",
                "-color_range",
                "tv",
                "-c:v",
                "libvpx-vp9",
                "-crf",
                "12",
                "-b:v",
                "0",
                "-pix_fmt",
                "yuv420p",
                "-threads",
                "2",
                "-an",
                "-n",
            ])
            .arg(&converted),
        600,
    )
    .await?;
    verify_repair(&before, &probe(&converted).await?)?;
    run(
        Command::new(ffmpeg_path::ffmpeg_path()?)
            .args(["-nostdin", "-v", "error", "-xerror", "-i"])
            .arg(&converted)
            .args(["-f", "null", "-"]),
        120,
    )
    .await?;
    let repaired_hash = hex::encode(
        crate::media_processing::get_hash_from_path(&converted).map_err(|e| e.to_string())?,
    );
    let backup_directory = application
        .root()
        .join("repairs")
        .join("vp9-range")
        .join(hash);
    std::fs::create_dir_all(&backup_directory).map_err(|e| e.to_string())?;
    let backup_path = backup_directory.join("original.webm");
    // create_new prevents retries from overwriting the recovery copy.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup_path)
    {
        Ok(mut output) => {
            std::io::copy(
                &mut std::fs::File::open(source).map_err(|e| e.to_string())?,
                &mut output,
            )
            .map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.to_string()),
    }
    if hex::encode(
        crate::media_processing::get_hash_from_path(&backup_path).map_err(|e| e.to_string())?,
    ) != hash
    {
        return Err("Recovery copy failed hash verification; original left unchanged".into());
    }
    application
        .blobs()
        .write_original_from_path(&repaired_hash, &converted, Some("webm"))
        .map_err(|e| e.to_string())?;
    let repaired_path = application
        .blobs()
        .original_path_with_ext(&repaired_hash, Some("webm"))
        .map_err(|e| e.to_string())?;
    let manifest = json!({ "media_id": media_id, "original_hash": hash, "original_path": source,
        "repaired_hash": repaired_hash, "repaired_path": repaired_path, "original_probe": before });
    std::fs::write(
        backup_directory.join("repair.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let receipt = application
        .library()
        .repair_media_content(
            MediaId(media_id),
            &RepairedMediaContent {
                expected_hash: hash.to_string(),
                content_hash: repaired_hash.clone(),
                file_path: repaired_path.to_string_lossy().into_owned(),
                size_bytes: std::fs::metadata(&converted)
                    .map_err(|e| e.to_string())?
                    .len(),
            },
            chrono::Utc::now().timestamp_millis(),
        )
        .map_err(|e| e.to_string())?;
    tracing::info!(original_hash = hash, %repaired_hash, "Repaired VP9 video; original bytes retained");
    Ok(VideoRepairResult {
        original_hash: hash.to_string(),
        repaired_hash,
        backup_path: backup_path.to_string_lossy().into_owned(),
        receipt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_rejects_timing_geometry_and_range_changes() {
        let before = json!({"streams":[{"codec_name":"vp9", "width":600,"height":600,"avg_frame_rate":"100/1","pix_fmt":"yuv420p","color_range":"pc"}],"format":{"duration":"8.070"}});
        let mut after = before.clone();
        assert!(verify_repair(&before, &after).is_err());
        after["streams"][0]["color_range"] = json!("tv");
        assert!(verify_repair(&before, &after).is_ok());
        after["format"]["duration"] = json!("8.170");
        assert!(verify_repair(&before, &after).is_err());
        after["format"]["duration"] = json!("8.070");
        after["streams"][0]["width"] = json!(599);
        assert!(verify_repair(&before, &after).is_err());
    }
}
