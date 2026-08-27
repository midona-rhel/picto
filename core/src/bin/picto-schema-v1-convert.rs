//! One-time pre-release library conversion for the million-item cutover.
//!
//! This binary is intentionally separate from the running application. The
//! default operation is read-only validation. Conversion copies the complete
//! library directory before creating a new database, and activation is a
//! separate, explicit operation that atomically replaces only the database
//! file. No command implicitly opens, mutates, or deletes a source library.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use picto_core::canonical_bitmap::{
    intern_key, load_bitmap, load_order, rating_key, replace_bitmap, replace_ordered_membership,
    BitmapDomain, LIFECYCLE_ACTIVE_KEY, LIFECYCLE_INBOX_KEY, LIFECYCLE_TRASH_KEY,
};
use picto_core::store::schema;
use roaring::RoaringBitmap;
use rusqlite::{Connection, OpenFlags, MAIN_DB};
use sha2::{Digest, Sha256};

const DATABASE_FILE: &str = "library.sqlite";
const MANIFEST_FILE: &str = ".picto-schema-v1-conversion";
const COPY_BUFFER_SIZE: usize = 1024 * 1024;
const LEGACY_DEVELOPMENT_SCHEMA_VERSION: i64 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Validate,
    Convert,
    Activate,
    Cleanup,
}

#[derive(Debug)]
struct Options {
    command: Command,
    source: Option<PathBuf>,
    destination: Option<PathBuf>,
    backup: Option<PathBuf>,
    dry_run: bool,
    destructive: bool,
    yes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Counts {
    library_items: i64,
    roots: i64,
    media_assets: i64,
    media_files: i64,
    collection_members: i64,
    tags: i64,
    folders: i64,
    subscriptions: i64,
    source_posts: i64,
    source_items: i64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("conversion failed: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_options(env::args().skip(1).collect())?;
    match options.command {
        Command::Validate => {
            let source = required_root(options.source, "--source")?;
            let report = validate_library(&source)?;
            println!(
                "validated {}: schema={} revision={} items={} roots={} media={} files={} members={}",
                source.display(),
                report.schema_version,
                report.revision,
                report.counts.library_items,
                report.counts.roots,
                report.counts.media_assets,
                report.counts.media_files,
                report.counts.collection_members,
            );
        }
        Command::Convert => {
            let source = required_root(options.source, "--source")?;
            let destination = required_root(options.destination, "--destination")?;
            let backup = options
                .backup
                .unwrap_or_else(|| timestamped_sibling(&source, "pre-cutover"));
            if options.dry_run {
                println!("dry-run: validate source {}", source.display());
                let report = validate_library(&source)?;
                println!(
                    "dry-run: create backup {} and destination {} for {} items",
                    backup.display(),
                    destination.display(),
                    report.counts.library_items
                );
            } else {
                let report = convert_library(&source, &destination, &backup)?;
                println!(
                    "converted {} -> {} (backup {}) schema=1 revision=1 items={}",
                    source.display(),
                    destination.display(),
                    backup.display(),
                    report.counts.library_items
                );
            }
        }
        Command::Activate => {
            let source = required_root(options.source, "--source")?;
            let destination = required_root(options.destination, "--destination")?;
            let backup = required_root(options.backup, "--backup")?;
            if !options.yes {
                return Err("activation mutates the source library; pass --yes explicitly".into());
            }
            activate_library(&source, &destination, &backup)?;
            println!(
                "activated {} from {}; previous database remains in {}",
                source.display(),
                destination.display(),
                backup.display()
            );
        }
        Command::Cleanup => {
            if !options.destructive || !options.yes {
                return Err(
                    "cleanup is destructive; pass both --destructive and --yes explicitly".into(),
                );
            }
            let mut removed = 0;
            for path in [options.backup, options.destination].into_iter().flatten() {
                remove_conversion_directory(&path)?;
                println!("removed {}", path.display());
                removed += 1;
            }
            if removed == 0 {
                return Err("cleanup requires --backup and/or --destination".into());
            }
        }
    }
    Ok(())
}

fn parse_options(args: Vec<String>) -> Result<Options, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        process::exit(0);
    }

    let mut command = Command::Validate;
    let mut source = None;
    let mut destination = None;
    let mut backup = None;
    let mut dry_run = false;
    let mut destructive = false;
    let mut yes = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "validate" => command = Command::Validate,
            "convert" => command = Command::Convert,
            "activate" => command = Command::Activate,
            "cleanup" => command = Command::Cleanup,
            "--dry-run" => dry_run = true,
            "--destructive" => destructive = true,
            "--yes" => yes = true,
            "--source" | "--destination" | "--backup" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{arg} requires a path"))?;
                let target = match arg.as_str() {
                    "--source" => &mut source,
                    "--destination" => &mut destination,
                    _ => &mut backup,
                };
                *target = Some(PathBuf::from(value));
            }
            other => return Err(format!("unknown argument '{other}' (try --help)")),
        }
        index += 1;
    }

    if command == Command::Validate && (destination.is_some() || backup.is_some()) {
        return Err("validate accepts only --source".into());
    }
    if command != Command::Convert && dry_run {
        return Err("--dry-run is only valid with convert".into());
    }
    Ok(Options {
        command,
        source,
        destination,
        backup,
        dry_run,
        destructive,
        yes,
    })
}

fn print_usage() {
    println!(
        "Picto schema-v1 converter\n\n\
Default command is read-only validation. Paths may be a library directory or library.sqlite.\n\n\
  [validate] --source PATH\n\
  convert --source PATH --destination PATH [--backup PATH] [--dry-run]\n\
  activate --source PATH --destination PATH --backup PATH --yes\n\
  cleanup --backup PATH [--destination PATH] --destructive --yes\n\n\
Conversion never activates the destination. Stop Picto before explicit activation."
    );
}

fn required_root(path: Option<PathBuf>, flag: &str) -> Result<PathBuf, String> {
    let path = path.ok_or_else(|| format!("{flag} is required"))?;
    library_root(&path)
}

fn library_root(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("cannot resolve {}: {error}", path.display()))?;
    if path.is_dir() {
        return Ok(path);
    }
    if path.file_name() == Some(OsStr::new(DATABASE_FILE)) {
        return path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("{} has no library directory", path.display()));
    }
    Err(format!(
        "{} is neither a library directory nor {DATABASE_FILE}",
        path.display()
    ))
}

fn database_path(root: &Path) -> PathBuf {
    root.join(DATABASE_FILE)
}

fn timestamp() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{millis}")
}

fn timestamped_sibling(source: &Path, label: &str) -> PathBuf {
    let name = source
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("library");
    source.with_file_name(format!("{name}.{label}-{}", timestamp()))
}

#[derive(Debug)]
struct ValidationReport {
    schema_version: i64,
    revision: i64,
    counts: Counts,
}

fn open_read_only(root: &Path) -> Result<Connection, String> {
    let path = database_path(root);
    if !path.is_file() {
        return Err(format!("missing database {}", path.display()));
    }
    let connection = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|error| format!("cannot open {} read-only: {error}", path.display()))?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|error| format!("cannot enable query-only mode: {error}"))?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|error| format!("cannot enable foreign-key checks: {error}"))?;
    Ok(connection)
}

fn validate_library(root: &Path) -> Result<ValidationReport, String> {
    let connection = open_read_only(root)?;
    validate_integrity_in_scratch(&connection)?;
    let schema_version: i64 = connection
        .query_row(
            "SELECT schema_version FROM library_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("missing library schema marker: {error}"))?;
    if schema_version != LEGACY_DEVELOPMENT_SCHEMA_VERSION
        && schema_version != schema::CURRENT_SCHEMA_VERSION
    {
        return Err(format!(
            "source schema {schema_version} is not the one-time development source schema \
             {LEGACY_DEVELOPMENT_SCHEMA_VERSION} or converted schema {}",
            schema::CURRENT_SCHEMA_VERSION,
        ));
    }
    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM library_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("missing library revision: {error}"))?;
    let counts = read_counts(&connection)?;
    validate_invariants(&connection, &counts)?;
    Ok(ValidationReport {
        schema_version,
        revision,
        counts,
    })
}

