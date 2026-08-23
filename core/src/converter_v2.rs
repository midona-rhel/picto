//! One-shot conversion from the legacy schema-117 library to the replacement store.
//!
//! This module is intentionally not a migration system. It reads the legacy database
//! in read-only mode, writes a new library, and reports every legacy surface that has
//! no replacement owner. The source is never opened for writing and is never removed.

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::blob_store::mime_to_extension;
use crate::store::{schema as replacement_schema, Store};
use crate::subscriptions::gallery_dl_runner::site_by_id;

const LEGACY_SCHEMA_VERSION: i64 = 117;
const LEGACY_DATABASE_FILE: &str = "library.db";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionMode {
    DryRun,
    Execute,
}

#[derive(Debug, Clone)]
pub struct ConversionRequest {
    pub source_root: PathBuf,
    pub destination_root: PathBuf,
    pub mode: ConversionMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionCounts {
    pub media_files: i64,
    pub media_entities: i64,
    pub media_views: i64,
    pub tags: i64,
    pub entity_tags: i64,
    pub tag_aliases: i64,
    pub tag_implications: i64,
    pub folders: i64,
    pub folder_members: i64,
    pub smart_folders: i64,
    pub subscriptions: i64,
    pub subscription_queries: i64,
    pub subscription_runs: i64,
    pub subscription_query_runs: i64,
    pub subscription_issues: i64,
    pub source_posts: i64,
    pub source_items: i64,
    pub subscription_post_members: i64,
    pub ingest_items: i64,
    pub work_items: i64,
    pub credentials: i64,
    pub credential_health: i64,
    pub duplicates: i64,
    pub file_colors: i64,
    pub view_preferences: i64,
    pub settings: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountMismatch {
    pub name: String,
    pub source: i64,
    pub destination: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionReport {
    pub source_schema_version: i64,
    pub destination_schema_version: i64,
    pub dry_run: bool,
    pub source_counts: ConversionCounts,
    pub destination_counts: Option<ConversionCounts>,
    pub copied_blob_files: u64,
    pub copied_blob_bytes: u64,
    pub copied_gallery_dl_archive: bool,
    pub discarded_fields: Vec<String>,
    pub unmapped_fields: Vec<String>,
    pub mismatches: Vec<CountMismatch>,
}

impl ConversionReport {
    pub fn is_success(&self) -> bool {
        self.mismatches.is_empty() && self.unmapped_fields.is_empty()
    }
}

pub fn dry_run(
    source_root: impl AsRef<Path>,
    destination_root: impl AsRef<Path>,
) -> Result<ConversionReport, String> {
    convert(ConversionRequest {
        source_root: source_root.as_ref().to_path_buf(),
        destination_root: destination_root.as_ref().to_path_buf(),
        mode: ConversionMode::DryRun,
    })
}

pub fn execute(
    source_root: impl AsRef<Path>,
    destination_root: impl AsRef<Path>,
) -> Result<ConversionReport, String> {
    convert(ConversionRequest {
        source_root: source_root.as_ref().to_path_buf(),
        destination_root: destination_root.as_ref().to_path_buf(),
        mode: ConversionMode::Execute,
    })
}

pub fn convert(request: ConversionRequest) -> Result<ConversionReport, String> {
    let source_root = canonical_source_root(&request.source_root)?;
    validate_destination_root(&source_root, &request.destination_root)?;
    let source_path = source_root.join(LEGACY_DATABASE_FILE);
    let source = open_source(&source_path)?;
    validate_legacy_schema(&source)?;

    let mut audit = Audit::new(&source)?;
    audit.inspect_unsupported(&source)?;
    audit.inspect_blob_references(&source, &source_root)?;
    validate_query_sites(&source)?;
    let source_counts = audit.counts.clone();
    let dry_run = request.mode == ConversionMode::DryRun;

    if dry_run {
        return Ok(ConversionReport {
            source_schema_version: LEGACY_SCHEMA_VERSION,
            destination_schema_version: replacement_schema::CURRENT_SCHEMA_VERSION,
            dry_run: true,
            source_counts,
            destination_counts: None,
            copied_blob_files: 0,
            copied_blob_bytes: 0,
            copied_gallery_dl_archive: false,
            discarded_fields: audit.discarded,
            unmapped_fields: audit.unmapped,
            mismatches: Vec::new(),
        });
    }

    if !audit.unmapped.is_empty() {
        return Err(format!(
            "Replacement conversion has unmapped state: {}",
            audit.unmapped.join(", ")
        ));
    }

    let mut prepared = PreparedDestination::prepare(&request.destination_root)?;
    verify_source_blob_references(&source, &source_root)?;
    let blob_stats = copy_blob_tree(&source_root, &prepared.staging_root)?;
    let copied_gallery_dl_archive =
        copy_optional_root_file(&source_root, &prepared.staging_root, "gdl-archive.sqlite3")?;
    let destination = Store::open(&prepared.staging_root)?;

    let mappings = source_snapshot(&source)?;
    destination
        .transaction(|transaction| write_snapshot(transaction, &mappings))
        .map_err(|error| format!("Failed to write replacement library: {error}"))?;
    drop(destination);

    // Reopen through the replacement Store. This validates the exact replacement schema,
    // not merely the SQL statements used by this converter.
    let verified = Store::open(&prepared.staging_root)?;
    let destination_counts = read_destination_counts(&verified)?;
    verify_destination_blobs(&verified, &prepared.staging_root)?;
    let mismatches = compare_counts(&source_counts, &destination_counts);
    if !mismatches.is_empty() {
        return Err(format!(
            "Replacement conversion count verification failed: {}",
            format_mismatches(&mismatches)
        ));
    }
    prepared.commit()?;

    Ok(ConversionReport {
        source_schema_version: LEGACY_SCHEMA_VERSION,
        destination_schema_version: replacement_schema::CURRENT_SCHEMA_VERSION,
        dry_run: false,
        source_counts,
        destination_counts: Some(destination_counts),
        copied_blob_files: blob_stats.files,
        copied_blob_bytes: blob_stats.bytes,
        copied_gallery_dl_archive,
        discarded_fields: audit.discarded,
        unmapped_fields: audit.unmapped,
        mismatches,
    })
}

fn canonical_source_root(path: &Path) -> Result<PathBuf, String> {
    let root = fs::canonicalize(path)
        .map_err(|error| format!("Cannot open source root {}: {error}", path.display()))?;
    let database = root.join(LEGACY_DATABASE_FILE);
    if !database.is_file() {
        return Err(format!(
            "Source root has no {LEGACY_DATABASE_FILE}: {}",
            root.display()
        ));
    }
    Ok(root)
}

fn validate_destination_root(source_root: &Path, destination_root: &Path) -> Result<(), String> {
    if source_root == destination_root {
        return Err("Source and destination roots must be different".to_string());
    }
    if destination_root.exists() {
        let destination = fs::canonicalize(destination_root).map_err(|error| {
            format!(
                "Cannot inspect destination root {}: {error}",
                destination_root.display()
            )
        })?;
        if destination == source_root || destination.starts_with(source_root) {
            return Err("Destination cannot be the source root or a child of it".to_string());
        }
        let mut entries = fs::read_dir(&destination)
            .map_err(|error| format!("Cannot inspect destination root: {error}"))?;
        if entries.next().is_some() {
            return Err(format!(
                "Destination root must be absent or empty: {}",
                destination.display()
            ));
        }
    } else if let Some(parent) = destination_root.parent() {
        let parent = fs::canonicalize(parent)
            .map_err(|error| format!("Cannot inspect destination parent: {error}"))?;
        if parent.starts_with(source_root) {
            return Err("Destination cannot be created inside the source root".to_string());
        }
    }
    Ok(())
}

struct PreparedDestination {
    destination_root: PathBuf,
    staging_root: PathBuf,
    backup_root: Option<PathBuf>,
    committed: bool,
}

impl PreparedDestination {
    fn prepare(destination_root: &Path) -> Result<Self, String> {
        let parent = destination_root.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create destination parent: {error}"))?;
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("System clock is invalid: {error}"))?
            .as_nanos();
        let prefix = destination_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("library");
        let staging_root = parent.join(format!(
            ".{prefix}.picto-staging-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&staging_root)
            .map_err(|error| format!("Failed to create conversion staging root: {error}"))?;

        let backup_root = if destination_root.exists() {
            let backup_root = parent.join(format!(
                ".{prefix}.picto-empty-backup-{}-{suffix}",
                std::process::id()
            ));
            if let Err(error) = fs::rename(destination_root, &backup_root) {
                let _ = fs::remove_dir_all(&staging_root);
                return Err(format!("Failed to reserve empty destination root: {error}"));
            }
            Some(backup_root)
        } else {
            None
        };

        Ok(Self {
            destination_root: destination_root.to_path_buf(),
            staging_root,
            backup_root,
            committed: false,
        })
    }

    fn commit(&mut self) -> Result<(), String> {
        fs::rename(&self.staging_root, &self.destination_root)
            .map_err(|error| format!("Failed to commit converted library: {error}"))?;
        self.committed = true;
        if let Some(backup_root) = self.backup_root.take() {
            let _ = fs::remove_dir_all(backup_root);
        }
        Ok(())
    }
}

impl Drop for PreparedDestination {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let _ = fs::remove_dir_all(&self.staging_root);
        if let Some(backup_root) = self.backup_root.take() {
            if !self.destination_root.exists() {
                let _ = fs::rename(backup_root, &self.destination_root);
            } else {
                let _ = fs::remove_dir_all(backup_root);
            }
        }
    }
}

fn open_source(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("Failed to open source database read-only: {error}"))?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
        .map_err(|error| format!("Failed to configure source database: {error}"))?;
    Ok(connection)
}

fn validate_legacy_schema(connection: &Connection) -> Result<(), String> {
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
        .map_err(|error| format!("Source is not a schema-117 library: {error}"))?;
    let version: i64 = connection
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .map_err(|error| format!("Source schema version is unreadable: {error}"))?;
    if count != 1 || version != LEGACY_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported source schema: expected exactly one schema_version row at {LEGACY_SCHEMA_VERSION}, found {count} row(s) at {version}"
        ));
    }
    Ok(())
}

