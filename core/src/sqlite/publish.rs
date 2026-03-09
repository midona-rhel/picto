//! Derived artifact publication boundary.
//!
//! Compilers rebuild in-memory/read-model artifacts. Publication is handled here:
//! artifact versions are bumped, bitmap payload state is published, and the
//! manifest snapshot is flushed atomically to SQLite.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use rusqlite::Connection;
use serde_json::json;

use super::read_model::{DerivedArtifact, PublishedArtifacts};
use super::SqliteDatabase;

fn parse_active_bitmap_file(payload_json: Option<&str>) -> Option<String> {
    payload_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|v| {
            v.get("active_file")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
}

struct ManifestState {
    published_epoch: u64,
    published_artifact_versions: HashMap<String, u64>,
    published_artifact_payloads: HashMap<String, String>,
    working_artifact_versions: HashMap<String, u64>,
    working_artifact_payloads: HashMap<String, String>,
    dirty: bool,
}

/// Global manifest snapshot tracker for derived artifact publication.
pub struct Manifest {
    state: RwLock<ManifestState>,
}

impl Manifest {
    pub fn new() -> Self {
        let mut artifact_versions = HashMap::new();
        let mut artifact_payloads = HashMap::new();
        for key in [
            "global",
            "files",
            "tags",
            "tag_graph",
            "effective_tags",
            "metadata_projection",
            "sidebar",
            "smart_folders",
            "bitmaps",
        ] {
            artifact_versions.insert(key.to_string(), 0);
        }
        artifact_payloads.insert(
            "bitmaps".to_string(),
            json!({"active_file":"bitmaps.bin"}).to_string(),
        );
        Self {
            state: RwLock::new(ManifestState {
                published_epoch: 0,
                published_artifact_versions: artifact_versions.clone(),
                published_artifact_payloads: artifact_payloads.clone(),
                working_artifact_versions: artifact_versions,
                working_artifact_payloads: artifact_payloads,
                dirty: false,
            }),
        }
    }

    pub fn load_from_db(conn: &Connection) -> rusqlite::Result<Self> {
        let m = Self::new();

        let has_new_manifest_tables: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master
             WHERE type='table' AND name='artifact_manifest_meta'",
            [],
            |row| row.get(0),
        )?;

        if has_new_manifest_tables {
            let published_epoch: u64 = conn
                .query_row(
                    "SELECT manifest_epoch FROM artifact_manifest_meta WHERE id = 1",
                    [],
                    |row| row.get::<_, u64>(0),
                )
                .unwrap_or(0);

            let mut stmt = conn.prepare_cached(
                "SELECT artifact_name, artifact_version, payload_json
                 FROM artifact_manifest_entry
                 WHERE manifest_epoch = ?1",
            )?;
            let rows = stmt.query_map([published_epoch], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;

            let mut loaded_any = false;
            {
                let mut state = crate::poison::write_or_recover(&m.state, "manifest::init");
                state.published_epoch = published_epoch;
                for row in rows {
                    let (name, version, payload_json) = row?;
                    state
                        .published_artifact_versions
                        .insert(name.clone(), version);
                    state
                        .published_artifact_payloads
                        .insert(name.clone(), payload_json.clone());
                    state
                        .working_artifact_versions
                        .insert(name.clone(), version);
                    state.working_artifact_payloads.insert(name, payload_json);
                    loaded_any = true;
                }
                state.dirty = false;
            }

            if loaded_any {
                return Ok(m);
            }
        }

        Ok(m)
    }

    pub fn published_epoch(&self) -> u64 {
        crate::poison::read_or_recover(&self.state, "manifest::published_epoch").published_epoch
    }

    pub fn published_artifact_version(&self, key: &str) -> u64 {
        crate::poison::read_or_recover(&self.state, "manifest::published_artifact_version")
            .published_artifact_versions
            .get(key)
            .copied()
            .unwrap_or(0)
    }

    pub fn published_artifact_payload_json(&self, key: &str) -> Option<String> {
        crate::poison::read_or_recover(&self.state, "manifest::published_payload")
            .published_artifact_payloads
            .get(key)
            .cloned()
    }

    pub fn mark_artifact_dirty(&self, artifact: DerivedArtifact) -> u64 {
        let key = artifact.as_key();
        let mut state = crate::poison::write_or_recover(&self.state, "manifest::bump_version");
        let new_version = {
            let version = state
                .working_artifact_versions
                .entry(key.to_string())
                .or_insert(0);
            *version += 1;
            *version
        };
        state.dirty = true;
        new_version
    }

    pub fn set_working_artifact_payload_json(&self, key: &str, payload_json: String) {
        let mut state = crate::poison::write_or_recover(&self.state, "manifest::set_payload");
        let changed = state
            .working_artifact_payloads
            .get(key)
            .map(|existing| existing != &payload_json)
            .unwrap_or(true);
        if changed {
            state
                .working_artifact_payloads
                .insert(key.to_string(), payload_json);
            state.dirty = true;
        }
    }

    pub fn flush_to_db(&self, conn: &mut Connection) -> rusqlite::Result<PublishedArtifacts> {
        let mut state = crate::poison::write_or_recover(&self.state, "manifest::flush");
        if !state.dirty {
            let artifact_versions = state
                .published_artifact_versions
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            return Ok(PublishedArtifacts {
                manifest_epoch: state.published_epoch,
                artifact_versions,
            });
        }

        let next_manifest_epoch = state.published_epoch + 1;
        let artifact_versions = state.working_artifact_versions.clone();
        let artifact_payloads = state.working_artifact_payloads.clone();

        let tx = conn.transaction()?;

        tx.execute(
            "INSERT OR IGNORE INTO artifact_manifest_meta (id, manifest_epoch, updated_at)
             VALUES (1, 0, CURRENT_TIMESTAMP)",
            [],
        )?;

        {
            let mut entry_stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO artifact_manifest_entry
                    (manifest_epoch, artifact_name, artifact_version, built_from_truth_seq, payload_json)
                 VALUES (?1, ?2, ?3, 0, ?4)",
            )?;
            for (artifact_name, artifact_version) in artifact_versions.iter() {
                let payload_json = artifact_payloads
                    .get(artifact_name)
                    .cloned()
                    .unwrap_or_else(|| "{}".to_string());
                entry_stmt.execute(rusqlite::params![
                    next_manifest_epoch,
                    artifact_name,
                    artifact_version,
                    payload_json
                ])?;
            }
        }

        tx.execute(
            "UPDATE artifact_manifest_meta
             SET manifest_epoch = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = 1",
            [next_manifest_epoch],
        )?;

        tx.commit()?;

        if next_manifest_epoch > 2 {
            conn.execute(
                "DELETE FROM artifact_manifest_entry WHERE manifest_epoch < ?1",
                [next_manifest_epoch - 1],
            )?;
        }

        state.published_epoch = next_manifest_epoch;
        state.published_artifact_versions = artifact_versions.clone();
        state.published_artifact_payloads = artifact_payloads;
        state.working_artifact_versions = artifact_versions.clone();
        state.dirty = false;
        Ok(PublishedArtifacts {
            manifest_epoch: next_manifest_epoch,
            artifact_versions: artifact_versions.into_iter().collect::<BTreeMap<_, _>>(),
        })
    }
}