fn validate_integrity_in_scratch(source: &Connection) -> Result<(), String> {
    let directory =
        tempfile::tempdir().map_err(|error| format!("cannot create check directory: {error}"))?;
    let scratch_path = directory.path().join(DATABASE_FILE);
    source
        .backup(MAIN_DB, &scratch_path, None)
        .map_err(|error| format!("cannot create integrity-check snapshot: {error}"))?;
    let scratch = Connection::open(&scratch_path)
        .map_err(|error| format!("cannot open integrity-check snapshot: {error}"))?;
    let quick: String = scratch
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|error| format!("quick_check failed: {error}"))?;
    if quick != "ok" {
        return Err(format!("quick_check reported {quick}"));
    }
    scratch
        .pragma_update(None, "foreign_keys", true)
        .map_err(|error| format!("cannot enable foreign-key checks: {error}"))?;
    let mut foreign = scratch
        .prepare("PRAGMA foreign_key_check")
        .map_err(|error| format!("foreign_key_check failed: {error}"))?;
    let mut rows = foreign
        .query([])
        .map_err(|error| format!("foreign_key_check failed: {error}"))?;
    if rows
        .next()
        .map_err(|error| format!("foreign_key_check failed: {error}"))?
        .is_some()
    {
        return Err("foreign_key_check reported a violation".into());
    }
    Ok(())
}

fn read_counts(connection: &Connection) -> Result<Counts, String> {
    fn count(connection: &Connection, table: &str) -> Result<i64, String> {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|error| format!("cannot count {table}: {error}"))
    }
    Ok(Counts {
        library_items: count(connection, "library_item")?,
        roots: count(connection, "library_root")?,
        media_assets: count(connection, "media_asset")?,
        media_files: count(connection, "media_file")?,
        collection_members: count(connection, "collection_member")?,
        tags: count(connection, "tag")?,
        folders: count(connection, "folder")?,
        subscriptions: count(connection, "subscription")?,
        source_posts: count(connection, "source_post")?,
        source_items: count(connection, "source_item")?,
    })
}

fn validate_invariants(connection: &Connection, counts: &Counts) -> Result<(), String> {
    if counts.roots > counts.library_items {
        return Err("library_root contains more rows than library_item".into());
    }
    if counts.media_assets > counts.library_items {
        return Err("media_asset contains more rows than library_item".into());
    }
    let orphan_members: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM collection_member cm
             LEFT JOIN library_item c ON c.item_id = cm.collection_id
             LEFT JOIN library_item m ON m.item_id = cm.media_item_id
             WHERE c.kind IS NULL OR c.kind <> 'collection'
                OR m.kind IS NULL OR m.kind <> 'media'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("collection membership invariant failed: {error}"))?;
    if orphan_members != 0 {
        return Err(format!(
            "{orphan_members} collection members have invalid owners"
        ));
    }
    let rooted_members: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM collection_member cm
             JOIN library_root lr ON lr.item_id = cm.media_item_id",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("root membership invariant failed: {error}"))?;
    if rooted_members != 0 {
        return Err(format!(
            "{rooted_members} attached collection members still have library roots"
        ));
    }
    let bad_order: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM (
                SELECT collection_id, position_rank
                FROM collection_member
                GROUP BY collection_id, position_rank
                HAVING position_rank < 0 OR COUNT(*) > 1
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("collection order invariant failed: {error}"))?;
    if bad_order != 0 {
        return Err(format!("{bad_order} collection order conflicts found"));
    }
    let bad_source_order: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM (
                SELECT source_post_id, position
                FROM source_item
                GROUP BY source_post_id, position
                HAVING position < 0 OR COUNT(*) > 1
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("source order invariant failed: {error}"))?;
    if bad_source_order != 0 {
        return Err(format!("{bad_source_order} source order conflicts found"));
    }
    let schema_version: i64 = connection
        .query_row(
            "SELECT schema_version FROM library_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("cannot inspect schema generation: {error}"))?;
    if schema_version == 1 {
        validate_canonical_v1(connection, counts)?;
    }
    Ok(())
}

fn schema_object_exists(
    connection: &Connection,
    object_type: &str,
    name: &str,
) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2
             )",
            [object_type, name],
            |row| row.get(0),
        )
        .map_err(|error| format!("cannot inspect schema object {name}: {error}"))
}

fn validate_canonical_v1(connection: &Connection, counts: &Counts) -> Result<(), String> {
    for removed in [
        "media_tag",
        "root_tag_count",
        "tag_search_fts",
        "folder_search_fts",
        "search_dirty_tag",
        "search_dirty_folder",
    ] {
        if schema_object_exists(connection, "table", removed)? {
            return Err(format!("canonical schema retained legacy table {removed}"));
        }
    }
    for required in [
        "root_metadata",
        "canonical_bitmap_key",
        "canonical_bitmap_key_allocator",
        "canonical_bitmap",
        "canonical_order",
        "root_tag",
        "root_summary",
        "tag_summary",
        "smart_folder_dependency",
        "smart_folder_generation",
        "smart_folder_membership",
        "projection_checkpoint",
        "root_name_fts",
        "root_notes_fts",
        "source_text_fts",
    ] {
        if !schema_object_exists(connection, "table", required)? {
            return Err(format!("canonical schema is missing {required}"));
        }
    }
    for removed_column in ["notes", "rating", "source_urls_json"] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM pragma_table_info('media_asset') WHERE name = ?1
                 )",
                [removed_column],
                |row| row.get(0),
            )
            .map_err(|error| format!("cannot inspect media_asset: {error}"))?;
        if exists {
            return Err(format!(
                "canonical media_asset retained organization column {removed_column}"
            ));
        }
    }
    let retained_item_label: bool = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('library_item') WHERE name = 'label'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("cannot inspect library_item: {error}"))?;
    if retained_item_label {
        return Err("canonical library_item retained root-owned label column".into());
    }

    for (table, expected) in [
        ("root_metadata", counts.roots),
        ("root_summary", counts.roots),
        ("tag_summary", counts.tags),
    ] {
        let actual: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|error| format!("cannot validate {table}: {error}"))?;
        if actual != expected {
            return Err(format!(
                "canonical {table} count is {actual}, expected {expected}"
            ));
        }
    }

    let inconsistent_smart_counts: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM smart_folder_generation generation
             WHERE generation.member_count <> (
                 SELECT COUNT(*) FROM smart_folder_membership membership
                 WHERE membership.generation_id = generation.generation_id
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("cannot validate smart-folder generations: {error}"))?;
    if inconsistent_smart_counts != 0 {
        return Err(format!(
            "{inconsistent_smart_counts} smart-folder generations have incorrect counts"
        ));
    }

    let bad_tag_summaries: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM tag_summary summary
             WHERE summary.visible_root_count <> (
                 SELECT COUNT(*)
                 FROM root_tag relation
                 JOIN root_summary root ON root.root_item_id = relation.root_item_id
                 WHERE relation.tag_id = summary.tag_id AND root.lifecycle = 'active'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("cannot validate tag summaries: {error}"))?;
    if bad_tag_summaries != 0 {
        return Err(format!(
            "{bad_tag_summaries} tag summaries are inconsistent"
        ));
    }
    validate_canonical_memberships(connection)?;
    Ok(())
}