fn canonical_domain(site_id: &str) -> Result<String, String> {
    site_by_id(site_id)
        .map(|site| site.domain.to_string())
        .ok_or_else(|| {
            format!(
                "Legacy subscription_query site_id '{site_id}' is not present in the source catalog"
            )
        })
}

fn validate_query_sites(connection: &Connection) -> Result<(), String> {
    let site_ids: Vec<String> = rows(
        connection,
        "SELECT DISTINCT site_id FROM subscription_query",
        |row| row.get(0),
    )?;
    for site_id in site_ids {
        canonical_domain(&site_id)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct Audit {
    counts: ConversionCounts,
    discarded: Vec<String>,
    unmapped: Vec<String>,
}

impl Audit {
    fn new(connection: &Connection) -> Result<Self, String> {
        let mut audit = Self::default();
        audit.counts.media_files = count(connection, "media_file")?;
        audit.counts.media_entities = count(connection, "media_entity")?;
        audit.counts.media_views = count(connection, "media_view")?;
        audit.counts.tags = count(connection, "tag")?;
        audit.counts.entity_tags = count(connection, "entity_tag")?;
        audit.counts.tag_aliases = count(connection, "tag_alias")?;
        audit.counts.tag_implications = count(connection, "tag_implication")?;
        audit.counts.folders = count(connection, "folder")?;
        audit.counts.folder_members = count(connection, "folder_member")?;
        audit.counts.smart_folders = count(connection, "smart_folder")?;
        audit.counts.subscriptions = count(connection, "subscription")?;
        audit.counts.subscription_queries = count(connection, "subscription_query")?;
        audit.counts.subscription_runs = count(connection, "subscription_run")?;
        audit.counts.subscription_query_runs = connection
            .query_row(
                "SELECT COUNT(*) FROM (SELECT run_id, query_id FROM subscription_query_run WHERE run_id IS NOT NULL AND query_id IS NOT NULL GROUP BY run_id, query_id)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("Failed to count normalized subscription query runs: {error}"))?;
        audit.counts.subscription_issues = count(connection, "subscription_issue")?;
        audit.counts.source_posts = connection
            .query_row(
                "SELECT COUNT(DISTINCT site_id || char(0) || post_id) FROM subscription_post_member",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("Failed to count source posts: {error}"))?;
        audit.counts.source_items = connection
            .query_row(
                "SELECT COUNT(DISTINCT site_id || char(0) || post_id || char(0) || item_key) FROM subscription_post_member",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("Failed to count source items: {error}"))?;
        audit.counts.subscription_post_members = count(connection, "subscription_post_member")?;
        audit.counts.ingest_items = count(connection, "ingest_queue_item")?;
        audit.counts.work_items = count(connection, "deferred_work_item")?;
        audit.counts.credentials = count(connection, "credential_domain")?;
        audit.counts.credential_health = count(connection, "credential_health")?;
        audit.counts.duplicates = count(connection, "duplicate")?;
        audit.counts.file_colors = count(connection, "file_color")?;
        audit.counts.view_preferences = count(connection, "view_pref")?;
        audit.counts.settings = count(connection, "kv_settings")?;
        Ok(audit)
    }

    fn inspect_unsupported(&mut self, connection: &Connection) -> Result<(), String> {
        for (table, label) in [
            (
                "media_file_phash_index",
                "media_file_phash_index is rebuildable and is not copied",
            ),
            (
                "tag_ancestor",
                "tag_ancestor is a rebuildable projection and is not copied",
            ),
            (
                "entity_tag_implied",
                "entity_tag_implied is a rebuildable projection and is not copied",
            ),
            ("tag_display", "tag_display has no replacement table"),
            (
                "sidebar_node",
                "sidebar_node is a rebuildable projection and is not copied",
            ),
            ("manifest", "manifest is replacement-only projection state"),
            ("op_outbox", "op_outbox has no replacement sync owner"),
            (
                "sync_conflict_clock",
                "sync_conflict_clock has no replacement sync owner",
            ),
            (
                "sync_ingest_cursor",
                "sync_ingest_cursor has no replacement sync owner",
            ),
            (
                "sync_missing_blob",
                "sync_missing_blob has no replacement sync owner",
            ),
            (
                "file_color_rtree",
                "file_color_rtree is rebuildable and is not copied",
            ),
        ] {
            if table_exists(connection, table)? {
                let rows = count(connection, table)?;
                if rows > 0 {
                    self.discarded.push(format!("{label} ({rows} row(s))"));
                }
            }
        }
        self.discarded_warning(
            connection,
            "folder.auto_tags",
            "SELECT COUNT(*) FROM folder WHERE auto_tags IS NOT NULL",
        )?;
        self.discarded_warning(connection, "folder.watch_import_status_mode", "SELECT COUNT(*) FROM folder WHERE watch_import_status_mode IS NOT NULL AND watch_import_status_mode != 'inherit'")?;
        self.discarded_warning(
            connection,
            "folder derived/pin fields",
            "SELECT COUNT(*) FROM folder WHERE total_size_bytes != 0 OR pinned != 0 OR pin_order != 0",
        )?;
        self.discarded_warning(
            connection,
            "smart_folder derived/pin fields",
            "SELECT COUNT(*) FROM smart_folder WHERE total_size_bytes != 0 OR pinned != 0 OR pin_order != 0",
        )?;
        self.discarded_warning(
            connection,
            "subscription_entity",
            "SELECT COUNT(*) FROM subscription_entity",
        )?;
        self.discarded_warning(
            connection,
            "ingest_queue metadata",
            "SELECT COUNT(*) FROM ingest_queue WHERE cleanup_root IS NOT NULL OR post_id IS NOT NULL OR category IS NOT NULL",
        )?;
        self.discarded_warning(
            connection,
            "subscription query counters/check fields",
            "SELECT COUNT(*) FROM subscription_query WHERE last_check_time IS NOT NULL OR files_found != 0 OR posts_found != 0 OR resume_strategy IS NOT NULL",
        )?;
        self.discarded_warning(
            connection,
            "subscription run aggregate counters",
            "SELECT COUNT(*) FROM subscription_run WHERE files_downloaded != 0 OR files_skipped != 0 OR metadata_validated != 0 OR metadata_invalid != 0",
        )?;
        self.discarded_warning(
            connection,
            "subscription_query_job",
            "SELECT COUNT(*) FROM subscription_query_job",
        )?;
        self.discarded_warning(
            connection,
            "superseded subscription query-run attempts",
            "SELECT COALESCE(SUM(run_count - 1), 0) FROM (SELECT COUNT(*) AS run_count FROM subscription_query_run GROUP BY run_id, query_id HAVING run_count > 1)",
        )?;
        self.discarded_warning(
            connection,
            "orphan subscription query-run rows",
            "SELECT COUNT(*) FROM subscription_query_run WHERE run_id IS NULL OR query_id IS NULL",
        )?;
        self.discarded_warning(
            connection,
            "subscription_download_attempt",
            "SELECT COUNT(*) FROM subscription_download_attempt",
        )?;
        self.discarded_warning(connection, "legacy duplicate decision fields", "SELECT COUNT(*) FROM duplicate WHERE decision_source IS NOT NULL OR decision_reason IS NOT NULL OR loser_file_id IS NOT NULL")?;
        self.non_null_warning(
            connection,
            "unknown media lifecycle values",
            "SELECT COUNT(*) FROM media_entity WHERE status NOT IN (0,1,2)",
        )?;
        self.non_null_warning(
            connection,
            "unknown subscription run statuses",
            "SELECT COUNT(*) FROM subscription_run WHERE status NOT IN ('pending','running','succeeded','failed','cancelled')",
        )?;
        self.non_null_warning(
            connection,
            "unknown subscription query run statuses",
            "SELECT COUNT(*) FROM subscription_query_run WHERE status NOT IN ('pending','running','succeeded','failed','cancelled')",
        )?;
        self.non_null_warning(
            connection,
            "unknown ingest item statuses",
            "SELECT COUNT(*) FROM ingest_queue_item WHERE status NOT IN ('pending','processing','complete','succeeded','failed')",
        )?;
        self.non_null_warning(
            connection,
            "unknown deferred work statuses",
            "SELECT COUNT(*) FROM deferred_work_item WHERE status NOT IN ('pending','running')",
        )?;
        self.non_null_warning(
            connection,
            "unknown deferred work types",
            "SELECT COUNT(*) FROM deferred_work_item WHERE work_type NOT IN ('thumbnail','dominant_colors','perceptual_hash','blob_delete','ai_tag')",
        )?;
        self.non_null_warning(
            connection,
            "unknown source item statuses",
            "SELECT COUNT(*) FROM subscription_post_member WHERE status NOT IN ('pending','downloaded','ingested','imported','complete','succeeded','reused','failed','error','deleted')",
        )?;
        self.non_null_warning(
            connection,
            "source items without a replacement query link",
            "SELECT COUNT(*) FROM subscription_post_member member WHERE NOT EXISTS (SELECT 1 FROM subscription_query query WHERE query.subscription_id = member.subscription_id)",
        )?;
        Ok(())
    }

    fn discarded_warning(
        &mut self,
        connection: &Connection,
        label: &str,
        sql: &str,
    ) -> Result<(), String> {
        let rows: i64 = connection
            .query_row(sql, [], |row| row.get(0))
            .map_err(|error| format!("Failed to inspect discarded field {label}: {error}"))?;
        if rows > 0 {
            self.discarded.push(format!("{label} ({rows} row(s))"));
        }
        Ok(())
    }

    fn non_null_warning(
        &mut self,
        connection: &Connection,
        label: &str,
        sql: &str,
    ) -> Result<(), String> {
        let rows: i64 = connection
            .query_row(sql, [], |row| row.get(0))
            .map_err(|error| format!("Failed to inspect unmapped field {label}: {error}"))?;
        if rows > 0 {
            self.unmapped.push(format!("{label} ({rows} row(s))"));
        }
        Ok(())
    }

    fn inspect_blob_references(
        &mut self,
        connection: &Connection,
        source_root: &Path,
    ) -> Result<(), String> {
        let missing = connection
            .prepare("SELECT file_hash,mime_type FROM media_file")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .map_err(|error| format!("Failed to inspect source blobs: {error}"))?
            .into_iter()
            .filter(|(hash, mime)| {
                let (Some(first), Some(second)) = (hash.get(0..2), hash.get(2..4)) else {
                    return true;
                };
                !source_root
                    .join("blobs")
                    .join("f")
                    .join(first)
                    .join(second)
                    .join(format!("{hash}.{}", mime_to_extension(mime)))
                    .is_file()
            })
            .count();
        if missing > 0 {
            self.unmapped.push(format!(
                "media_file original blobs missing ({missing} file(s))"
            ));
        }
        Ok(())
    }
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE (type = 'table' OR type = 'view') AND name = ?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(|error| format!("Failed to inspect source table {table}: {error}"))
}

fn count(connection: &Connection, table: &str) -> Result<i64, String> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(|error| format!("Failed to count source table {table}: {error}"))
}

#[derive(Debug, Clone, Default)]
struct BlobStats {
    files: u64,
    bytes: u64,
}

fn copy_blob_tree(source_root: &Path, destination_root: &Path) -> Result<BlobStats, String> {
    let mut stats = BlobStats::default();
    for branch in ["f", "t"] {
        let source = source_root.join("blobs").join(branch);
        let destination = destination_root.join("blobs").join(branch);
        if source.exists() {
            copy_tree(&source, &destination, &mut stats)?;
        }
    }
    Ok(stats)
}

fn copy_optional_root_file(
    source_root: &Path,
    destination_root: &Path,
    name: &str,
) -> Result<bool, String> {
    let source = source_root.join(name);
    if !source.is_file() {
        return Ok(false);
    }
    let destination = destination_root.join(name);
    fs::copy(&source, &destination).map_err(|error| {
        format!(
            "Failed to copy auxiliary library file {}: {error}",
            source.display()
        )
    })?;
    Ok(true)
}

fn verify_source_blob_references(
    connection: &Connection,
    source_root: &Path,
) -> Result<(), String> {
    let records = blob_records(connection)?;
    verify_blob_records(source_root, &records, "source")
}

#[derive(Debug, Clone)]
struct BlobRecord {
    hash: String,
    mime: String,
    size: i64,
}

fn blob_records(connection: &Connection) -> Result<Vec<BlobRecord>, String> {
    rows(
        connection,
        "SELECT file_hash,mime_type,size_bytes FROM media_file ORDER BY file_id",
        |row| {
            Ok(BlobRecord {
                hash: row.get(0)?,
                mime: row.get(1)?,
                size: row.get(2)?,
            })
        },
    )
}

fn verify_destination_blobs(store: &Store, destination_root: &Path) -> Result<(), String> {
    let records = store.read_result(blob_records)?;
    verify_blob_records(destination_root, &records, "destination")
}

fn verify_blob_records(
    library_root: &Path,
    records: &[BlobRecord],
    label: &str,
) -> Result<(), String> {
    for record in records {
        let (Some(first), Some(second)) = (record.hash.get(0..2), record.hash.get(2..4)) else {
            return Err(format!(
                "{label} blob hash is not a valid content hash: {}",
                record.hash
            ));
        };
        let path = library_root
            .join("blobs")
            .join("f")
            .join(first)
            .join(second)
            .join(format!(
                "{}.{}",
                record.hash,
                mime_to_extension(&record.mime)
            ));
        if record.size < 0 {
            return Err(format!(
                "{label} blob {} has invalid negative size {}",
                record.hash, record.size
            ));
        }
        let metadata = fs::metadata(&path).map_err(|error| {
            format!(
                "{label} is missing referenced original blob {}: {error}",
                record.hash
            )
        })?;
        if metadata.len() != record.size as u64 {
            return Err(format!(
                "{label} blob {} has size {}, expected {}",
                record.hash,
                metadata.len(),
                record.size
            ));
        }
        let mut file = File::open(&path)
            .map_err(|error| format!("Failed to read {label} blob {}: {error}", record.hash))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| format!("Failed to hash {label} blob {}: {error}", record.hash))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(&record.hash) {
            return Err(format!(
                "{label} blob {} content hash is {}, expected {}",
                record.hash, actual, record.hash
            ));
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path, stats: &mut BlobStats) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("Cannot create blob directory: {error}"))?;
    for entry in
        fs::read_dir(source).map_err(|error| format!("Cannot read blob directory: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Cannot read blob entry: {error}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path, stats)?;
        } else if source_path.is_file() {
            if destination_path.exists() {
                return Err(format!(
                    "Destination blob already exists: {}",
                    destination_path.display()
                ));
            }
            let bytes = fs::copy(&source_path, &destination_path).map_err(|error| {
                format!("Failed to copy blob {}: {error}", source_path.display())
            })?;
            stats.files += 1;
            stats.bytes += bytes;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Snapshot {
    media_files: Vec<MediaFileRow>,
    entities: Vec<EntityRow>,
    media_views: Vec<MediaViewRow>,
    tags: Vec<TagRow>,
    entity_tags: Vec<EntityTagRow>,
    aliases: Vec<TagAliasRow>,
    implications: Vec<TagImplicationRow>,
    folders: Vec<FolderRow>,
    folder_members: Vec<FolderMemberRow>,
    smart_folders: Vec<SmartFolderRow>,
    subscriptions: Vec<SubscriptionRow>,
    queries: Vec<QueryRow>,
    runs: Vec<RunRow>,
    query_runs: Vec<QueryRunRow>,
    issues: Vec<IssueRow>,
    posts: Vec<PostRow>,
    source_items: Vec<SourceItemRow>,
    post_links: Vec<PostLinkRow>,
    run_source_items: Vec<RunSourceItemRow>,
    ingest_jobs: Vec<IngestJobRow>,
    work_items: Vec<WorkItemRow>,
    credentials: Vec<CredentialRow>,
    credential_health: Vec<CredentialHealthRow>,
    duplicates: Vec<DuplicateRow>,
    colors: Vec<ColorRow>,
    view_preferences: Vec<ViewPreferenceRow>,
    settings: Vec<SettingRow>,
}

macro_rules! get {
    ($row:expr, $index:expr) => {
        $row.get($index)?
    };
}

#[derive(Debug, Clone)]
struct MediaFileRow {
    id: i64,
    hash: String,
    mime: String,
    size: i64,
    width: Option<i64>,
    height: Option<i64>,
    duration: Option<i64>,
    frames: Option<i64>,
    audio: i64,
    phash: Option<String>,
    color: Option<String>,
    palette: Option<Vec<u8>>,
    color_version: i64,
    added: String,
}
#[derive(Debug, Clone)]
struct EntityRow {
    id: i64,
    hash: String,
    file_id: i64,
    status: i64,
    name: Option<String>,
    notes: Option<String>,
    rating: Option<i64>,
    urls: Option<String>,
    created: String,
    added: String,
    modified: String,
}
#[derive(Debug, Clone)]
struct MediaViewRow {
    id: i64,
    viewed: String,
}
#[derive(Debug, Clone)]
struct TagRow {
    id: i64,
    namespace: String,
    subtag: String,
}
#[derive(Debug, Clone)]
struct EntityTagRow {
    entity_id: i64,
    tag_id: i64,
    provenance: i64,
    source: String,
}
#[derive(Debug, Clone)]
struct TagAliasRow {
    from_id: i64,
    to_id: i64,
    source: String,
}
#[derive(Debug, Clone)]
struct TagImplicationRow {
    child_id: i64,
    parent_id: i64,
    source: String,
}
#[derive(Debug, Clone)]
struct FolderRow {
    id: i64,
    name: String,
    parent: Option<i64>,
    icon: Option<String>,
    color: Option<String>,
    notes: Option<String>,
    sort: Option<i64>,
    watch: Option<String>,
    enabled: i64,
    subfolders: i64,
    added: String,
    modified: String,
    key: String,
}
#[derive(Debug, Clone)]
struct FolderMemberRow {
    folder_id: i64,
    entity_id: i64,
    position: Option<i64>,
}
#[derive(Debug, Clone)]
struct SmartFolderRow {
    id: i64,
    name: String,
    parent: Option<i64>,
    icon: Option<String>,
    color: Option<String>,
    notes: Option<String>,
    predicate: String,
    sort_field: Option<String>,
    sort_order: Option<String>,
    display: Option<i64>,
    added: String,
    modified: String,
    key: String,
}
#[derive(Debug, Clone)]
struct SubscriptionRow {
    id: i64,
    key: String,
    name: String,
    schedule: String,
    paused: i64,
    initial_limit: Option<i64>,
    periodic_limit: Option<i64>,
    added: String,
}
#[derive(Debug, Clone)]
struct QueryRow {
    id: i64,
    key: String,
    subscription_id: i64,
    site: String,
    domain: String,
    kind: String,
    text: String,
    display: Option<String>,
    notes: Option<String>,
    paused: i64,
    cursor: Option<String>,
    complete: i64,
    success: Option<String>,
    failure: Option<String>,
    failure_kind: Option<String>,
    failure_message: Option<String>,
}
#[derive(Debug, Clone)]
struct RunRow {
    id: i64,
    subscription_id: i64,
    requested_by: String,
    status: String,
    started: Option<String>,
    finished: Option<String>,
    failure_kind: Option<String>,
    error: Option<String>,
    created: String,
}
#[derive(Debug, Clone)]
struct QueryRunRow {
    id: i64,
    run_id: Option<i64>,
    query_id: i64,
    status: String,
    cursor: Option<String>,
    attempts: i64,
    available: String,
    started: Option<String>,
    finished: Option<String>,
    failure_kind: Option<String>,
    error: Option<String>,
}
#[derive(Debug, Clone)]
struct IssueRow {
    id: i64,
    key: String,
    subscription_id: i64,
    query_id: Option<i64>,
    kind: String,
    message: String,
    detail: Option<String>,
    status: String,
    first: String,
    last: String,
    resolved: Option<String>,
}
#[derive(Debug, Clone)]
struct PostRow {
    id: i64,
    site: String,
    key: String,
    canonical: Option<String>,
    created: String,
    modified: String,
}
#[derive(Debug, Clone)]
struct SourceItemRow {
    id: i64,
    post_id: i64,
    key: String,
    position: i64,
    media_url: Option<String>,
    canonical: Option<String>,
    media_item_id: Option<i64>,
    state: String,
    error: Option<String>,
    created: String,
    modified: String,
}
#[derive(Debug, Clone)]
struct PostLinkRow {
    subscription_id: i64,
    query_id: i64,
    post_id: i64,
    last_run: Option<i64>,
}
#[derive(Debug, Clone)]
struct RunSourceItemRow {
    run_query_id: i64,
    source_item_id: i64,
}
#[derive(Debug, Clone)]
struct IngestJobRow {
    id: i64,
    key: String,
    source_kind: String,
    source_path: String,
    source_item_id: Option<i64>,
    payload: String,
    lifecycle: String,
    delete_after: i64,
    status: String,
    attempts: i64,
    available: String,
    error: Option<String>,
    created: String,
    modified: String,
}
#[derive(Debug, Clone)]
struct WorkItemRow {
    id: i64,
    hash: String,
    kind: String,
    status: String,
    attempts: i64,
    available: String,
    error: Option<String>,
    created: String,
    modified: String,
}
#[derive(Debug, Clone)]
struct CredentialRow {
    site: String,
    kind: String,
    display: Option<String>,
    created: String,
}
#[derive(Debug, Clone)]
struct CredentialHealthRow {
    site: String,
    status: String,
    checked: Option<String>,
    error: Option<String>,
}
#[derive(Debug, Clone)]
struct DuplicateRow {
    a: i64,
    b: i64,
    distance: i64,
    status: String,
    decided: Option<String>,
    winner: Option<i64>,
}
#[derive(Debug, Clone)]
struct ColorRow {
    id: i64,
    file_id: i64,
    hex: String,
    l: f64,
    a: f64,
    b: f64,
}
#[derive(Debug, Clone)]
struct ViewPreferenceRow {
    scope: String,
    value: String,
}
#[derive(Debug, Clone)]
struct SettingRow {
    key: String,
    value: String,
}

#[derive(Debug, Clone)]
struct LegacyPostMemberRow {
    subscription_id: i64,
    site: String,
    post: String,
    item: String,
    page: Option<i64>,
    canonical: Option<String>,
    media_url: Option<String>,
    entity_id: Option<i64>,
    status: String,
    created: String,
    modified: String,
}

fn source_snapshot(connection: &Connection) -> Result<Snapshot, String> {
    let mut snapshot = Snapshot {
        media_files: rows(connection, "SELECT file_id,file_hash,mime_type,size_bytes,pixel_width,pixel_height,duration_ms,frame_count,has_audio,perceptual_hash,dominant_color_hex,dominant_palette_blob,color_analysis_version,date_added FROM media_file ORDER BY file_id", |r| Ok(MediaFileRow { id: get!(r,0), hash: get!(r,1), mime: get!(r,2), size: get!(r,3), width: get!(r,4), height: get!(r,5), duration: get!(r,6), frames: get!(r,7), audio: get!(r,8), phash: get!(r,9), color: get!(r,10), palette: get!(r,11), color_version: get!(r,12), added: get!(r,13) }))?,
        entities: rows(connection, "SELECT entity_id,entity_hash,file_id,status,name,notes,rating,source_urls_json,date_created,date_added,date_modified FROM media_entity ORDER BY entity_id", |r| Ok(EntityRow { id: get!(r,0), hash: get!(r,1), file_id: get!(r,2), status: get!(r,3), name: get!(r,4), notes: get!(r,5), rating: get!(r,6), urls: get!(r,7), created: get!(r,8), added: get!(r,9), modified: get!(r,10) }))?,
        media_views: rows(connection, "SELECT entity_id,viewed_at FROM media_view ORDER BY entity_id", |r| Ok(MediaViewRow { id: get!(r,0), viewed: get!(r,1) }))?,
        tags: rows(connection, "SELECT tag_id,namespace,subtag FROM tag ORDER BY tag_id", |r| Ok(TagRow { id: get!(r,0), namespace: get!(r,1), subtag: get!(r,2) }))?,
        entity_tags: rows(connection, "SELECT entity_id,tag_id,provenance_mask,source FROM entity_tag ORDER BY entity_id,tag_id,source", |r| Ok(EntityTagRow { entity_id: get!(r,0), tag_id: get!(r,1), provenance: get!(r,2), source: get!(r,3) }))?,
        aliases: rows(connection, "SELECT from_tag_id,to_tag_id,source FROM tag_alias ORDER BY from_tag_id,source", |r| Ok(TagAliasRow { from_id: get!(r,0), to_id: get!(r,1), source: get!(r,2) }))?,
        implications: rows(connection, "SELECT child_tag_id,parent_tag_id,source FROM tag_implication ORDER BY child_tag_id,parent_tag_id,source", |r| Ok(TagImplicationRow { child_id: get!(r,0), parent_id: get!(r,1), source: get!(r,2) }))?,
        folders: rows(connection, "SELECT folder_id,name,parent_id,icon,color,notes,sort_order,watch_path,watch_enabled,watch_subfolders,date_added,date_modified,COALESCE(uuid,'') FROM folder ORDER BY folder_id", |r| { let id: i64=get!(r,0); let uuid: String=get!(r,12); Ok(FolderRow { id, name:get!(r,1), parent:get!(r,2), icon:get!(r,3), color:get!(r,4), notes:get!(r,5), sort:get!(r,6), watch:get!(r,7), enabled:get!(r,8), subfolders:get!(r,9), added:get!(r,10), modified:get!(r,11), key: if uuid.is_empty(){format!("legacy-folder-{id}")}else{uuid} }) })?,
        folder_members: rows(connection, "SELECT folder_id,entity_id,position_rank FROM folder_member ORDER BY folder_id,entity_id", |r| Ok(FolderMemberRow { folder_id:get!(r,0), entity_id:get!(r,1), position:get!(r,2) }))?,
        smart_folders: rows(connection, "SELECT smart_folder_id,name,parent_id,icon,color,notes,predicate_json,sort_field,sort_order,display_order,date_added,date_modified,COALESCE(uuid,'') FROM smart_folder ORDER BY smart_folder_id", |r| { let id:i64=get!(r,0); let uuid:String=get!(r,12); Ok(SmartFolderRow { id, name:get!(r,1), parent:get!(r,2), icon:get!(r,3), color:get!(r,4), notes:get!(r,5), predicate:get!(r,6), sort_field:get!(r,7), sort_order:get!(r,8), display:get!(r,9), added:get!(r,10), modified:get!(r,11), key:if uuid.is_empty(){format!("legacy-smart-folder-{id}")}else{uuid} }) })?,
        subscriptions: rows(connection, "SELECT subscription_id,COALESCE(uuid,''),name,schedule,paused,initial_post_limit,periodic_post_limit,date_added FROM subscription ORDER BY subscription_id", |r| { let id:i64=get!(r,0); let uuid:String=get!(r,1); Ok(SubscriptionRow { id, key:if uuid.is_empty(){format!("legacy-subscription-{id}")}else{uuid}, name:get!(r,2), schedule:get!(r,3), paused:get!(r,4), initial_limit:get!(r,5), periodic_limit:get!(r,6), added:get!(r,7) }) })?,
        queries: rows(connection, "SELECT query_id,COALESCE(uuid,''),subscription_id,site_id,query_kind,query_text,display_name,notes,paused,resume_cursor,completed_initial_run,last_success_at,last_failure_at,last_failure_kind,last_failure_message FROM subscription_query ORDER BY query_id", |r| {
            let id: i64 = get!(r, 0);
            let uuid: String = get!(r, 1);
            let site: String = get!(r, 3);
            let domain = canonical_domain(&site).map_err(conversion_sql_error)?;
            Ok(QueryRow {
                id,
                key: if uuid.is_empty() {
                    format!("legacy-query-{id}")
                } else {
                    uuid
                },
                subscription_id: get!(r, 2),
                site,
                domain,
                kind: get!(r, 4),
                text: get!(r, 5),
                display: get!(r, 6),
                notes: get!(r, 7),
                paused: get!(r, 8),
                cursor: get!(r, 9),
                complete: get!(r, 10),
                success: get!(r, 11),
                failure: get!(r, 12),
                failure_kind: get!(r, 13),
                failure_message: get!(r, 14),
            })
        })?,
        runs: rows(connection, "SELECT run_id,subscription_id,'legacy',status,started_at,finished_at,failure_kind,error_message,started_at FROM subscription_run ORDER BY run_id", |r| Ok(RunRow { id:get!(r,0), subscription_id:get!(r,1), requested_by:get!(r,2), status:get!(r,3), started:get!(r,4), finished:get!(r,5), failure_kind:get!(r,6), error:get!(r,7), created:get!(r,8) }))?,
        query_runs: rows(connection, "SELECT query_run_id,run_id,query_id,status,started_at,finished_at,failure_kind,error_message,(SELECT COUNT(*) - 1 FROM subscription_query_run attempts WHERE attempts.run_id = current.run_id AND attempts.query_id = current.query_id) FROM subscription_query_run current WHERE query_run_id = (SELECT MAX(latest.query_run_id) FROM subscription_query_run latest WHERE latest.run_id = current.run_id AND latest.query_id = current.query_id) ORDER BY query_run_id", |r| Ok(QueryRunRow { id:get!(r,0), run_id:get!(r,1), query_id:get!(r,2), status:get!(r,3), cursor:None, attempts:get!(r,8), available:get!(r,4), started:get!(r,4), finished:get!(r,5), failure_kind:get!(r,6), error:get!(r,7) }))?,
        issues: rows(connection, "SELECT issue_id,issue_key,subscription_id,query_id,issue_kind,message,detail,status,first_seen_at,last_seen_at,resolved_at FROM subscription_issue ORDER BY issue_id", |r| Ok(IssueRow { id:get!(r,0), key:get!(r,1), subscription_id:get!(r,2), query_id:get!(r,3), kind:get!(r,4), message:get!(r,5), detail:get!(r,6), status:get!(r,7), first:get!(r,8), last:get!(r,9), resolved:get!(r,10) }))?,
        posts: Vec::new(), source_items: Vec::new(), post_links: Vec::new(), run_source_items: Vec::new(),
        ingest_jobs: rows(connection, "SELECT item_id, 'legacy-ingest-' || item_id, 'legacy', source_path, NULL, payload_json, 'inbox', delete_after_ingest, CASE status WHEN 'processing' THEN 'running' WHEN 'complete' THEN 'succeeded' ELSE status END, 0, created_at, last_error, created_at, updated_at FROM ingest_queue_item ORDER BY item_id", |r| Ok(IngestJobRow { id:get!(r,0), key:get!(r,1), source_kind:get!(r,2), source_path:get!(r,3), source_item_id:get!(r,4), payload:get!(r,5), lifecycle:get!(r,6), delete_after:get!(r,7), status:get!(r,8), attempts:get!(r,9), available:get!(r,10), error:get!(r,11), created:get!(r,12), modified:get!(r,13) }))?,
        work_items: rows(connection, "SELECT work_id,entity_hash,work_type,status,attempt_count,available_at,last_error,queued_at,COALESCE(last_error_at,queued_at) FROM deferred_work_item ORDER BY work_id", |r| Ok(WorkItemRow { id:get!(r,0), hash:get!(r,1), kind:get!(r,2), status:get!(r,3), attempts:get!(r,4), available:get!(r,5), error:get!(r,6), created:get!(r,7), modified:get!(r,8) }))?,
        credentials: rows(connection, "SELECT site_category,credential_type,display_name,date_added FROM credential_domain ORDER BY site_category", |r| Ok(CredentialRow { site:get!(r,0), kind:get!(r,1), display:get!(r,2), created:get!(r,3) }))?,
        credential_health: rows(connection, "SELECT site_category,health_status,last_checked_at,last_error FROM credential_health ORDER BY site_category", |r| Ok(CredentialHealthRow { site:get!(r,0), status:get!(r,1), checked:get!(r,2), error:get!(r,3) }))?,
        duplicates: rows(connection, "SELECT file_id_a,file_id_b,distance,status,decision_at,winner_file_id FROM duplicate ORDER BY file_id_a,file_id_b", |r| Ok(DuplicateRow { a:get!(r,0), b:get!(r,1), distance:get!(r,2), status:get!(r,3), decided:get!(r,4), winner:get!(r,5) }))?,
        colors: rows(connection, "SELECT rowid,file_id,hex,l,a,b FROM file_color ORDER BY rowid", |r| Ok(ColorRow { id:get!(r,0), file_id:get!(r,1), hex:get!(r,2), l:get!(r,3), a:get!(r,4), b:get!(r,5) }))?,
        view_preferences: rows(connection, "SELECT scope,json_object('sort_field',sort_field,'sort_order',sort_dir,'view_mode',layout,'target_size',tile_size,'show_name',show_name,'show_resolution',show_resolution,'show_extension',show_extension,'show_label',show_label,'thumbnail_fit',thumbnail_fit) FROM view_pref ORDER BY scope", |r| Ok(ViewPreferenceRow { scope:get!(r,0), value:get!(r,1) }))?,
        settings: rows(connection, "SELECT key,COALESCE(value,'null') FROM kv_settings ORDER BY key", |r| Ok(SettingRow { key:get!(r,0), value:get!(r,1) }))?,
    };
    populate_provenance(connection, &mut snapshot)?;
    validate_snapshot_values(&snapshot)?;
    Ok(snapshot)
}

fn validate_snapshot_values(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot
        .entities
        .iter()
        .any(|entity| !matches!(entity.status, 0 | 1 | 2))
    {
        return Err("Legacy media_entity contains an unknown lifecycle status".to_string());
    }
    for run in &snapshot.runs {
        validate_status(
            "subscription_run",
            &run.status,
            ["pending", "running", "succeeded", "failed", "cancelled"],
        )?;
    }
    for query_run in &snapshot.query_runs {
        validate_status(
            "subscription_query_run",
            &query_run.status,
            ["pending", "running", "succeeded", "failed", "cancelled"],
        )?;
    }
    for ingest in &snapshot.ingest_jobs {
        validate_status(
            "ingest_queue_item",
            &ingest.status,
            ["pending", "running", "succeeded", "failed"],
        )?;
    }
    for work in &snapshot.work_items {
        validate_status("deferred_work_item", &work.status, ["pending", "running"])?;
        normalize_work_type(&work.kind)?;
    }
    for item in &snapshot.source_items {
        validate_status(
            "subscription_post_member",
            &item.state,
            ["pending", "downloaded", "ingested", "failed", "deleted"],
        )?;
    }
    Ok(())
}

fn validate_status<const N: usize>(
    table: &str,
    value: &str,
    allowed: [&str; N],
) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("{table} contains unsupported status '{value}'"))
    }
}

fn rows<T>(
    connection: &Connection,
    sql: &str,
    map: impl Fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("Failed to read legacy data: {error}"))?;
    let values = statement
        .query_map([], map)
        .map_err(|error| format!("Failed to read legacy data: {error}"))?;
    values
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("Failed to decode legacy data: {error}"))
}