pub fn active_bitmap_file_from_manifest(manifest: &Manifest) -> Option<String> {
    parse_active_bitmap_file(manifest.published_artifact_payload_json("bitmaps").as_deref())
}

pub async fn publish_pending(
    db: &Arc<SqliteDatabase>,
    dirty_artifacts: &[DerivedArtifact],
) -> Result<PublishedArtifacts, String> {
    let previous_active = active_bitmap_file_from_manifest(&db.manifest);
    let mut new_active_for_cleanup: Option<String> = None;

    for artifact in dirty_artifacts {
        db.manifest.mark_artifact_dirty(*artifact);
    }

    if db.bitmaps.is_dirty() {
        let bitmap_version = db.manifest.mark_artifact_dirty(DerivedArtifact::Bitmaps);
        let active_file = db
            .bitmaps
            .flush_versioned(bitmap_version)
            .map_err(|e| format!("Bitmap flush error: {e}"))?;
        new_active_for_cleanup = Some(active_file.clone());
        db.manifest.set_working_artifact_payload_json(
            "bitmaps",
            json!({ "active_file": active_file }).to_string(),
        );
    }

    let manifest = db.manifest.clone();
    let published = db
        .with_conn_mut(move |conn| manifest.flush_to_db(conn))
        .await?;

    if let Some(active_file) = new_active_for_cleanup {
        let mut keep = vec![active_file.clone()];
        if let Some(prev) = previous_active.filter(|p| p != &active_file) {
            keep.push(prev);
        }
        match db.bitmaps.prune_artifacts(&keep) {
            Ok(deleted) => {
                if deleted > 0 {
                    tracing::info!(deleted, "Pruned stale bitmap artifact files");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Bitmap artifact cleanup (post-flush) failed");
            }
        }
    }

    Ok(published)
}