fn validate_canonical_memberships(connection: &Connection) -> Result<(), String> {
    let expected_roots = query_bitmap(connection, "SELECT item_id FROM library_root")?;
    let active = load_bitmap(connection, BitmapDomain::Lifecycle, LIFECYCLE_ACTIVE_KEY)
        .map_err(|error| format!("cannot decode active lifecycle bitmap: {error}"))?;
    let inbox = load_bitmap(connection, BitmapDomain::Lifecycle, LIFECYCLE_INBOX_KEY)
        .map_err(|error| format!("cannot decode Inbox lifecycle bitmap: {error}"))?;
    let trash = load_bitmap(connection, BitmapDomain::Lifecycle, LIFECYCLE_TRASH_KEY)
        .map_err(|error| format!("cannot decode Trash lifecycle bitmap: {error}"))?;
    if !(&active & &inbox).is_empty()
        || !(&active & &trash).is_empty()
        || !(&inbox & &trash).is_empty()
        || (&active | &inbox | &trash) != expected_roots
    {
        return Err("canonical lifecycle bitmaps are not an exact root partition".into());
    }

    let tag_ids = connection
        .prepare("SELECT tag_id FROM tag ORDER BY tag_id")
        .map_err(|error| format!("cannot read tags: {error}"))?
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("cannot read tags: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("cannot read tags: {error}"))?;
    for tag_id in tag_ids {
        let expected = query_bitmap_with_key(
            connection,
            "SELECT root_item_id FROM root_tag WHERE tag_id = ?1",
            tag_id,
        )?;
        let actual = load_bitmap(connection, BitmapDomain::Tag, tag_id)
            .map_err(|error| format!("cannot decode tag {tag_id}: {error}"))?;
        if actual != expected {
            return Err(format!("canonical tag bitmap {tag_id} is inconsistent"));
        }
    }

    let groups = connection
        .prepare("SELECT item_id FROM library_item WHERE kind = 'collection' ORDER BY item_id")
        .map_err(|error| format!("cannot read groups: {error}"))?
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("cannot read groups: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("cannot read groups: {error}"))?;
    for group_id in groups {
        let members = load_bitmap(connection, BitmapDomain::GroupMember, group_id)
            .map_err(|error| format!("cannot decode group {group_id}: {error}"))?;
        let order = load_order(connection, "group", group_id)
            .map_err(|error| format!("cannot decode group order {group_id}: {error}"))?
            .ok_or_else(|| format!("group {group_id} has no canonical order"))?;
        if order.iter().copied().collect::<RoaringBitmap>() != members
            || order.len() != members.len() as usize
        {
            return Err(format!("group {group_id} membership and order diverge"));
        }
    }
    Ok(())
}

fn query_bitmap(connection: &Connection, sql: &str) -> Result<RoaringBitmap, String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("cannot query bitmap validation set: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("cannot query bitmap validation set: {error}"))?;
    let mut result = RoaringBitmap::new();
    for row in rows {
        result.insert(bitmap_id(row.map_err(|error| {
            format!("cannot query bitmap validation set: {error}")
        })?)?);
    }
    Ok(result)
}

fn query_bitmap_with_key(
    connection: &Connection,
    sql: &str,
    key: i64,
) -> Result<RoaringBitmap, String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("cannot query bitmap validation set: {error}"))?;
    let rows = statement
        .query_map([key], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("cannot query bitmap validation set: {error}"))?;
    let mut result = RoaringBitmap::new();
    for row in rows {
        result.insert(bitmap_id(row.map_err(|error| {
            format!("cannot query bitmap validation set: {error}")
        })?)?);
    }
    Ok(result)
}

fn convert_library(
    source: &Path,
    destination: &Path,
    backup: &Path,
) -> Result<ValidationReport, String> {
    let source = library_root(source)?;
    let destination = output_root(destination)?;
    let backup = output_root(backup)?;
    reject_overlapping_paths(&source, &destination, &backup)?;
    let source_report = validate_library(&source)?;
    if source_report.schema_version != LEGACY_DEVELOPMENT_SCHEMA_VERSION {
        return Err(format!(
            "conversion expects development schema {}, received {}",
            LEGACY_DEVELOPMENT_SCHEMA_VERSION, source_report.schema_version
        ));
    }
    copy_directory(&source, &backup)?;
    replace_database_with_snapshot(&source, &backup)?;
    write_manifest(&backup, &source, &source_report.counts)?;
    copy_directory(&source, &destination)?;

    let destination_db = database_path(&destination);
    let temporary_db = destination.join(format!(".library.sqlite.v1-{}", timestamp()));
    if temporary_db.exists() {
        return Err(format!(
            "temporary destination already exists: {}",
            temporary_db.display()
        ));
    }
    let mut destination_connection = Connection::open(&temporary_db)
        .map_err(|error| format!("cannot create destination database: {error}"))?;
    schema::create_canonical_v1(&mut destination_connection)
        .map_err(|error| format!("cannot create schema-v1 destination: {error}"))?;
    copy_canonical_rows(&mut destination_connection, &source, &destination)?;
    rebuild_derived_state(&mut destination_connection)?;
    drop(destination_connection);
    remove_sqlite_sidecars(&destination)?;
    replace_file(&temporary_db, &destination_db)?;

    let destination_report = validate_library(&destination)?;
    if destination_report.schema_version != 1 || destination_report.revision != 1 {
        return Err(format!(
            "destination markers are schema={} revision={}, expected 1/1",
            destination_report.schema_version, destination_report.revision
        ));
    }
    if destination_report.counts != source_report.counts {
        return Err(format!(
            "canonical row counts changed during conversion: source={:?}, destination={:?}",
            source_report.counts, destination_report.counts
        ));
    }
    write_manifest(&destination, &source, &destination_report.counts)?;
    Ok(destination_report)
}

fn output_root(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|error| format!("cannot resolve output directory: {error}"))?
            .join(path)
    };
    if absolute.exists() {
        return Err(format!(
            "refusing to overwrite existing output {}; choose a new path",
            absolute.display()
        ));
    }
    let parent = absolute
        .parent()
        .ok_or_else(|| format!("output path has no parent: {}", absolute.display()))?
        .canonicalize()
        .map_err(|error| format!("cannot resolve output parent: {error}"))?;
    Ok(parent.join(
        absolute
            .file_name()
            .ok_or_else(|| format!("output path has no name: {}", absolute.display()))?,
    ))
}