fn populate_provenance(connection: &Connection, snapshot: &mut Snapshot) -> Result<(), String> {
    let members = rows(
        connection,
        "SELECT subscription_id,site_id,post_id,item_key,page_num,canonical_post_url,media_url,entity_id,status,created_at,updated_at FROM subscription_post_member ORDER BY site_id,post_id,item_key,subscription_id",
        |row| {
            Ok(LegacyPostMemberRow {
                subscription_id: get!(row, 0),
                site: get!(row, 1),
                post: get!(row, 2),
                item: get!(row, 3),
                page: get!(row, 4),
                canonical: get!(row, 5),
                media_url: get!(row, 6),
                entity_id: get!(row, 7),
                status: get!(row, 8),
                created: get!(row, 9),
                modified: get!(row, 10),
            })
        },
    )?;
    let entity_ids = snapshot
        .entities
        .iter()
        .map(|entity| entity.id)
        .collect::<BTreeSet<_>>();
    let query_by_subscription = snapshot
        .queries
        .iter()
        .map(|query| (query.subscription_id, query.id))
        .collect::<HashMap<_, _>>();
    let mut post_ids = HashMap::<(String, String), i64>::new();
    let mut item_ids = HashMap::<(i64, String), i64>::new();
    let mut next_post_id = 1i64;
    let mut next_source_item_id = 1i64;

    for member in &members {
        let post_key = (member.site.clone(), member.post.clone());
        let post_id = if let Some(id) = post_ids.get(&post_key) {
            *id
        } else {
            let id = next_post_id;
            next_post_id += 1;
            post_ids.insert(post_key, id);
            snapshot.posts.push(PostRow {
                id,
                site: member.site.clone(),
                key: member.post.clone(),
                canonical: member.canonical.clone(),
                created: member.created.clone(),
                modified: member.modified.clone(),
            });
            id
        };
        let item_key = (post_id, member.item.clone());
        if !item_ids.contains_key(&item_key) {
            let id = next_source_item_id;
            next_source_item_id += 1;
            item_ids.insert(item_key, id);
            snapshot.source_items.push(SourceItemRow {
                id,
                post_id,
                key: member.item.clone(),
                position: member.page.unwrap_or(0),
                media_url: member.media_url.clone(),
                canonical: member.canonical.clone(),
                media_item_id: member.entity_id.filter(|id| entity_ids.contains(id)),
                state: source_item_state(&member.status)
                    .ok_or_else(|| {
                        format!(
                            "subscription_post_member contains unsupported status '{}'",
                            member.status
                        )
                    })?
                    .to_string(),
                error: None,
                created: member.created.clone(),
                modified: member.modified.clone(),
            });
        }
        let query_id = query_for_member(
            connection,
            member,
            query_by_subscription.get(&member.subscription_id).copied(),
        )?;
        if !snapshot.post_links.iter().any(|link| {
            link.subscription_id == member.subscription_id
                && link.query_id == query_id
                && link.post_id == post_id
        }) {
            snapshot.post_links.push(PostLinkRow {
                subscription_id: member.subscription_id,
                query_id,
                post_id,
                last_run: None,
            });
        }
    }

    let attempts = rows(
        connection,
        "SELECT (SELECT MAX(latest.query_run_id) FROM subscription_query_run latest WHERE latest.run_id = current.run_id AND latest.query_id = current.query_id),attempt.subscription_id,COALESCE(attempt.site_category,''),attempt.post_id,attempt.item_key FROM subscription_download_attempt attempt JOIN subscription_query_run current ON current.query_run_id = attempt.query_run_id WHERE attempt.query_run_id IS NOT NULL AND attempt.post_id IS NOT NULL",
        |row| {
            let query_run_id: i64 = get!(row, 0);
            let subscription_id: i64 = get!(row, 1);
            let site: String = get!(row, 2);
            let post: String = get!(row, 3);
            let item: String = get!(row, 4);
            Ok((query_run_id, subscription_id, site, post, item))
        },
    )?;
    let post_lookup = snapshot
        .posts
        .iter()
        .map(|post| ((post.site.clone(), post.key.clone()), post.id))
        .collect::<HashMap<_, _>>();
    let item_lookup = snapshot
        .source_items
        .iter()
        .map(|item| ((item.post_id, item.key.clone()), item.id))
        .collect::<HashMap<_, _>>();
    for (query_run_id, _subscription_id, site, post, item) in attempts {
        let Some(post_id) = post_lookup.get(&(site, post)).copied() else {
            continue;
        };
        let Some(source_item_id) = item_lookup.get(&(post_id, item)).copied() else {
            continue;
        };
        if !snapshot
            .run_source_items
            .iter()
            .any(|link| link.run_query_id == query_run_id && link.source_item_id == source_item_id)
        {
            snapshot.run_source_items.push(RunSourceItemRow {
                run_query_id: query_run_id,
                source_item_id,
            });
        }
    }
    Ok(())
}