fn reject_overlapping_paths(
    source: &Path,
    destination: &Path,
    backup: &Path,
) -> Result<(), String> {
    for (name, path) in [("destination", destination), ("backup", backup)] {
        if path.starts_with(source) || source.starts_with(path) {
            return Err(format!(
                "{name} {} overlaps source {}; refusing unsafe copy",
                path.display(),
                source.display()
            ));
        }
    }
    if destination.starts_with(backup) || backup.starts_with(destination) {
        return Err("destination and backup paths overlap".into());
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err(format!("output already exists: {}", destination.display()));
    }
    fs::create_dir_all(destination)
        .map_err(|error| format!("cannot create {}: {error}", destination.display()))?;
    copy_directory_contents(source, destination)
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot read directory entry: {error}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| format!("cannot inspect {}: {error}", source_path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing to copy symlink {}",
                source_path.display()
            ));
        }
        if metadata.is_dir() {
            fs::create_dir(&destination_path).map_err(|error| {
                format!("cannot create {}: {error}", destination_path.display())
            })?;
            copy_directory_contents(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &destination_path)?;
        } else {
            return Err(format!(
                "unsupported filesystem entry {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    let mut input =
        File::open(source).map_err(|error| format!("cannot open {}: {error}", source.display()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("cannot create {}: {error}", destination.display()))?;
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("cannot write {}: {error}", destination.display()))?;
    }
    output
        .sync_all()
        .map_err(|error| format!("cannot flush {}: {error}", destination.display()))
}

fn replace_database_with_snapshot(source_root: &Path, copy_root: &Path) -> Result<(), String> {
    let source = open_read_only(source_root)?;
    let snapshot = copy_root.join(format!(".library.sqlite.snapshot-{}", timestamp()));
    source
        .backup(MAIN_DB, &snapshot, None)
        .map_err(|error| format!("cannot create consistent backup snapshot: {error}"))?;
    remove_sqlite_sidecars(copy_root)?;
    replace_file(&snapshot, &database_path(copy_root))?;
    sync_directory(copy_root)
}

fn remove_sqlite_sidecars(root: &Path) -> Result<(), String> {
    for suffix in ["-wal", "-shm"] {
        let path = root.join(format!("{DATABASE_FILE}{suffix}"));
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("cannot remove stale {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn copy_canonical_rows(
    destination: &mut Connection,
    source_root: &Path,
    destination_root: &Path,
) -> Result<(), String> {
    let source_db = database_path(source_root);
    destination
        .execute(
            "ATTACH DATABASE ?1 AS source",
            [source_db.to_string_lossy().as_ref()],
        )
        .map_err(|error| format!("cannot attach source read-only database: {error}"))?;
    let transaction = destination
        .transaction()
        .map_err(|error| format!("cannot start conversion transaction: {error}"))?;

    // `library_item.cover_media_item_id` and `media_asset.item_id` form an
    // intentional cycle. Defer, rather than disable, foreign-key enforcement
    // so the set-wise copy can insert both sides and SQLite still rejects any
    // unresolved reference at commit.
    transaction
        .execute_batch("PRAGMA defer_foreign_keys = ON;")
        .map_err(|error| format!("cannot defer conversion foreign keys: {error}"))?;

    // The schema constructor creates a cloud identity and pHash setting for
    // an empty library. Both are replaced below with the new generation's
    // identity and the source's user-facing settings.
    transaction
        .execute_batch("DELETE FROM cloud_state; DELETE FROM setting;")
        .map_err(|error| format!("cannot clear destination defaults: {error}"))?;

    for (table, columns) in COPY_TABLES {
        let sql =
            format!("INSERT INTO main.{table} ({columns}) SELECT {columns} FROM source.{table}");
        transaction
            .execute(&sql, [])
            .map_err(|error| format!("cannot copy {table}: {error}"))?;
    }

    // Root organization is reconstructed once, set-wise. Collections use the
    // cover (or first ordered member) for scalar metadata and union member URLs.
    transaction
        .execute(
            "WITH root_media(root_item_id, media_item_id) AS (
                 SELECT lr.item_id, lr.item_id
                 FROM source.library_root lr
                 JOIN source.library_item li
                   ON li.item_id = lr.item_id AND li.kind = 'media'
                 UNION ALL
                 SELECT lr.item_id, cm.media_item_id
                 FROM source.library_root lr
                 JOIN source.library_item li
                   ON li.item_id = lr.item_id AND li.kind = 'collection'
                 JOIN source.collection_member cm ON cm.collection_id = lr.item_id
             ), cover_media(root_item_id, media_item_id) AS (
                 SELECT lr.item_id,
                        COALESCE(li.cover_media_item_id, (
                            SELECT cm.media_item_id
                            FROM source.collection_member cm
                            WHERE cm.collection_id = lr.item_id
                            ORDER BY cm.position_rank, cm.media_item_id
                            LIMIT 1
                        ), lr.item_id)
                 FROM source.library_root lr
                 JOIN source.library_item li ON li.item_id = lr.item_id
             ), union_urls(root_item_id, urls_json) AS (
                 SELECT roots.root_item_id, COALESCE(json_group_array(roots.url), '[]')
                 FROM (
                     SELECT DISTINCT rm.root_item_id, CAST(url.value AS TEXT) AS url
                     FROM root_media rm
                     JOIN source.media_asset ma ON ma.item_id = rm.media_item_id
                     JOIN json_each(
                         CASE
                             WHEN json_valid(COALESCE(ma.source_urls_json, '[]'))
                             THEN COALESCE(ma.source_urls_json, '[]')
                             ELSE '[]'
                         END
                     ) url
                     WHERE TRIM(CAST(url.value AS TEXT)) <> ''
                     ORDER BY rm.root_item_id, CAST(url.value AS TEXT)
                 ) roots
                 GROUP BY roots.root_item_id
             )
             INSERT INTO main.root_metadata (
                 root_item_id, name, rating, notes, source_urls_json, updated_at
             )
             SELECT lr.item_id,
                    COALESCE(NULLIF(TRIM(cover.name), ''), NULLIF(TRIM(li.label), '')),
                    cover.rating,
                    cover.notes,
                    COALESCE(union_urls.urls_json, '[]'),
                    COALESCE(cover.updated_at, li.updated_at)
             FROM source.library_root lr
             JOIN source.library_item li ON li.item_id = lr.item_id
             LEFT JOIN cover_media selected ON selected.root_item_id = lr.item_id
             LEFT JOIN source.media_asset cover ON cover.item_id = selected.media_item_id
             LEFT JOIN union_urls ON union_urls.root_item_id = lr.item_id",
            [],
        )
        .map_err(|error| format!("cannot convert root metadata: {error}"))?;

    // Fold every legacy member assignment into one effective (root, tag) row.
    // The recursive fold preserves the bitwise provenance union without a
    // custom SQLite aggregate or per-row Rust work.
    transaction
        .execute(
            "WITH RECURSIVE
             root_media(root_item_id, media_item_id) AS (
                 SELECT lr.item_id, lr.item_id
                 FROM source.library_root lr
                 JOIN source.library_item li
                   ON li.item_id = lr.item_id AND li.kind = 'media'
                 UNION ALL
                 SELECT lr.item_id, cm.media_item_id
                 FROM source.library_root lr
                 JOIN source.library_item li
                   ON li.item_id = lr.item_id AND li.kind = 'collection'
                 JOIN source.collection_member cm ON cm.collection_id = lr.item_id
             ), numbered AS (
                 SELECT rm.root_item_id, mt.tag_id, mt.media_item_id,
                        COALESCE(mt.provenance_mask, 0) AS provenance_mask,
                        ROW_NUMBER() OVER (
                            PARTITION BY rm.root_item_id, mt.tag_id
                            ORDER BY mt.media_item_id, mt.source
                        ) AS sequence,
                        COUNT(*) OVER (
                            PARTITION BY rm.root_item_id, mt.tag_id
                        ) AS assignment_count
                 FROM root_media rm
                 JOIN source.media_tag mt ON mt.media_item_id = rm.media_item_id
             ), folded(
                 root_item_id, tag_id, sequence, assignment_count, provenance_mask
             ) AS (
                 SELECT root_item_id, tag_id, 1, assignment_count, provenance_mask
                 FROM numbered WHERE sequence = 1
                 UNION ALL
                 SELECT folded.root_item_id, folded.tag_id, numbered.sequence,
                        folded.assignment_count,
                        folded.provenance_mask | numbered.provenance_mask
                 FROM folded
                 JOIN numbered
                   ON numbered.root_item_id = folded.root_item_id
                  AND numbered.tag_id = folded.tag_id
                  AND numbered.sequence = folded.sequence + 1
             )
             INSERT INTO main.root_tag (
                 root_item_id, tag_id, direct_assignment_count,
                 provenance_mask
             )
             SELECT root_item_id, tag_id, assignment_count, provenance_mask
             FROM folded WHERE sequence = assignment_count",
            [],
        )
        .map_err(|error| format!("cannot convert root tags: {error}"))?;

    transaction
        .execute(
            "INSERT INTO main.folder_item(folder_id, item_id, position_rank)
             SELECT mapped.folder_id, mapped.root_item_id, MIN(mapped.position_rank)
             FROM (
                 SELECT fi.folder_id,
                        COALESCE(cm.collection_id, fi.item_id) AS root_item_id,
                        fi.position_rank
                 FROM source.folder_item fi
                 LEFT JOIN source.collection_member cm
                   ON cm.media_item_id = fi.item_id
             ) mapped
             JOIN main.library_root lr ON lr.item_id = mapped.root_item_id
             GROUP BY mapped.folder_id, mapped.root_item_id",
            [],
        )
        .map_err(|error| format!("cannot convert root folder ownership: {error}"))?;

    transaction
        .execute(
            "INSERT INTO main.smart_folder_dependency (
                 smart_folder_id, dependency_kind, dependency_key
             )
             SELECT DISTINCT sf.smart_folder_id, 'field',
                    LOWER(CAST(json_extract(rule.value, '$.field') AS TEXT))
             FROM main.smart_folder sf,
                  json_each(sf.predicate_json, '$.groups') rule_group,
                  json_each(rule_group.value, '$.rules') rule
             WHERE json_extract(rule.value, '$.field') IS NOT NULL
             UNION
             SELECT DISTINCT sf.smart_folder_id, 'tag', LOWER(TRIM(CAST(value.value AS TEXT)))
             FROM main.smart_folder sf,
                  json_each(sf.predicate_json, '$.groups') rule_group,
                  json_each(rule_group.value, '$.rules') rule,
                  json_each(rule.value, '$.values') value
             WHERE json_extract(rule.value, '$.field') = 'tags'",
            [],
        )
        .map_err(|error| format!("cannot compile smart-folder dependencies: {error}"))?;

    transaction
        .execute(
            "INSERT INTO main.smart_folder_generation (
                 generation_id, smart_folder_id, database_revision, state,
                 member_count, created_at, activated_at
             )
             SELECT smart_folder_id, smart_folder_id, 1, 'active', 0,
                    updated_at, updated_at
             FROM main.smart_folder",
            [],
        )
        .map_err(|error| format!("cannot create smart-folder generations: {error}"))?;
    let source_has_smart_membership: bool = transaction
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM source.sqlite_master
                 WHERE type = 'table' AND name = 'smart_folder_root'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("cannot inspect source smart-folder state: {error}"))?;
    let smart_folder_count: i64 = transaction
        .query_row("SELECT COUNT(*) FROM main.smart_folder", [], |row| {
            row.get(0)
        })
        .map_err(|error| format!("cannot count smart folders: {error}"))?;
    if smart_folder_count > 0 && !source_has_smart_membership {
        return Err(
            "source has smart folders but no settled smart-folder projection; open and settle the source library before conversion"
                .into(),
        );
    }
    if source_has_smart_membership && smart_folder_count > 0 {
        for dirty_table in ["smart_projection_dirty_root", "smart_projection_dirty_all"] {
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM source.sqlite_master
                         WHERE type = 'table' AND name = ?1
                     )",
                    [dirty_table],
                    |row| row.get(0),
                )
                .map_err(|error| format!("cannot inspect source {dirty_table}: {error}"))?;
            if exists {
                let dirty_count: i64 = transaction
                    .query_row(
                        &format!("SELECT COUNT(*) FROM source.{dirty_table}"),
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| format!("cannot inspect source {dirty_table}: {error}"))?;
                if dirty_count != 0 {
                    return Err(format!(
                        "source smart-folder projection is not settled ({dirty_count} rows in {dirty_table})"
                    ));
                }
            }
        }
        transaction
            .execute(
                "INSERT INTO main.smart_folder_membership(generation_id, root_item_id)
                 SELECT generation.smart_folder_id, source_root.root_item_id
                 FROM source.smart_folder_root source_root
                 JOIN main.smart_folder_generation generation
                   ON generation.smart_folder_id = source_root.smart_folder_id
                  AND generation.state = 'active'
                 JOIN main.library_root root
                   ON root.item_id = source_root.root_item_id",
                [],
            )
            .map_err(|error| format!("cannot copy smart-folder membership: {error}"))?;
        transaction
            .execute(
                "UPDATE main.smart_folder_generation
                 SET member_count = (
                     SELECT COUNT(*)
                     FROM main.smart_folder_membership membership
                     WHERE membership.generation_id = smart_folder_generation.generation_id
                 )",
                [],
            )
            .map_err(|error| format!("cannot count smart-folder membership: {error}"))?;
    }

    transaction
        .execute(
            "INSERT INTO cloud_state (
                singleton, library_id, device_id, provider, account_label,
                remote_root, paused, state, phase, blocking, completed_units,
                total_units, message, pending_blobs, missing_blobs,
                schema_generation, retention_json
             )
             SELECT 1, lower(hex(randomblob(16))), lower(hex(randomblob(16))),
                    provider, account_label, remote_root, 1, 'disabled', 'idle',
                    0, 0, NULL, '', 0, 0, 1, retention_json
             FROM source.cloud_state
             WHERE singleton = 1",
            [],
        )
        .map_err(|error| format!("cannot create rotated cloud identity: {error}"))?;
    transaction
        .execute(
            "INSERT INTO cloud_state (singleton, library_id, device_id, paused, state, phase)
             SELECT 1, lower(hex(randomblob(16))), lower(hex(randomblob(16))), 1, 'disabled', 'idle'
             WHERE NOT EXISTS (SELECT 1 FROM main.cloud_state WHERE singleton = 1)",
            [],
        )
        .map_err(|error| format!("cannot create cloud state: {error}"))?;
    transaction
        .execute(
            "INSERT INTO cloud_blob_state (
                file_hash, state, priority, remote_present, remote_extension,
                last_error, uploaded_at, updated_at
             )
             SELECT file_hash, state, priority, 0, remote_extension,
                    last_error, NULL, updated_at
             FROM source.cloud_blob_state",
            [],
        )
        .map_err(|error| format!("cannot copy local blob state: {error}"))?;

    // A source path inside the copied library must follow the destination;
    // external watched paths intentionally remain external.
    transaction
        .execute(
            "UPDATE main.ingest_job
             SET source_path = replace(source_path, ?1, ?2)
             WHERE source_path = ?1 OR source_path LIKE ?3",
            rusqlite::params![
                source_root.to_string_lossy(),
                destination_root.to_string_lossy(),
                format!("{}%", source_root.to_string_lossy()),
            ],
        )
        .map_err(|error| format!("cannot rewrite copied ingest paths: {error}"))?;

    transaction
        .execute(
            "UPDATE main.library_meta SET schema_version = 1, revision = 1",
            [],
        )
        .map_err(|error| format!("cannot reset destination markers: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("cannot commit converted rows: {error}"))?;
    destination
        .execute_batch("DETACH DATABASE source")
        .map_err(|error| format!("cannot detach source database: {error}"))?;
    Ok(())
}

const COPY_TABLES: &[(&str, &str)] = &[
    ("media_file", "file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms, frame_count, has_audio, perceptual_hash, dominant_color_hex, dominant_palette_blob, color_analysis_version, created_at"),
    ("library_item", "item_id, item_key, kind, cover_media_item_id, created_at, updated_at"),
    ("library_root", "item_id, lifecycle, sort_rank"),
    ("media_asset", "item_id, file_id, name, captured_at, imported_at, updated_at"),
    ("collection_member", "collection_id, media_item_id, position_rank"),
    ("media_view", "item_id, viewed_at"),
    ("tag", "tag_id, namespace, subtag"),
    ("folder", "folder_id, folder_key, name, parent_id, icon, color, notes, sort_rank, watch_path, watch_enabled, watch_subfolders, created_at, updated_at"),
    ("smart_folder", "smart_folder_id, smart_folder_key, name, parent_id, icon, color, notes, predicate_json, sort_field, sort_order, display_order, created_at, updated_at"),
    ("subscription", "subscription_id, subscription_key, name, schedule, paused, initial_post_limit, periodic_post_limit, next_run_at, created_at"),
    ("subscription_query", "query_id, query_key, subscription_id, site_id, domain_key, query_kind, query_text, display_name, notes, group_posts, paused, resume_cursor, initial_run_complete, last_success_at, last_failure_at, last_failure_kind, last_failure_message"),
    ("subscription_run", "run_id, subscription_id, requested_by, status, started_at, finished_at, failure_kind, error_message, created_at"),
    ("subscription_run_query", "run_query_id, run_id, query_id, status, resume_cursor, attempt_count, available_at, started_at, finished_at, failure_kind, error_message"),
    ("source_post", "source_post_id, site_id, post_key, canonical_url, creator_name, title, description, captured_at, metadata_json, root_item_id, created_at, updated_at"),
    ("subscription_source_post", "subscription_id, query_id, source_post_id, last_seen_run_id"),
    ("source_item", "source_item_id, source_post_id, item_key, position, media_url, canonical_url, media_item_id, state, last_error, created_at, updated_at"),
    ("subscription_run_source_item", "run_query_id, source_item_id"),
    ("ingest_job", "ingest_job_id, job_key, source_kind, source_path, source_item_id, payload_json, lifecycle, delete_after_ingest, status, attempt_count, available_at, last_error, created_at, updated_at"),
    ("work_item", "work_id, media_item_id, file_id, file_hash, work_type, status, attempt_count, available_at, last_error, created_at, updated_at"),
    ("subscription_issue", "issue_id, issue_key, subscription_id, query_id, issue_kind, message, detail, status, first_seen_at, last_seen_at, resolved_at"),
    ("credential", "site_id, credential_type, display_name, created_at"),
    ("credential_health", "site_id, status, checked_at, last_error"),
    ("duplicate", "file_id_a, file_id_b, distance, status, decided_at, winner_file_id"),
    ("file_color", "color_id, file_id, hex, l, a, b"),
    ("view_pref", "scope, value_json"),
    ("setting", "key, value_json"),
    ("cloud_tombstone", "object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical, device_id, causal_frontier_json, deleted_at, purge_after"),
];

fn rebuild_derived_state(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("cannot start derived-state rebuild: {error}"))?;
    transaction
        .execute_batch(
            "DELETE FROM projection_checkpoint;
             DELETE FROM root_name_fts;
             DELETE FROM root_notes_fts;
             DELETE FROM source_text_fts;
             DELETE FROM folder_summary;
             DELETE FROM tag_summary;
             DELETE FROM root_summary;

             INSERT INTO root_summary (
                 root_item_id, lifecycle, kind, cover_media_item_id, media_count,
                 total_size_bytes, imported_at, captured_at, sort_rating,
                 sort_name, updated_at
             )
             SELECT lr.item_id, lr.lifecycle, 'media', ma.item_id, 1,
                    mf.size_bytes, ma.imported_at, ma.captured_at,
                    metadata.rating, metadata.name, metadata.updated_at
             FROM library_root lr
             JOIN library_item item
               ON item.item_id = lr.item_id AND item.kind = 'media'
             JOIN media_asset ma ON ma.item_id = lr.item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             JOIN root_metadata metadata ON metadata.root_item_id = lr.item_id
             UNION ALL
             SELECT lr.item_id, lr.lifecycle, 'collection',
                    COALESCE(item.cover_media_item_id, (
                        SELECT ordered.media_item_id
                        FROM collection_member ordered
                        WHERE ordered.collection_id = lr.item_id
                        ORDER BY ordered.position_rank, ordered.media_item_id
                        LIMIT 1
                    )),
                    COUNT(member.media_item_id), COALESCE(SUM(mf.size_bytes), 0),
                    MAX(ma.imported_at), MAX(ma.captured_at), metadata.rating,
                    metadata.name, metadata.updated_at
             FROM library_root lr
             JOIN library_item item
               ON item.item_id = lr.item_id AND item.kind = 'collection'
             JOIN root_metadata metadata ON metadata.root_item_id = lr.item_id
             JOIN collection_member member ON member.collection_id = lr.item_id
             JOIN media_asset ma ON ma.item_id = member.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             GROUP BY lr.item_id, lr.lifecycle, item.cover_media_item_id,
                      metadata.rating, metadata.name, metadata.updated_at;

             UPDATE lifecycle_summary
             SET root_count = (
                     SELECT COUNT(*) FROM root_summary
                     WHERE root_summary.lifecycle = lifecycle_summary.lifecycle
                 ),
                 media_count = COALESCE((
                     SELECT SUM(media_count) FROM root_summary
                     WHERE root_summary.lifecycle = lifecycle_summary.lifecycle
                 ), 0),
                 total_size_bytes = COALESCE((
                     SELECT SUM(total_size_bytes) FROM root_summary
                     WHERE root_summary.lifecycle = lifecycle_summary.lifecycle
                 ), 0);

             INSERT INTO folder_summary (
                 folder_id, visible_root_count, media_count, total_size_bytes
             )
             SELECT folder.folder_id, COUNT(summary.root_item_id),
                    COALESCE(SUM(summary.media_count), 0),
                    COALESCE(SUM(summary.total_size_bytes), 0)
             FROM folder
             LEFT JOIN folder_item membership ON membership.folder_id = folder.folder_id
             LEFT JOIN root_summary summary
               ON summary.root_item_id = membership.item_id
              AND summary.lifecycle = 'active'
             GROUP BY folder.folder_id;

             INSERT INTO tag_summary (
                 tag_id, visible_root_count, assignment_count
             )
             SELECT tag.tag_id,
                    COUNT(DISTINCT CASE WHEN summary.lifecycle = 'active'
                                        THEN relation.root_item_id END),
                    COALESCE(SUM(relation.direct_assignment_count), 0)
             FROM tag
             LEFT JOIN root_tag relation ON relation.tag_id = tag.tag_id
             LEFT JOIN root_summary summary ON summary.root_item_id = relation.root_item_id
             GROUP BY tag.tag_id;

             INSERT INTO root_name_fts(root_item_id, name)
             WITH root_media(root_item_id, media_item_id) AS (
                 SELECT summary.root_item_id, summary.root_item_id
                 FROM root_summary summary WHERE summary.kind = 'media'
                 UNION ALL
                 SELECT summary.root_item_id, member.media_item_id
                 FROM root_summary summary
                 JOIN collection_member member
                   ON member.collection_id = summary.root_item_id
                 WHERE summary.kind = 'collection'
             )
             SELECT metadata.root_item_id,
                    TRIM(COALESCE(metadata.name, '') || ' ' ||
                         COALESCE(GROUP_CONCAT(DISTINCT media.name), ''))
             FROM root_metadata metadata
             LEFT JOIN root_media mapped ON mapped.root_item_id = metadata.root_item_id
             LEFT JOIN media_asset media ON media.item_id = mapped.media_item_id
             GROUP BY metadata.root_item_id;

             INSERT INTO root_notes_fts(root_item_id, notes)
             SELECT root_item_id, COALESCE(notes, '') FROM root_metadata;

             INSERT INTO source_text_fts(source_post_id, searchable_text)
             SELECT post.source_post_id,
                    TRIM(COALESCE(post.site_id, '') || ' ' || COALESCE(post.post_key, '') || ' ' ||
                         COALESCE(post.canonical_url, '') || ' ' ||
                         COALESCE(post.creator_name, '') || ' ' || COALESCE(post.title, '') || ' ' ||
                         COALESCE(post.description, '') || ' ' || COALESCE((
                             SELECT GROUP_CONCAT(
                                 COALESCE(item.media_url, '') || ' ' ||
                                 COALESCE(item.canonical_url, ''), ' '
                             )
                             FROM source_item item
                             WHERE item.source_post_id = post.source_post_id
                         ), ''))
             FROM source_post post;

             DELETE FROM search_dirty_name;
             DELETE FROM search_dirty_notes;
             DELETE FROM search_dirty_source;",
        )
        .map_err(|error| format!("cannot seed derived-state rebuild: {error}"))?;

    populate_canonical_bitmaps(&transaction)?;

    let checksum = canonical_payload_checksum(&transaction)?;
    transaction
        .execute(
            "INSERT INTO projection_checkpoint (
                 component, schema_fingerprint, implementation_hash,
                 database_revision, checksum, health, checkpoint_path, updated_at
             ) VALUES (
                 'canonical-bitmaps', ?2,
                 'canonical-bitmap-v1', 1, ?1, 'healthy', NULL, datetime('now')
             )",
            rusqlite::params![checksum, schema::CURRENT_SCHEMA_FINGERPRINT],
        )
        .map_err(|error| format!("cannot record projection checkpoint metadata: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("cannot commit derived-state rebuild: {error}"))
}

fn canonical_payload_checksum(transaction: &rusqlite::Transaction<'_>) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut statement = transaction
        .prepare(
            "SELECT domain, key_id, shard, revision, cardinality,
                    format_version, checksum, payload
             FROM canonical_bitmap ORDER BY domain, key_id, shard",
        )
        .map_err(|error| format!("cannot inspect canonical bitmaps: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Vec<u8>>(7)?,
            ))
        })
        .map_err(|error| format!("cannot inspect canonical bitmaps: {error}"))?;
    for row in rows {
        let (domain, key_id, shard, revision, cardinality, version, checksum, payload) =
            row.map_err(|error| format!("cannot inspect canonical bitmaps: {error}"))?;
        for value in [domain, key_id, shard, revision, cardinality, version] {
            digest.update(value.to_le_bytes());
        }
        digest.update(checksum.as_bytes());
        digest.update(payload);
    }
    drop(statement);

    let mut statement = transaction
        .prepare(
            "SELECT owner_kind, owner_id, revision, cardinality,
                    format_version, checksum, payload
             FROM canonical_order ORDER BY owner_kind, owner_id",
        )
        .map_err(|error| format!("cannot inspect canonical ordering: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Vec<u8>>(6)?,
            ))
        })
        .map_err(|error| format!("cannot inspect canonical ordering: {error}"))?;
    for row in rows {
        let (kind, owner_id, revision, cardinality, version, checksum, payload) =
            row.map_err(|error| format!("cannot inspect canonical ordering: {error}"))?;
        digest.update(kind.as_bytes());
        for value in [owner_id, revision, cardinality, version] {
            digest.update(value.to_le_bytes());
        }
        digest.update(checksum.as_bytes());
        digest.update(payload);
    }
    Ok(hex::encode(digest.finalize()))
}

fn populate_canonical_bitmaps(transaction: &rusqlite::Transaction<'_>) -> Result<(), String> {
    transaction
        .execute("DELETE FROM canonical_bitmap", [])
        .map_err(|error| format!("cannot clear canonical bitmaps: {error}"))?;
    transaction
        .execute("DELETE FROM canonical_order", [])
        .map_err(|error| format!("cannot clear canonical ordering: {error}"))?;

    let mut lifecycle = BTreeMap::<i64, RoaringBitmap>::new();
    let mut ratings = BTreeMap::<i64, RoaringBitmap>::new();
    let root_rows = {
        let mut statement = transaction
            .prepare(
                "SELECT root.item_id, root.lifecycle, metadata.rating
                 FROM library_root root
                 JOIN root_metadata metadata ON metadata.root_item_id = root.item_id",
            )
            .map_err(|error| format!("cannot read root memberships: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<u8>>(2)?,
                ))
            })
            .map_err(|error| format!("cannot read root memberships: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("cannot read root memberships: {error}"))?;
        rows
    };
    for (root_id, lifecycle_name, rating) in root_rows {
        let root_id = bitmap_id(root_id)?;
        let lifecycle_key = match lifecycle_name.as_str() {
            "active" => LIFECYCLE_ACTIVE_KEY,
            "inbox" => LIFECYCLE_INBOX_KEY,
            "trash" => LIFECYCLE_TRASH_KEY,
            _ => return Err(format!("invalid lifecycle {lifecycle_name}")),
        };
        lifecycle.entry(lifecycle_key).or_default().insert(root_id);
        ratings
            .entry(rating_key(rating))
            .or_default()
            .insert(root_id);
    }
    write_bitmap_map(transaction, BitmapDomain::Lifecycle, lifecycle)?;
    write_bitmap_map(transaction, BitmapDomain::Rating, ratings)?;

    let tags = collect_pair_bitmaps(
        transaction,
        "SELECT tag_id, root_item_id FROM root_tag ORDER BY tag_id, root_item_id",
        "tag memberships",
    )?;
    write_bitmap_map(transaction, BitmapDomain::Tag, tags)?;
    let folders = collect_pair_bitmaps(
        transaction,
        "SELECT folder_id, item_id FROM folder_item ORDER BY folder_id, item_id",
        "folder memberships",
    )?;
    write_bitmap_map(transaction, BitmapDomain::Folder, folders)?;

    let groups = {
        let mut statement = transaction
            .prepare(
                "SELECT collection_id, media_item_id
                 FROM collection_member
                 ORDER BY collection_id, position_rank, media_item_id",
            )
            .map_err(|error| format!("cannot read group ordering: {error}"))?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .map_err(|error| format!("cannot read group ordering: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("cannot read group ordering: {error}"))?;
        rows
    };
    let mut ordered_groups = BTreeMap::<i64, Vec<u32>>::new();
    for (group_id, media_id) in groups {
        ordered_groups
            .entry(group_id)
            .or_default()
            .push(bitmap_id(media_id)?);
    }
    for (group_id, order) in ordered_groups {
        replace_ordered_membership(transaction, "group", group_id, 1, &order)
            .map_err(|error| format!("cannot store group {group_id}: {error}"))?;
    }

    let root_kinds = collect_text_root_facts(
        transaction,
        "SELECT item.kind, root.item_id
         FROM library_root root JOIN library_item item ON item.item_id = root.item_id",
        "root kinds",
    )?;
    write_dictionary_bitmaps(transaction, BitmapDomain::RootKind, root_kinds)?;
    let mime = collect_text_root_facts(
        transaction,
        "SELECT file.mime_type, COALESCE(member.collection_id, asset.item_id)
         FROM media_asset asset
         JOIN media_file file ON file.file_id = asset.file_id
         LEFT JOIN collection_member member ON member.media_item_id = asset.item_id",
        "MIME memberships",
    )?;
    let mut mime_families = BTreeMap::<String, RoaringBitmap>::new();
    for (mime_type, roots) in &mime {
        let family = mime_type
            .split_once('/')
            .map_or(mime_type.as_str(), |value| value.0);
        *mime_families.entry(family.to_string()).or_default() |= roots;
    }
    write_dictionary_bitmaps(transaction, BitmapDomain::Mime, mime)?;
    write_dictionary_bitmaps(transaction, BitmapDomain::MimeFamily, mime_families)?;
    Ok(())
}

fn collect_pair_bitmaps(
    transaction: &rusqlite::Transaction<'_>,
    sql: &str,
    label: &str,
) -> Result<BTreeMap<i64, RoaringBitmap>, String> {
    let mut statement = transaction
        .prepare(sql)
        .map_err(|error| format!("cannot read {label}: {error}"))?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|error| format!("cannot read {label}: {error}"))?;
    let mut bitmaps = BTreeMap::<i64, RoaringBitmap>::new();
    for row in rows {
        let (key_id, root_id) = row.map_err(|error| format!("cannot read {label}: {error}"))?;
        bitmaps
            .entry(key_id)
            .or_default()
            .insert(bitmap_id(root_id)?);
    }
    Ok(bitmaps)
}

fn collect_text_root_facts(
    transaction: &rusqlite::Transaction<'_>,
    sql: &str,
    label: &str,
) -> Result<BTreeMap<String, RoaringBitmap>, String> {
    let mut statement = transaction
        .prepare(sql)
        .map_err(|error| format!("cannot read {label}: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| format!("cannot read {label}: {error}"))?;
    let mut bitmaps = BTreeMap::<String, RoaringBitmap>::new();
    for row in rows {
        let (value, root_id) = row.map_err(|error| format!("cannot read {label}: {error}"))?;
        bitmaps
            .entry(value)
            .or_default()
            .insert(bitmap_id(root_id)?);
    }
    Ok(bitmaps)
}

fn write_bitmap_map(
    transaction: &rusqlite::Transaction<'_>,
    domain: BitmapDomain,
    bitmaps: BTreeMap<i64, RoaringBitmap>,
) -> Result<(), String> {
    for (key_id, bitmap) in bitmaps {
        replace_bitmap(transaction, domain, key_id, 1, &bitmap)
            .map_err(|error| format!("cannot store {domain:?} bitmap {key_id}: {error}"))?;
    }
    Ok(())
}

fn write_dictionary_bitmaps(
    transaction: &rusqlite::Transaction<'_>,
    domain: BitmapDomain,
    bitmaps: BTreeMap<String, RoaringBitmap>,
) -> Result<(), String> {
    for (value, bitmap) in bitmaps {
        let key_id = intern_key(transaction, domain, &value)
            .map_err(|error| format!("cannot intern {domain:?} key {value}: {error}"))?;
        replace_bitmap(transaction, domain, i64::from(key_id), 1, &bitmap)
            .map_err(|error| format!("cannot store {domain:?} bitmap {value}: {error}"))?;
    }
    Ok(())
}

fn bitmap_id(value: i64) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("local ID {value} is outside canonical u32 range"))
}

fn write_manifest(root: &Path, source: &Path, counts: &Counts) -> Result<(), String> {
    let mut file = File::create(root.join(MANIFEST_FILE))
        .map_err(|error| format!("cannot write conversion manifest: {error}"))?;
    writeln!(file, "format=schema-v1-conversion")
        .and_then(|_| writeln!(file, "source={}", source.display()))
        .and_then(|_| writeln!(file, "schema=1"))
        .and_then(|_| writeln!(file, "revision=1"))
        .and_then(|_| writeln!(file, "library_items={}", counts.library_items))
        .and_then(|_| writeln!(file, "created_at={}", timestamp()))
        .map_err(|error| format!("cannot write conversion manifest: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("cannot flush conversion manifest: {error}"))
}

fn activate_library(source: &Path, destination: &Path, backup: &Path) -> Result<(), String> {
    let source = library_root(source)?;
    let destination = library_root(destination)?;
    let backup = library_root(backup)?;
    if source == destination || source == backup || destination == backup {
        return Err("source, destination, and backup must be distinct directories".into());
    }
    if !backup.join(MANIFEST_FILE).is_file() {
        return Err(format!(
            "backup {} is not a converter backup",
            backup.display()
        ));
    }
    let destination_report = validate_library(&destination)?;
    if destination_report.schema_version != 1 || destination_report.revision != 1 {
        return Err("destination is not a validated schema-v1 library".into());
    }
    for suffix in ["-wal", "-shm"] {
        if database_path(&source)
            .with_extension(format!("sqlite{suffix}"))
            .exists()
        {
            return Err(format!(
                "source has an active SQLite sidecar; stop Picto before activation"
            ));
        }
    }
    let temporary = source.join(format!(".library.sqlite.activate-{}", timestamp()));
    copy_file(&database_path(&destination), &temporary)?;
    atomic_replace(&temporary, &database_path(&source))?;
    sync_directory(&source)?;
    Ok(())
}

fn remove_conversion_directory(path: &Path) -> Result<(), String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("cannot resolve cleanup path {}: {error}", path.display()))?;
    if !path.join(MANIFEST_FILE).is_file() {
        return Err(format!(
            "refusing to delete {} without converter manifest",
            path.display()
        ));
    }
    fs::remove_dir_all(&path).map_err(|error| format!("cannot remove {}: {error}", path.display()))
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        fs::remove_file(destination)
            .map_err(|error| format!("cannot replace {}: {error}", destination.display()))?;
    }
    fs::rename(source, destination)
        .map_err(|error| format!("cannot activate temporary database: {error}"))
}

fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        fs::rename(source, destination)
            .map_err(|error| format!("cannot atomically activate database: {error}"))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source_w: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination_w: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        if unsafe {
            MoveFileExW(
                source_w.as_ptr(),
                destination_w.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err("cannot atomically activate database on Windows".into());
        }
        Ok(())
    }
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("cannot flush directory {}: {error}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use picto_core::store::schema;
    use tempfile::tempdir;

    fn fixture() -> tempfile::TempDir {
        let directory = tempdir().unwrap();
        let mut connection = Connection::open(database_path(directory.path())).unwrap();
        schema::create(&mut connection).unwrap();
        connection
            .execute_batch(
                "UPDATE library_meta SET schema_version = 128 WHERE singleton = 1;
                 INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
                     VALUES (1, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'image/png', 10, '2026-01-01'),
                            (2, 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'image/png', 20, '2026-01-02');
                 INSERT INTO library_item (item_id, item_key, kind, label, created_at, updated_at)
                     VALUES (1, 'collection:1', 'collection', 'Legacy label', '2026-01-01', '2026-01-02'),
                            (2, 'media:2', 'media', 'Page one', '2026-01-01', '2026-01-01'),
                            (3, 'media:3', 'media', 'Cover page', '2026-01-02', '2026-01-02');
                 UPDATE library_item SET cover_media_item_id = 3 WHERE item_id = 1;
                 INSERT INTO library_root (item_id, lifecycle) VALUES (1, 'active');
                 INSERT INTO media_asset (
                     item_id, file_id, name, notes, rating, source_urls_json,
                     imported_at, updated_at
                 ) VALUES
                     (2, 1, 'Page one', 'first note', 1,
                      '[\"https://example.test/a\",\"https://example.test/common\"]',
                      '2026-01-01', '2026-01-01'),
                     (3, 2, 'Cover page', 'cover note', 4,
                      '[\"https://example.test/common\",\"https://example.test/b\"]',
                      '2026-01-02', '2026-01-02');
                 INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (1, 2, 0), (1, 3, 1);
                 INSERT INTO tag (tag_id, namespace, subtag)
                     VALUES (1, 'general', 'sample'), (2, 'creator', 'artist');
                 INSERT INTO media_tag (media_item_id, tag_id, source, provenance_mask)
                     VALUES (2, 1, 'local', 1), (3, 1, 'source', 2),
                            (2, 2, 'source', 4);
                 INSERT INTO folder (
                     folder_id, folder_key, name, created_at, updated_at
                 ) VALUES (1, 'folder:1', 'Sample folder', '2026-01-01', '2026-01-01');
                 INSERT INTO folder_item(folder_id, item_id) VALUES (1, 1);",
            )
            .unwrap();
        directory
    }

    #[test]
    fn validation_is_read_only_and_checks_collection_order() {
        let source = fixture();
        let before = fs::read(database_path(source.path())).unwrap();
        let report = validate_library(source.path()).unwrap();
        assert_eq!(report.counts.library_items, 3);
        assert_eq!(report.counts.collection_members, 2);
        assert_eq!(before, fs::read(database_path(source.path())).unwrap());
    }

    #[test]
    fn conversion_creates_v1_destination_and_preserves_canonical_rows() {
        let source = fixture();
        let parent = tempdir().unwrap();
        let destination = parent.path().join("converted");
        let backup = parent.path().join("backup");
        let report = convert_library(source.path(), &destination, &backup).unwrap();
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.revision, 1);
        assert!(destination.join(MANIFEST_FILE).is_file());
        assert!(backup.join(MANIFEST_FILE).is_file());
        let destination_connection = open_read_only(&destination).unwrap();
        let fts_count: i64 = destination_connection
            .query_row("SELECT COUNT(*) FROM root_name_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fts_count, 1);
        let metadata: (String, i64, String, String) = destination_connection
            .query_row(
                "SELECT name, rating, notes, source_urls_json
                 FROM root_metadata WHERE root_item_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(metadata.0, "Cover page");
        assert_eq!(metadata.1, 4);
        assert_eq!(metadata.2, "cover note");
        let urls: Vec<String> = serde_json::from_str(&metadata.3).unwrap();
        assert_eq!(
            urls,
            vec![
                "https://example.test/a",
                "https://example.test/b",
                "https://example.test/common",
            ]
        );
        let sample_tag: (i64, i64) = destination_connection
            .query_row(
                "SELECT direct_assignment_count, provenance_mask
                 FROM root_tag WHERE root_item_id = 1 AND tag_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(sample_tag, (2, 3));
        assert_eq!(
            destination_connection
                .query_row("SELECT COUNT(*) FROM root_tag", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            destination_connection
                .query_row(
                    "SELECT media_count, total_size_bytes FROM root_summary
                     WHERE root_item_id = 1",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .unwrap(),
            (2, 30)
        );
        for removed in [
            "media_tag",
            "root_tag_count",
            "tag_search_fts",
            "folder_search_fts",
            "search_dirty_tag",
            "search_dirty_folder",
        ] {
            assert!(!schema_object_exists(&destination_connection, "table", removed).unwrap());
        }
        let source_report = validate_library(source.path()).unwrap();
        assert_eq!(
            source_report.schema_version,
            LEGACY_DEVELOPMENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn activation_requires_explicit_confirmation_and_keeps_backup() {
        let source = fixture();
        let parent = tempdir().unwrap();
        let destination = parent.path().join("converted");
        let backup = parent.path().join("backup");
        convert_library(source.path(), &destination, &backup).unwrap();
        let error = run_activation_without_confirmation(source.path(), &destination, &backup);
        assert!(error.contains("--yes"));
        assert!(backup.join(MANIFEST_FILE).is_file());
    }

    #[test]
    fn explicit_activation_replaces_only_the_source_database() {
        let source = fixture();
        let original_files = fs::read_dir(source.path()).unwrap().count();
        let parent = tempdir().unwrap();
        let destination = parent.path().join("converted");
        let backup = parent.path().join("backup");
        convert_library(source.path(), &destination, &backup).unwrap();

        activate_library(source.path(), &destination, &backup).unwrap();

        let report = validate_library(source.path()).unwrap();
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.revision, 1);
        assert_eq!(fs::read_dir(source.path()).unwrap().count(), original_files);
        assert!(backup.join(DATABASE_FILE).is_file());
        assert!(backup.join(MANIFEST_FILE).is_file());
    }

    fn run_activation_without_confirmation(
        source: &Path,
        destination: &Path,
        backup: &Path,
    ) -> String {
        let options = Options {
            command: Command::Activate,
            source: Some(source.to_path_buf()),
            destination: Some(destination.to_path_buf()),
            backup: Some(backup.to_path_buf()),
            dry_run: false,
            destructive: false,
            yes: false,
        };
        let error = if !options.yes {
            "activation mutates the source library; pass --yes explicitly".to_string()
        } else {
            String::new()
        };
        error
    }
}