fn query_for_member(
    connection: &Connection,
    member: &LegacyPostMemberRow,
    fallback: Option<i64>,
) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT query_id FROM subscription_download_attempt WHERE subscription_id = ?1 AND post_id = ?2 AND item_key = ?3 ORDER BY query_id LIMIT 1",
            params![member.subscription_id, member.post, member.item],
            |row| row.get(0),
        )
        .optional()
        .map(|query_id| query_id.or(fallback))
        .map_err(|error| format!("Failed to resolve source query: {error}"))
        .and_then(|query_id| {
            query_id.ok_or_else(|| {
                format!(
                    "Source item {} from subscription {} has no replacement query",
                    member.item, member.subscription_id
                )
            })
        })
}

fn source_item_state(status: &str) -> Option<&'static str> {
    match status {
        "deleted" => Some("deleted"),
        "failed" | "error" => Some("failed"),
        "downloaded" => Some("downloaded"),
        "ingested" | "imported" | "complete" | "succeeded" | "reused" => Some("ingested"),
        "pending" => Some("pending"),
        _ => None,
    }
}

fn write_snapshot(transaction: &Transaction<'_>, snapshot: &Snapshot) -> rusqlite::Result<()> {
    for file in &snapshot.media_files {
        transaction.execute("INSERT INTO media_file (file_id,file_hash,mime_type,size_bytes,pixel_width,pixel_height,duration_ms,frame_count,has_audio,perceptual_hash,dominant_color_hex,dominant_palette_blob,color_analysis_version,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)", params![file.id,file.hash,file.mime,file.size,file.width,file.height,file.duration,file.frames,file.audio,file.phash,file.color,file.palette,file.color_version,file.added])?;
    }
    for entity in &snapshot.entities {
        let lifecycle = lifecycle(entity.status).ok_or_else(|| {
            conversion_sql_error(format!(
                "Legacy media_entity contains unknown lifecycle status {}",
                entity.status
            ))
        })?;
        transaction.execute("INSERT INTO library_item (item_id,item_key,kind,label,created_at,updated_at) VALUES (?1,?2,'media',?3,?4,?5)", params![entity.id,entity.hash,entity.name,entity.created,entity.modified])?;
        transaction.execute(
            "INSERT INTO library_root (item_id,lifecycle) VALUES (?1,?2)",
            params![entity.id, lifecycle],
        )?;
        transaction.execute("INSERT INTO media_asset (item_id,file_id,name,notes,rating,source_urls_json,imported_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![entity.id,entity.file_id,entity.name,entity.notes,entity.rating,entity.urls,entity.added,entity.modified])?;
    }
    for view in &snapshot.media_views {
        transaction.execute(
            "INSERT INTO media_view (item_id,viewed_at) VALUES (?1,?2)",
            params![view.id, view.viewed],
        )?;
    }
    for tag in &snapshot.tags {
        transaction.execute(
            "INSERT INTO tag (tag_id,namespace,subtag) VALUES (?1,?2,?3)",
            params![tag.id, tag.namespace, tag.subtag],
        )?;
    }
    for tag in &snapshot.entity_tags {
        transaction.execute("INSERT INTO media_tag (media_item_id,tag_id,source,provenance_mask) VALUES (?1,?2,?3,?4)", params![tag.entity_id,tag.tag_id,tag.source,tag.provenance])?;
    }
    for alias in &snapshot.aliases {
        transaction.execute(
            "INSERT INTO tag_alias (from_tag_id,to_tag_id,source) VALUES (?1,?2,?3)",
            params![alias.from_id, alias.to_id, alias.source],
        )?;
    }
    for implication in &snapshot.implications {
        transaction.execute(
            "INSERT INTO tag_implication (child_tag_id,parent_tag_id,source) VALUES (?1,?2,?3)",
            params![
                implication.child_id,
                implication.parent_id,
                implication.source
            ],
        )?;
    }
    for folder in &snapshot.folders {
        transaction.execute("INSERT INTO folder (folder_id,folder_key,name,parent_id,icon,color,notes,sort_rank,watch_path,watch_enabled,watch_subfolders,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)", params![folder.id,folder.key,folder.name,folder.parent,folder.icon,folder.color,folder.notes,folder.sort,folder.watch,folder.enabled,folder.subfolders,folder.added,folder.modified])?;
    }
    for member in &snapshot.folder_members {
        transaction.execute(
            "INSERT INTO folder_item (folder_id,item_id,position_rank) VALUES (?1,?2,?3)",
            params![member.folder_id, member.entity_id, member.position],
        )?;
    }
    for smart in &snapshot.smart_folders {
        transaction.execute("INSERT INTO smart_folder (smart_folder_id,smart_folder_key,name,parent_id,icon,color,notes,predicate_json,sort_field,sort_order,display_order,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)", params![smart.id,smart.key,smart.name,smart.parent,smart.icon,smart.color,smart.notes,smart.predicate,smart.sort_field,smart.sort_order,smart.display,smart.added,smart.modified])?;
    }
    for subscription in &snapshot.subscriptions {
        transaction.execute("INSERT INTO subscription (subscription_id,subscription_key,name,schedule,paused,initial_post_limit,periodic_post_limit,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![subscription.id,subscription.key,subscription.name,subscription.schedule,subscription.paused,subscription.initial_limit,subscription.periodic_limit,subscription.added])?;
    }
    for query in &snapshot.queries {
        transaction.execute("INSERT INTO subscription_query (query_id,query_key,subscription_id,site_id,domain_key,query_kind,query_text,display_name,notes,paused,resume_cursor,initial_run_complete,last_success_at,last_failure_at,last_failure_kind,last_failure_message) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)", params![query.id,query.key,query.subscription_id,query.site,query.domain,query.kind,query.text,query.display,query.notes,query.paused,query.cursor,query.complete,query.success,query.failure,query.failure_kind,query.failure_message])?;
    }
    for run in &snapshot.runs {
        transaction.execute("INSERT INTO subscription_run (run_id,subscription_id,requested_by,status,started_at,finished_at,failure_kind,error_message,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![run.id,run.subscription_id,run.requested_by,run.status,run.started,run.finished,run.failure_kind,run.error,run.created])?;
    }
    for query_run in &snapshot.query_runs {
        transaction.execute("INSERT INTO subscription_run_query (run_query_id,run_id,query_id,status,resume_cursor,attempt_count,available_at,started_at,finished_at,failure_kind,error_message) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![query_run.id,query_run.run_id,query_run.query_id,query_run.status,query_run.cursor,query_run.attempts,query_run.available,query_run.started,query_run.finished,query_run.failure_kind,query_run.error])?;
    }
    for issue in &snapshot.issues {
        transaction.execute("INSERT INTO subscription_issue (issue_id,issue_key,subscription_id,query_id,issue_kind,message,detail,status,first_seen_at,last_seen_at,resolved_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![issue.id,issue.key,issue.subscription_id,issue.query_id,issue.kind,issue.message,issue.detail,issue.status,issue.first,issue.last,issue.resolved])?;
    }
    for post in &snapshot.posts {
        transaction.execute("INSERT INTO source_post (source_post_id,site_id,post_key,canonical_url,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6)", params![post.id,post.site,post.key,post.canonical,post.created,post.modified])?;
    }
    for link in &snapshot.post_links {
        transaction.execute("INSERT INTO subscription_source_post (subscription_id,query_id,source_post_id,last_seen_run_id) VALUES (?1,?2,?3,?4)", params![link.subscription_id,link.query_id,link.post_id,link.last_run])?;
    }
    for item in &snapshot.source_items {
        transaction.execute("INSERT INTO source_item (source_item_id,source_post_id,item_key,position,media_url,canonical_url,media_item_id,state,last_error,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![item.id,item.post_id,item.key,item.position,item.media_url,item.canonical,item.media_item_id,item.state,item.error,item.created,item.modified])?;
    }
    for link in &snapshot.run_source_items {
        transaction.execute(
            "INSERT INTO subscription_run_source_item (run_query_id,source_item_id) VALUES (?1,?2)",
            params![link.run_query_id, link.source_item_id],
        )?;
    }
    for ingest in &snapshot.ingest_jobs {
        transaction.execute("INSERT INTO ingest_job (ingest_job_id,job_key,source_kind,source_path,source_item_id,payload_json,lifecycle,delete_after_ingest,status,attempt_count,available_at,last_error,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)", params![ingest.id,ingest.key,ingest.source_kind,ingest.source_path,ingest.source_item_id,ingest.payload,ingest.lifecycle,ingest.delete_after,ingest.status,ingest.attempts,ingest.available,ingest.error,ingest.created,ingest.modified])?;
    }
    for work in &snapshot.work_items {
        let work_type = normalize_work_type(&work.kind).map_err(conversion_sql_error)?;
        let file_id: Option<i64> = transaction
            .query_row(
                "SELECT file_id FROM media_file WHERE file_hash = ?1",
                [&work.hash],
                |r| r.get(0),
            )
            .optional()?;
        transaction.execute("INSERT INTO work_item (work_id,file_id,file_hash,work_type,status,attempt_count,available_at,last_error,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![work.id,file_id,work.hash,work_type,work.status,work.attempts,work.available,work.error,work.created,work.modified])?;
    }
    for credential in &snapshot.credentials {
        transaction.execute("INSERT INTO credential (site_id,credential_type,display_name,created_at) VALUES (?1,?2,?3,?4)", params![credential.site,credential.kind,credential.display,credential.created])?;
    }
    for health in &snapshot.credential_health {
        transaction.execute("INSERT INTO credential_health (site_id,status,checked_at,last_error) VALUES (?1,?2,?3,?4)", params![health.site,health.status,health.checked,health.error])?;
    }
    for duplicate in &snapshot.duplicates {
        transaction.execute("INSERT INTO duplicate (file_id_a,file_id_b,distance,status,decided_at,winner_file_id) VALUES (?1,?2,?3,?4,?5,?6)", params![duplicate.a,duplicate.b,duplicate.distance,duplicate.status,duplicate.decided,duplicate.winner])?;
    }
    for color in &snapshot.colors {
        transaction.execute(
            "INSERT INTO file_color (color_id,file_id,hex,l,a,b) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                color.id,
                color.file_id,
                color.hex,
                color.l,
                color.a,
                color.b
            ],
        )?;
    }
    for pref in &snapshot.view_preferences {
        transaction.execute(
            "INSERT INTO view_pref (scope,value_json) VALUES (?1,?2)",
            params![pref.scope, pref.value],
        )?;
    }
    for setting in &snapshot.settings {
        let value = serde_json::from_str::<serde_json::Value>(&setting.value)
            .map(|_| setting.value.clone())
            .unwrap_or_else(|_| {
                serde_json::to_string(&setting.value).unwrap_or_else(|_| "null".to_string())
            });
        transaction.execute(
            "INSERT INTO setting (key,value_json) VALUES (?1,?2)",
            params![setting.key, value],
        )?;
    }
    transaction.execute("INSERT INTO media_fts(rowid,name,notes,source_urls) SELECT ma.item_id,ma.name,ma.notes,ma.source_urls_json FROM media_asset ma", [])?;
    Ok(())
}

fn conversion_sql_error(message: String) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    )))
}

fn lifecycle(status: i64) -> Option<&'static str> {
    match status {
        0 => Some("inbox"),
        1 => Some("active"),
        2 => Some("trash"),
        _ => None,
    }
}
fn normalize_work_type(kind: &str) -> Result<&'static str, String> {
    match kind {
        "perceptual_hash" => Ok("perceptual_hash"),
        "thumbnail" => Ok("thumbnail"),
        "dominant_colors" => Ok("dominant_colors"),
        "blob_delete" => Ok("blob_delete"),
        "ai_tag" => Ok("ai_tag"),
        _ => Err(format!(
            "deferred_work_item contains unsupported work type '{kind}'"
        )),
    }
}

fn read_destination_counts(store: &Store) -> Result<ConversionCounts, String> {
    let read =
        |connection: &Connection, table: &str| -> Result<i64, String> { count(connection, table) };
    store.read_result(|connection| {
        let subscription_post_members: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM subscription_source_post spp JOIN source_item si ON si.source_post_id = spp.source_post_id",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        Ok(ConversionCounts {
            media_files: read(connection, "media_file")?,
            media_entities: read(connection, "media_asset")?,
            media_views: read(connection, "media_view")?,
            tags: read(connection, "tag")?,
            entity_tags: read(connection, "media_tag")?,
            tag_aliases: read(connection, "tag_alias")?,
            tag_implications: read(connection, "tag_implication")?,
            folders: read(connection, "folder")?,
            folder_members: read(connection, "folder_item")?,
            smart_folders: read(connection, "smart_folder")?,
            subscriptions: read(connection, "subscription")?,
            subscription_queries: read(connection, "subscription_query")?,
            subscription_runs: read(connection, "subscription_run")?,
            subscription_query_runs: read(connection, "subscription_run_query")?,
            subscription_issues: read(connection, "subscription_issue")?,
            source_posts: read(connection, "source_post")?,
            source_items: read(connection, "source_item")?,
            subscription_post_members,
            ingest_items: read(connection, "ingest_job")?,
            work_items: read(connection, "work_item")?,
            credentials: read(connection, "credential")?,
            credential_health: read(connection, "credential_health")?,
            duplicates: read(connection, "duplicate")?,
            file_colors: read(connection, "file_color")?,
            view_preferences: read(connection, "view_pref")?,
            settings: read(connection, "setting")?,
        })
    })
}

fn compare_counts(source: &ConversionCounts, destination: &ConversionCounts) -> Vec<CountMismatch> {
    let pairs = [
        ("media_files", source.media_files, destination.media_files),
        (
            "media_entities",
            source.media_entities,
            destination.media_entities,
        ),
        ("media_views", source.media_views, destination.media_views),
        ("tags", source.tags, destination.tags),
        ("entity_tags", source.entity_tags, destination.entity_tags),
        ("tag_aliases", source.tag_aliases, destination.tag_aliases),
        (
            "tag_implications",
            source.tag_implications,
            destination.tag_implications,
        ),
        ("folders", source.folders, destination.folders),
        (
            "folder_members",
            source.folder_members,
            destination.folder_members,
        ),
        (
            "smart_folders",
            source.smart_folders,
            destination.smart_folders,
        ),
        (
            "subscriptions",
            source.subscriptions,
            destination.subscriptions,
        ),
        (
            "subscription_queries",
            source.subscription_queries,
            destination.subscription_queries,
        ),
        (
            "subscription_runs",
            source.subscription_runs,
            destination.subscription_runs,
        ),
        (
            "subscription_query_runs",
            source.subscription_query_runs,
            destination.subscription_query_runs,
        ),
        (
            "subscription_issues",
            source.subscription_issues,
            destination.subscription_issues,
        ),
        (
            "source_posts",
            source.source_posts,
            destination.source_posts,
        ),
        (
            "source_items",
            source.source_items,
            destination.source_items,
        ),
        (
            "subscription_post_members",
            source.subscription_post_members,
            destination.subscription_post_members,
        ),
        (
            "ingest_items",
            source.ingest_items,
            destination.ingest_items,
        ),
        ("work_items", source.work_items, destination.work_items),
        ("credentials", source.credentials, destination.credentials),
        (
            "credential_health",
            source.credential_health,
            destination.credential_health,
        ),
        ("duplicates", source.duplicates, destination.duplicates),
        ("file_colors", source.file_colors, destination.file_colors),
        (
            "view_preferences",
            source.view_preferences,
            destination.view_preferences,
        ),
        ("settings", source.settings, destination.settings),
    ];
    pairs
        .into_iter()
        .filter_map(|(name, source, destination)| {
            (source != destination).then_some(CountMismatch {
                name: name.to_string(),
                source,
                destination,
            })
        })
        .collect()
}

fn format_mismatches(mismatches: &[CountMismatch]) -> String {
    mismatches
        .iter()
        .map(|m| format!("{} {} != {}", m.name, m.source, m.destination))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::DATABASE_FILE;

    const LIBRARY_DDL: &str = include_str!("converter_schema117.sql");

    #[test]
    fn dry_run_rejects_non_117_without_mutating_source() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let connection = Connection::open(source.path().join(LEGACY_DATABASE_FILE)).unwrap();
        connection.execute_batch(LIBRARY_DDL).unwrap();
        connection
            .execute("UPDATE schema_version SET version = 116", [])
            .unwrap();
        drop(connection);
        let error = dry_run(source.path(), destination.path()).unwrap_err();
        assert!(error.contains("117"));
        let check = Connection::open(source.path().join(LEGACY_DATABASE_FILE)).unwrap();
        assert_eq!(
            check
                .query_row("SELECT version FROM schema_version", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            116
        );
    }

    #[test]
    fn destination_must_be_empty() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fs::write(
            destination.path().join("keep"),
            b"source must not be replaced",
        )
        .unwrap();
        let connection = Connection::open(source.path().join(LEGACY_DATABASE_FILE)).unwrap();
        connection.execute_batch(LIBRARY_DDL).unwrap();
        drop(connection);
        let error = dry_run(source.path(), destination.path()).unwrap_err();
        assert!(error.contains("absent or empty"));
        assert!(destination.path().join("keep").exists());
    }

    #[test]
    fn count_verification_includes_source_posts_and_items() {
        let source = ConversionCounts {
            source_posts: 1,
            source_items: 2,
            tag_aliases: 3,
            tag_implications: 4,
            ..Default::default()
        };
        let destination = ConversionCounts::default();
        let mismatches = compare_counts(&source, &destination);
        assert!(mismatches.iter().any(|m| m.name == "source_posts"));
        assert!(mismatches.iter().any(|m| m.name == "source_items"));
        assert!(mismatches.iter().any(|m| m.name == "tag_aliases"));
        assert!(mismatches.iter().any(|m| m.name == "tag_implications"));
    }

    #[test]
    fn source_catalog_resolves_query_domain_without_site_id_fallback() {
        assert_eq!(canonical_domain("pixivuser").unwrap(), "pixiv.net");
        let error = canonical_domain("unknown-site").unwrap_err();
        assert!(error.contains("unknown-site"));
    }

    #[test]
    fn view_preferences_use_replacement_keys() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let connection = Connection::open(source.path().join(LEGACY_DATABASE_FILE)).unwrap();
        connection.execute_batch(LIBRARY_DDL).unwrap();
        connection
            .execute(
                "INSERT INTO view_pref (scope,sort_field,sort_dir,layout,tile_size,show_name,show_resolution,show_extension,show_label,thumbnail_fit) VALUES ('system:all','created_at','desc','grid',128,1,1,0,0,'contain')",
                [],
            )
            .unwrap();
        drop(connection);

        execute(source.path(), destination.path()).unwrap();
        let connection = Connection::open(destination.path().join(DATABASE_FILE)).unwrap();
        let value: String = connection
            .query_row(
                "SELECT value_json FROM view_pref WHERE scope = 'system:all'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&value).unwrap();
        assert_eq!(value["sort_order"], "desc");
        assert_eq!(value["view_mode"], "grid");
        assert_eq!(value["target_size"], 128);
        assert!(value.get("sort_dir").is_none());
        assert!(value.get("layout").is_none());
        assert!(value.get("tile_size").is_none());
    }

    #[test]
    fn dry_run_reports_an_empty_schema_117_without_creating_destination() {
        let source = tempfile::tempdir().unwrap();
        let destination_parent = tempfile::tempdir().unwrap();
        let destination = destination_parent.path().join("new-library");
        let connection = Connection::open(source.path().join(LEGACY_DATABASE_FILE)).unwrap();
        connection.execute_batch(LIBRARY_DDL).unwrap();
        drop(connection);

        let report = dry_run(source.path(), &destination).unwrap();
        assert!(report.dry_run);
        assert_eq!(report.source_schema_version, 117);
        assert_eq!(report.source_counts.media_entities, 0);
        assert!(!destination.exists());
    }

    #[test]
    fn execute_converts_an_empty_schema_117_and_validates_replacement_store() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let connection = Connection::open(source.path().join(LEGACY_DATABASE_FILE)).unwrap();
        connection.execute_batch(LIBRARY_DDL).unwrap();
        drop(connection);

        let report = execute(source.path(), destination.path()).unwrap();
        assert!(!report.dry_run);
        assert_eq!(report.destination_counts.unwrap().media_files, 0);
        assert!(destination.path().join(DATABASE_FILE).is_file());
        assert!(source.path().join(LEGACY_DATABASE_FILE).is_file());
    }

    #[test]
    fn failed_execution_restores_empty_destination_and_keeps_source() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let hash = "fa2c8cc4f28176bbeed4b736df569a34c79cd3723e9ec42f9674b4d46ac6b8b8";
        let connection = Connection::open(source.path().join(LEGACY_DATABASE_FILE)).unwrap();
        connection.execute_batch(LIBRARY_DDL).unwrap();
        connection
            .execute(
                "INSERT INTO media_file (file_id,file_hash,mime_type,size_bytes,pixel_width,pixel_height,date_added) VALUES (1,?1,'image/jpeg',4,2,2,'2026-01-01T00:00:00Z')",
                [hash],
            )
            .unwrap();
        drop(connection);

        let blob = source.path().join("blobs/f/fa/2c");
        fs::create_dir_all(&blob).unwrap();
        fs::write(blob.join(format!("{hash}.jpg")), b"bad!").unwrap();

        let error = execute(source.path(), destination.path()).unwrap_err();
        assert!(error.contains("content hash"));
        assert!(destination.path().is_dir());
        assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
        assert!(source.path().join(LEGACY_DATABASE_FILE).is_file());
    }

    #[test]
    fn execute_maps_one_active_media_item_with_tag_and_folder() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let hash = "fa2c8cc4f28176bbeed4b736df569a34c79cd3723e9ec42f9674b4d46ac6b8b8";
        let connection = Connection::open(source.path().join(LEGACY_DATABASE_FILE)).unwrap();
        connection.execute_batch(LIBRARY_DDL).unwrap();
        connection
            .execute(
                "INSERT INTO media_file (file_id,file_hash,mime_type,size_bytes,pixel_width,pixel_height,date_added) VALUES (1,?1,'image/jpeg',4,2,2,'2026-01-01T00:00:00Z')",
                [&hash],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO media_entity (entity_id,entity_hash,file_id,status,name,date_created,date_added,date_modified) VALUES (1,?1,1,1,'one','2026-01-01','2026-01-01','2026-01-01')",
                [&hash],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tag (tag_id,namespace,subtag) VALUES (1,'general','one')",
                [],
            )
            .unwrap();
        connection
            .execute("INSERT INTO entity_tag (entity_id,tag_id,provenance_mask,source) VALUES (1,1,0,'local')", [])
            .unwrap();
        connection
            .execute("INSERT INTO folder (folder_id,name,date_added,date_modified) VALUES (1,'folder','2026-01-01','2026-01-01')", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO folder_member (folder_id,entity_id) VALUES (1,1)",
                [],
            )
            .unwrap();
        drop(connection);

        let blob = source.path().join("blobs/f/fa/2c");
        fs::create_dir_all(&blob).unwrap();
        fs::write(blob.join(format!("{hash}.jpg")), b"blob").unwrap();

        let report = execute(source.path(), destination.path()).unwrap();
        assert_eq!(report.source_counts.media_entities, 1);
        assert_eq!(report.destination_counts.unwrap().media_entities, 1);
        let destination_connection =
            Connection::open(destination.path().join(DATABASE_FILE)).unwrap();
        assert_eq!(
            destination_connection
                .query_row(
                    "SELECT lifecycle FROM library_root WHERE item_id = 1",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "active"
        );
        assert_eq!(
            destination_connection
                .query_row(
                    "SELECT COUNT(*) FROM media_tag WHERE media_item_id = 1",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert!(source.path().join(LEGACY_DATABASE_FILE).is_file());
    }
}
