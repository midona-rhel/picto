//! Standalone schema-128 to `picto_library` generation-1 converter.
//!
//! This is the only code allowed to understand the retired schema. It copies
//! roots/media, collections, tags, folders, smart folders, subscriptions, and
//! settings, and credential metadata. Credential secrets already live in the
//! operating-system credential store. Cloud state, history, tasks, work
//! queues, and other runtime state deliberately start clean.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::DateTime;
use picto_core::blob_store::mime_to_extension;
use picto_core::media_processing::colors::deserialize_dominant_palette_blob;
use picto_library::database::WorkPriority;
use picto_library::predicate::{
    FilterClause, FilterExpr, ItemSort, SetMatchMode, SortDirection, SortField, TextField,
    ViewQuerySpec,
};
use picto_library::{
    FolderId, ImmutableMediaFacts, LabColor, Library, Lifecycle, MediaId, PreparedCollectionImport,
    PreparedImport, Rating, RootId, SmartFolderId, SmartFolderInput, SourceIdentity, TagId,
};
use rusqlite::backup::Backup;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

const DATABASE_FILE: &str = "library.sqlite";
const MANIFEST_FILE: &str = ".picto-schema-v1-conversion";
const SOURCE_SCHEMA: i64 = 128;

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
    roots: i64,
    media: i64,
    collections: i64,
    tags: i64,
    folders: i64,
    smart_folders: i64,
    subscriptions: i64,
    settings: i64,
}

#[derive(Debug)]
struct ValidationReport {
    schema: i64,
    revision: i64,
    counts: Counts,
}

#[derive(Debug, Clone)]
struct SourceFolder {
    id: i64,
    stable_key: String,
    parent_id: Option<i64>,
    name: String,
    icon: Option<String>,
    color: Option<String>,
    notes: Option<String>,
    sort_rank: Option<i64>,
    watch_path: Option<String>,
    watch_enabled: bool,
    watch_subfolders: bool,
}

#[derive(Debug, Clone)]
struct SourceSmartFolder {
    id: i64,
    stable_key: String,
    parent_id: Option<i64>,
    name: String,
    icon: Option<String>,
    color: Option<String>,
    notes: Option<String>,
    predicate_json: String,
    sort_field: Option<String>,
    sort_order: Option<String>,
    display_order: Option<i64>,
}

#[derive(Debug, Clone)]
struct SourceRoot {
    id: i64,
    stable_key: String,
    kind: String,
    label: Option<String>,
    cover_media_id: Option<i64>,
    created_at: String,
    updated_at: String,
    lifecycle: Lifecycle,
}

#[derive(Debug, Clone)]
struct SourceMedia {
    id: i64,
    stable_key: String,
    name: String,
    notes: Option<String>,
    rating: Rating,
    source_urls: Vec<String>,
    captured_at_ms: Option<i64>,
    imported_at_ms: i64,
    file_hash: String,
    mime: String,
    size_bytes: u64,
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    frame_count: Option<u32>,
    perceptual_hash: Option<String>,
    palette: Vec<LabColor>,
    source_identity: Option<SourceIdentity>,
}

#[derive(Default)]
struct IdMaps {
    roots: HashMap<i64, RootId>,
    media: HashMap<i64, MediaId>,
    folders: HashMap<i64, FolderId>,
    smart_folders: HashMap<i64, SmartFolderId>,
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
            let report = validate_source(&source)?;
            println!(
                "validated {}: schema={} revision={} roots={} media={} collections={} tags={} folders={} smart_folders={} subscriptions={} settings={}",
                source.display(), report.schema, report.revision, report.counts.roots,
                report.counts.media, report.counts.collections, report.counts.tags,
                report.counts.folders, report.counts.smart_folders,
                report.counts.subscriptions, report.counts.settings,
            );
        }
        Command::Convert => {
            let source = required_root(options.source, "--source")?;
            let destination = required_output(options.destination, "--destination")?;
            let backup = options
                .backup
                .map(|path| output_root(&path))
                .transpose()?
                .unwrap_or_else(|| timestamped_sibling(&source, "pre-cutover"));
            let report = validate_source(&source)?;
            if options.dry_run {
                println!(
                    "dry-run: schema-128 {} -> {} with backup {}; roots={} media={}",
                    source.display(),
                    destination.display(),
                    backup.display(),
                    report.counts.roots,
                    report.counts.media,
                );
            } else {
                let converted = convert_library(&source, &destination, &backup)?;
                println!(
                    "converted {} -> {} (backup {}) schema=1 revision=1 roots={} media={}",
                    source.display(),
                    destination.display(),
                    backup.display(),
                    converted.roots,
                    converted.media,
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
                "activated {} from {}; backup retained at {}",
                source.display(),
                destination.display(),
                backup.display()
            );
        }
        Command::Cleanup => {
            if !options.destructive || !options.yes {
                return Err("cleanup is destructive; pass --destructive and --yes".into());
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
        println!(
            "Picto schema-128 converter\n\n\
             [validate] --source PATH\n\
             convert --source PATH --destination PATH [--backup PATH] [--dry-run]\n\
             activate --source PATH --destination PATH --backup PATH --yes\n\
             cleanup --backup PATH [--destination PATH] --destructive --yes\n\n\
             Stop Picto before conversion. Conversion never activates implicitly."
        );
        process::exit(0);
    }
    let mut options = Options {
        command: Command::Validate,
        source: None,
        destination: None,
        backup: None,
        dry_run: false,
        destructive: false,
        yes: false,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "validate" => options.command = Command::Validate,
            "convert" => options.command = Command::Convert,
            "activate" => options.command = Command::Activate,
            "cleanup" => options.command = Command::Cleanup,
            "--dry-run" => options.dry_run = true,
            "--destructive" => options.destructive = true,
            "--yes" => options.yes = true,
            flag @ ("--source" | "--destination" | "--backup") => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{flag} requires a path"))?;
                match flag {
                    "--source" => options.source = Some(value.into()),
                    "--destination" => options.destination = Some(value.into()),
                    _ => options.backup = Some(value.into()),
                }
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
        index += 1;
    }
    if options.command != Command::Convert && options.dry_run {
        return Err("--dry-run is only valid with convert".into());
    }
    Ok(options)
}

fn required_root(path: Option<PathBuf>, flag: &str) -> Result<PathBuf, String> {
    library_root(&path.ok_or_else(|| format!("{flag} is required"))?)
}

fn required_output(path: Option<PathBuf>, flag: &str) -> Result<PathBuf, String> {
    output_root(&path.ok_or_else(|| format!("{flag} is required"))?)
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
            .ok_or_else(|| "database has no parent".into());
    }
    Err(format!("{} is not a Picto library", path.display()))
}

fn output_root(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().map_err(|e| e.to_string())?.join(path)
    };
    if absolute.exists() {
        return Err(format!("refusing to overwrite {}", absolute.display()));
    }
    let parent = absolute
        .parent()
        .ok_or_else(|| "output has no parent".to_string())?
        .canonicalize()
        .map_err(|error| format!("cannot resolve output parent: {error}"))?;
    Ok(parent.join(
        absolute
            .file_name()
            .ok_or_else(|| "output has no name".to_string())?,
    ))
}

fn database_path(root: &Path) -> PathBuf {
    root.join(DATABASE_FILE)
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn timestamped_sibling(source: &Path, label: &str) -> PathBuf {
    let name = source
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("library");
    source
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{name}.{label}-{}", timestamp()))
}

fn open_source(root: &Path) -> Result<Connection, String> {
    let uri = format!(
        "file:{}?mode=ro&immutable=1",
        database_path(root).to_string_lossy()
    );
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|error| format!("cannot open schema-128 source: {error}"))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    Ok(connection)
}

fn validate_source(root: &Path) -> Result<ValidationReport, String> {
    let connection = open_source(root)?;
    let (schema, revision) = connection
        .query_row(
            "SELECT schema_version, revision FROM library_meta WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(|error| format!("missing schema metadata: {error}"))?;
    if schema != SOURCE_SCHEMA {
        return Err(format!(
            "converter accepts only schema {SOURCE_SCHEMA}; found {schema}"
        ));
    }
    let quick: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if quick != "ok" {
        return Err(format!("source quick_check failed: {quick}"));
    }
    let foreign_keys: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    if foreign_keys != 0 {
        return Err(format!("source has {foreign_keys} foreign-key violations"));
    }
    Ok(ValidationReport {
        schema,
        revision,
        counts: source_counts(&connection)?,
    })
}

fn source_counts(connection: &Connection) -> Result<Counts, String> {
    Ok(Counts {
        roots: count(connection, "library_root")?,
        media: count(connection, "media_asset")?,
        collections: connection
            .query_row(
                "SELECT COUNT(*) FROM library_item WHERE kind = 'collection'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?,
        tags: count(connection, "tag")?,
        folders: count(connection, "folder")?,
        smart_folders: count(connection, "smart_folder")?,
        subscriptions: count(connection, "subscription")?,
        settings: count(connection, "setting")? + count(connection, "view_pref")?,
    })
}

fn count(connection: &Connection, table: &str) -> Result<i64, String> {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())
}

fn convert_library(source: &Path, destination: &Path, backup: &Path) -> Result<Counts, String> {
    let source = library_root(source)?;
    let source_report = validate_source(&source)?;
    reject_overlap(&source, destination, backup)?;
    fs::create_dir(backup).map_err(|e| format!("cannot create {}: {e}", backup.display()))?;
    fs::create_dir(destination)
        .map_err(|e| format!("cannot create {}: {e}", destination.display()))?;
    write_manifest(backup, &source)?;
    write_manifest(destination, &source)?;

    let source_db = open_source(&source)?;
    backup_database(&source_db, &database_path(backup))?;
    let folders = load_folders(&source_db)?;
    let smart_folders = load_smart_folders(&source_db)?;
    let roots = load_roots(&source_db)?;
    let media = load_media(&source_db)?;
    let tags = load_media_tags(&source_db)?;
    let folder_memberships = load_folder_memberships(&source_db)?;
    let folder_orders = load_folder_orders(&source_db)?;
    let collection_members = load_collection_members(&source_db)?;

    let mut library =
        Library::create(database_path(destination)).map_err(|error| error.to_string())?;
    let mut maps = IdMaps::default();
    create_folders(&library, &folders, &mut maps)?;
    import_roots(
        &library,
        &source,
        &roots,
        &media,
        &tags,
        &folder_memberships,
        &collection_members,
        &mut maps,
    )?;
    preserve_unused_tags(&library, &source_db)?;
    // Direct dictionary insertion is converter-only. Reopen once so the
    // projection sees every tag before compiling smart folders.
    drop(library);
    library = Library::open(database_path(destination)).map_err(|error| error.to_string())?;
    finish_folders(&library, &folders, &folder_orders, &maps)?;
    create_smart_folders(&library, &smart_folders, &mut maps)?;
    copy_subscriptions(&library, &source, &maps)?;
    copy_settings(&library, &source_db, &maps)?;
    while library
        .settle_fts(512)
        .map_err(|e| e.to_string())?
        .is_some()
    {}
    drop(library);

    normalize_generation_one(&database_path(destination))?;
    let reopened = Library::open(database_path(destination)).map_err(|e| e.to_string())?;
    reopened
        .write_projection_checkpoint()
        .map_err(|e| e.to_string())?;
    drop(reopened);
    let converted = validate_destination(destination)?;
    if converted.roots != source_report.counts.roots
        || converted.media != source_report.counts.media
        || converted.collections != source_report.counts.collections
        || converted.tags != source_report.counts.tags
        || converted.folders != source_report.counts.folders
        || converted.smart_folders != source_report.counts.smart_folders
        || converted.subscriptions != source_report.counts.subscriptions
    {
        return Err(format!(
            "converted counts differ: source={:?}, destination={converted:?}",
            source_report.counts
        ));
    }
    Ok(converted)
}

fn backup_database(source: &Connection, destination: &Path) -> Result<(), String> {
    let mut output =
        Connection::open(destination).map_err(|e| format!("cannot create database backup: {e}"))?;
    let backup = Backup::new(source, &mut output)
        .map_err(|e| format!("cannot initialize database backup: {e}"))?;
    backup
        .run_to_completion(256, std::time::Duration::from_millis(5), None)
        .map_err(|e| format!("cannot copy database backup: {e}"))?;
    drop(backup);
    let quick: String = output
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|e| format!("cannot validate database backup: {e}"))?;
    if quick != "ok" {
        return Err(format!("database backup quick_check failed: {quick}"));
    }
    Ok(())
}

fn reject_overlap(source: &Path, destination: &Path, backup: &Path) -> Result<(), String> {
    for path in [destination, backup] {
        if path.starts_with(source) || source.starts_with(path) {
            return Err("source and outputs overlap".into());
        }
    }
    if destination.starts_with(backup) || backup.starts_with(destination) {
        return Err("destination and backup overlap".into());
    }
    Ok(())
}

fn load_folders(connection: &Connection) -> Result<Vec<SourceFolder>, String> {
    let mut statement = connection.prepare("SELECT folder_id, folder_key, parent_id, name, icon, color, notes, sort_rank, watch_path, watch_enabled, watch_subfolders FROM folder ORDER BY COALESCE(sort_rank, folder_id), folder_id").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(SourceFolder {
                id: row.get(0)?,
                stable_key: row.get(1)?,
                parent_id: row.get(2)?,
                name: row.get(3)?,
                icon: row.get(4)?,
                color: row.get(5)?,
                notes: row.get(6)?,
                sort_rank: row.get(7)?,
                watch_path: row.get(8)?,
                watch_enabled: row.get::<_, i64>(9)? != 0,
                watch_subfolders: row.get::<_, i64>(10)? != 0,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn load_smart_folders(connection: &Connection) -> Result<Vec<SourceSmartFolder>, String> {
    let mut statement = connection.prepare("SELECT smart_folder_id, smart_folder_key, parent_id, name, icon, color, notes, predicate_json, sort_field, sort_order, display_order FROM smart_folder ORDER BY COALESCE(display_order, smart_folder_id), smart_folder_id").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(SourceSmartFolder {
                id: row.get(0)?,
                stable_key: row.get(1)?,
                parent_id: row.get(2)?,
                name: row.get(3)?,
                icon: row.get(4)?,
                color: row.get(5)?,
                notes: row.get(6)?,
                predicate_json: row.get(7)?,
                sort_field: row.get(8)?,
                sort_order: row.get(9)?,
                display_order: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn load_roots(connection: &Connection) -> Result<Vec<SourceRoot>, String> {
    let mut statement = connection.prepare("SELECT item.item_id, item.item_key, item.kind, item.label, item.cover_media_item_id, item.created_at, item.updated_at, root.lifecycle FROM library_root root JOIN library_item item ON item.item_id = root.item_id ORDER BY item.item_id").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let lifecycle: String = row.get(7)?;
            Ok(SourceRoot {
                id: row.get(0)?,
                stable_key: row.get(1)?,
                kind: row.get(2)?,
                label: row.get(3)?,
                cover_media_id: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                lifecycle: parse_lifecycle(&lifecycle)
                    .map_err(rusqlite::Error::InvalidParameterName)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn load_media(connection: &Connection) -> Result<HashMap<i64, SourceMedia>, String> {
    let identities = load_source_identities(connection)?;
    let mut statement = connection.prepare(
        "SELECT asset.item_id, item.item_key, COALESCE(NULLIF(TRIM(asset.name), ''), NULLIF(TRIM(item.label), ''), item.item_key), asset.notes, asset.rating, asset.source_urls_json, asset.captured_at, asset.imported_at, file.file_hash, file.mime_type, file.size_bytes, file.pixel_width, file.pixel_height, file.duration_ms, file.frame_count, file.perceptual_hash, file.dominant_palette_blob FROM media_asset asset JOIN library_item item ON item.item_id = asset.item_id JOIN media_file file ON file.file_id = asset.file_id ORDER BY asset.item_id"
    ).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let urls: Option<String> = row.get(5)?;
            let palette_blob: Option<Vec<u8>> = row.get(16)?;
            Ok(SourceMedia {
                id,
                stable_key: row.get(1)?,
                name: row.get(2)?,
                notes: row.get(3)?,
                rating: parse_rating(row.get(4)?),
                source_urls: urls
                    .as_deref()
                    .and_then(|value| serde_json::from_str(value).ok())
                    .unwrap_or_default(),
                captured_at_ms: row
                    .get::<_, Option<String>>(6)?
                    .as_deref()
                    .and_then(parse_timestamp),
                imported_at_ms: parse_timestamp(&row.get::<_, String>(7)?).unwrap_or_default(),
                file_hash: row.get(8)?,
                mime: row.get(9)?,
                size_bytes: row.get::<_, i64>(10)?.max(0) as u64,
                width: row
                    .get::<_, Option<i64>>(11)?
                    .and_then(|v| u32::try_from(v).ok()),
                height: row
                    .get::<_, Option<i64>>(12)?
                    .and_then(|v| u32::try_from(v).ok()),
                duration_ms: row
                    .get::<_, Option<i64>>(13)?
                    .and_then(|v| u64::try_from(v).ok()),
                frame_count: row
                    .get::<_, Option<i64>>(14)?
                    .and_then(|v| u32::try_from(v).ok()),
                perceptual_hash: row.get(15)?,
                palette: decode_palette(palette_blob.as_deref()),
                source_identity: identities.get(&id).cloned(),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut output = HashMap::new();
    for row in rows {
        let media = row.map_err(|e| e.to_string())?;
        output.insert(media.id, media);
    }
    Ok(output)
}

fn load_source_identities(connection: &Connection) -> Result<HashMap<i64, SourceIdentity>, String> {
    let mut statement = connection.prepare(
        "SELECT item.media_item_id, post.site_id, item.item_key, COALESCE(post.title, post.description, post.canonical_url) FROM source_item item JOIN source_post post ON post.source_post_id = item.source_post_id WHERE item.media_item_id IS NOT NULL ORDER BY item.source_item_id"
    ).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                SourceIdentity {
                    source_key: row.get(1)?,
                    source_item_key: row.get(2)?,
                    source_text: row.get(3)?,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut output = HashMap::new();
    for row in rows {
        let (id, identity) = row.map_err(|e| e.to_string())?;
        output.entry(id).or_insert(identity);
    }
    Ok(output)
}

fn load_media_tags(connection: &Connection) -> Result<HashMap<i64, Vec<String>>, String> {
    let mut statement = connection.prepare("SELECT relation.media_item_id, tag.namespace, tag.subtag FROM media_tag relation JOIN tag ON tag.tag_id = relation.tag_id ORDER BY relation.media_item_id, tag.tag_id").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                tag_name(&row.get::<_, String>(1)?, &row.get::<_, String>(2)?),
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut output = HashMap::new();
    for row in rows {
        let (id, tag) = row.map_err(|e| e.to_string())?;
        output.entry(id).or_insert_with(Vec::new).push(tag);
    }
    Ok(output)
}

fn load_folder_memberships(connection: &Connection) -> Result<HashMap<i64, Vec<i64>>, String> {
    let mut statement = connection.prepare("SELECT item_id, folder_id FROM folder_item ORDER BY item_id, COALESCE(position_rank, 9223372036854775807), folder_id").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut output = HashMap::new();
    for row in rows {
        let (root, folder) = row.map_err(|e| e.to_string())?;
        output.entry(root).or_insert_with(Vec::new).push(folder);
    }
    Ok(output)
}

fn load_folder_orders(connection: &Connection) -> Result<HashMap<i64, Vec<i64>>, String> {
    let mut statement = connection
        .prepare(
            "SELECT folder_id, item_id FROM folder_item
         ORDER BY folder_id, COALESCE(position_rank, 9223372036854775807), item_id",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut output = HashMap::new();
    for row in rows {
        let (folder, root) = row.map_err(|e| e.to_string())?;
        output.entry(folder).or_insert_with(Vec::new).push(root);
    }
    Ok(output)
}

fn load_collection_members(connection: &Connection) -> Result<HashMap<i64, Vec<i64>>, String> {
    let mut statement = connection.prepare("SELECT collection_id, media_item_id FROM collection_member ORDER BY collection_id, position_rank, media_item_id").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut output = HashMap::new();
    for row in rows {
        let (group, member) = row.map_err(|e| e.to_string())?;
        output.entry(group).or_insert_with(Vec::new).push(member);
    }
    Ok(output)
}

fn create_folders(
    library: &Library,
    source: &[SourceFolder],
    maps: &mut IdMaps,
) -> Result<(), String> {
    let mut pending = source.iter().collect::<Vec<_>>();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|folder| {
            let parent = folder
                .parent_id
                .and_then(|id| maps.folders.get(&id).copied());
            if folder.parent_id.is_some() && parent.is_none() {
                return true;
            }
            match library.create_folder(&folder.name, parent) {
                Ok((id, _)) => {
                    maps.folders.insert(folder.id, id);
                    false
                }
                Err(error) => {
                    eprintln!("folder {} failed: {error}", folder.id);
                    true
                }
            }
        });
        if pending.len() == before {
            return Err("folder hierarchy contains a cycle or invalid parent".into());
        }
    }
    Ok(())
}

fn import_roots(
    library: &Library,
    source_root: &Path,
    roots: &[SourceRoot],
    media: &HashMap<i64, SourceMedia>,
    tags: &HashMap<i64, Vec<String>>,
    folder_memberships: &HashMap<i64, Vec<i64>>,
    collection_members: &HashMap<i64, Vec<i64>>,
    maps: &mut IdMaps,
) -> Result<(), String> {
    let mut standalone_inputs = Vec::new();
    let mut standalone_old_ids = Vec::new();
    for root in roots.iter().filter(|root| root.kind == "media") {
        let source = media
            .get(&root.id)
            .ok_or_else(|| format!("root {} has no media", root.id))?;
        standalone_inputs.push(prepared_media(
            source_root,
            source,
            root.lifecycle,
            tags.get(&root.id),
            folder_memberships.get(&root.id),
            &maps.folders,
        )?);
        standalone_old_ids.push(root.id);
        if standalone_inputs.len() == 64 {
            flush_standalone(
                library,
                &mut standalone_inputs,
                &mut standalone_old_ids,
                maps,
            )?;
        }
    }
    flush_standalone(
        library,
        &mut standalone_inputs,
        &mut standalone_old_ids,
        maps,
    )?;

    for root in roots.iter().filter(|root| root.kind == "collection") {
        let members = collection_members
            .get(&root.id)
            .ok_or_else(|| format!("collection {} has no members", root.id))?;
        if members.is_empty() {
            return Err(format!("collection {} has no members", root.id));
        }
        let cover = root.cover_media_id.unwrap_or(members[0]);
        let cover_index = members
            .iter()
            .position(|id| *id == cover)
            .ok_or_else(|| format!("collection {} cover is not a member", root.id))?;
        let mut inputs = Vec::with_capacity(members.len());
        for member in members {
            let source = media
                .get(member)
                .ok_or_else(|| format!("collection member {member} is missing"))?;
            inputs.push(prepared_media(
                source_root,
                source,
                root.lifecycle,
                tags.get(member),
                folder_memberships.get(&root.id),
                &maps.folders,
            )?);
        }
        let modified_at_ms = parse_timestamp(&root.updated_at)
            .or_else(|| parse_timestamp(&root.created_at))
            .unwrap_or_default();
        let (new_root, _) = library
            .ingest_collection(&PreparedCollectionImport {
                members: inputs,
                cover_index,
                name: root.label.clone(),
                modified_at_ms,
            })
            .map_err(|e| format!("collection {}: {e}", root.id))?;
        maps.roots.insert(root.id, new_root);
        let details = library.details(new_root).map_err(|e| e.to_string())?;
        for (old_media, new_media) in members.iter().zip(details.media.iter()) {
            maps.media.insert(*old_media, new_media.media_id);
        }
        set_stable_key(library, new_root.0, &root.stable_key)?;
    }
    Ok(())
}

fn flush_standalone(
    library: &Library,
    inputs: &mut Vec<PreparedImport>,
    old_ids: &mut Vec<i64>,
    maps: &mut IdMaps,
) -> Result<(), String> {
    if inputs.is_empty() {
        return Ok(());
    }
    let outputs = library.ingest_batch(inputs).map_err(|e| e.to_string())?;
    for (old_id, (root_id, _)) in old_ids.drain(..).zip(outputs) {
        maps.roots.insert(old_id, root_id);
        maps.media.insert(old_id, MediaId(root_id.0));
    }
    inputs.clear();
    Ok(())
}

fn prepared_media(
    library_root: &Path,
    source: &SourceMedia,
    lifecycle: Lifecycle,
    tags: Option<&Vec<String>>,
    folders: Option<&Vec<i64>>,
    folder_map: &HashMap<i64, FolderId>,
) -> Result<PreparedImport, String> {
    let folder_ids = folders
        .into_iter()
        .flatten()
        .map(|id| {
            folder_map
                .get(id)
                .copied()
                .ok_or_else(|| format!("unknown folder {id}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PreparedImport {
        stable_key: source.stable_key.clone(),
        media_name: source.name.clone(),
        file_path: original_path(library_root, &source.file_hash, &source.mime)?
            .to_string_lossy()
            .into_owned(),
        facts: ImmutableMediaFacts {
            mime: source.mime.clone(),
            size_bytes: source.size_bytes,
            width: source.width,
            height: source.height,
            duration_ms: source.duration_ms,
            frame_count: source.frame_count,
            content_hash: source.file_hash.clone(),
            perceptual_hash: source.perceptual_hash.clone(),
            palette: source.palette.clone(),
        },
        lifecycle,
        rating: source.rating,
        notes: source.notes.clone(),
        tags: tags.cloned().unwrap_or_default(),
        folders: folder_ids,
        source_urls: source.source_urls.clone(),
        source_identity: source.source_identity.clone(),
        imported_at_ms: source.imported_at_ms,
        captured_at_ms: source.captured_at_ms,
    })
}

fn original_path(root: &Path, hash: &str, mime: &str) -> Result<PathBuf, String> {
    if hash.len() < 4 {
        return Err(format!("invalid content hash {hash}"));
    }
    Ok(root
        .join("blobs")
        .join("f")
        .join(&hash[..2])
        .join(&hash[2..4])
        .join(format!("{hash}.{}", mime_to_extension(mime))))
}

fn preserve_unused_tags(library: &Library, source: &Connection) -> Result<(), String> {
    let existing = library
        .tags()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|tag| tag_name(&tag.namespace, &tag.subname))
        .collect::<HashSet<_>>();
    let mut statement = source
        .prepare("SELECT namespace, subtag FROM tag ORDER BY tag_id")
        .map_err(|e| e.to_string())?;
    let source_tags = statement
        .query_map([], |row| {
            Ok(tag_name(
                &row.get::<_, String>(0)?,
                &row.get::<_, String>(1)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    let missing = source_tags
        .into_iter()
        .filter(|name| !existing.contains(name))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    library.database().maintenance_write(WorkPriority::CanonicalIngest, |transaction| {
        for name in &missing {
            let (namespace, subname) = split_tag(name);
            let namespace_id = match transaction.query_row(
                "SELECT namespace_id FROM tag_namespace WHERE display_name = ?1",
                [namespace],
                |row| row.get::<_, u32>(0),
            ).optional()? {
                Some(namespace_id) => namespace_id,
                None => {
                    let namespace_id = picto_library::LibraryDatabase::allocate_id(transaction)?;
                    transaction.execute(
                        "INSERT INTO tag_namespace(namespace_id, stable_key, display_name) VALUES (?1, ?2, ?3)",
                        params![namespace_id, uuid::Uuid::new_v4().to_string(), namespace],
                    )?;
                    namespace_id
                }
            };
            let tag_id = picto_library::LibraryDatabase::allocate_id(transaction)?;
            transaction.execute("INSERT INTO tag_definition(tag_id, stable_key, namespace_id, subname) VALUES (?1, ?2, ?3, ?4)", params![tag_id, uuid::Uuid::new_v4().to_string(), namespace_id, subname])?;
        }
        Ok(())
    }).map_err(|e| e.to_string())
}

fn finish_folders(
    library: &Library,
    source: &[SourceFolder],
    orders: &HashMap<i64, Vec<i64>>,
    maps: &IdMaps,
) -> Result<(), String> {
    for folder in source {
        let id = maps.folders[&folder.id];
        library
            .set_folder_metadata(
                id,
                folder.icon.as_deref(),
                folder.color.as_deref(),
                folder.notes.as_deref(),
            )
            .map_err(|e| e.to_string())?;
        if folder.watch_enabled {
            let path = folder
                .watch_path
                .as_deref()
                .ok_or_else(|| format!("watched folder {} has no path", folder.id))?;
            library
                .set_folder_watch(id, path, folder.watch_subfolders)
                .map_err(|e| e.to_string())?;
        }
        set_folder_stable_key(library, id, &folder.stable_key)?;
    }
    for folder in source {
        let ordered = orders
            .get(&folder.id)
            .into_iter()
            .flatten()
            .filter_map(|old_root| maps.roots.get(old_root).copied())
            .collect::<Vec<_>>();
        if !ordered.is_empty() {
            library
                .reorder_folder_items(id_for_folder(maps, folder.id)?, &ordered)
                .map_err(|e| e.to_string())?;
        }
    }
    let mut siblings: BTreeMap<Option<i64>, Vec<&SourceFolder>> = BTreeMap::new();
    for folder in source {
        siblings.entry(folder.parent_id).or_default().push(folder);
    }
    for (parent, mut children) in siblings {
        children.sort_by_key(|folder| (folder.sort_rank.unwrap_or(folder.id), folder.id));
        library
            .reorder_folder_children(
                parent.and_then(|id| maps.folders.get(&id).copied()),
                &children
                    .iter()
                    .map(|folder| maps.folders[&folder.id])
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn id_for_folder(maps: &IdMaps, old: i64) -> Result<FolderId, String> {
    maps.folders
        .get(&old)
        .copied()
        .ok_or_else(|| format!("unknown folder {old}"))
}

fn create_smart_folders(
    library: &Library,
    source: &[SourceSmartFolder],
    maps: &mut IdMaps,
) -> Result<(), String> {
    let tags = library
        .tags()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|tag| (tag_name(&tag.namespace, &tag.subname), tag.tag_id))
        .collect::<HashMap<_, _>>();
    let mut pending = source.iter().collect::<Vec<_>>();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|smart| {
            let parent = smart
                .parent_id
                .and_then(|id| maps.smart_folders.get(&id).copied());
            if smart.parent_id.is_some() && parent.is_none() {
                return true;
            }
            let result = translate_smart(smart, &tags, &maps.folders).and_then(|view| {
                library
                    .create_smart_folder(SmartFolderInput {
                        name: smart.name.clone(),
                        parent_id: parent,
                        icon: smart.icon.clone(),
                        color: smart.color.clone(),
                        notes: smart.notes.clone(),
                        view,
                    })
                    .map_err(|e| e.to_string())
            });
            match result {
                Ok((id, _)) => {
                    maps.smart_folders.insert(smart.id, id);
                    let _ = set_smart_stable_key(library, id, &smart.stable_key);
                    false
                }
                Err(error) => {
                    eprintln!("smart folder {} failed: {error}", smart.id);
                    true
                }
            }
        });
        if pending.len() == before {
            return Err("smart-folder conversion could not make progress".into());
        }
    }
    let mut siblings: BTreeMap<Option<i64>, Vec<&SourceSmartFolder>> = BTreeMap::new();
    for smart in source {
        siblings.entry(smart.parent_id).or_default().push(smart);
    }
    for (parent, mut children) in siblings {
        children.sort_by_key(|smart| (smart.display_order.unwrap_or(smart.id), smart.id));
        library
            .reorder_smart_folder_children(
                parent.and_then(|id| maps.smart_folders.get(&id).copied()),
                &children
                    .iter()
                    .map(|smart| maps.smart_folders[&smart.id])
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn translate_smart(
    source: &SourceSmartFolder,
    tags: &HashMap<String, TagId>,
    folders: &HashMap<i64, FolderId>,
) -> Result<ViewQuerySpec, String> {
    let value: Value = serde_json::from_str(&source.predicate_json)
        .map_err(|e| format!("invalid predicate JSON: {e}"))?;
    let groups = value
        .get("groups")
        .and_then(Value::as_array)
        .ok_or_else(|| "predicate has no groups".to_string())?;
    let mut expressions = Vec::new();
    for group in groups {
        let rules = group
            .get("rules")
            .and_then(Value::as_array)
            .ok_or_else(|| "group has no rules".to_string())?;
        let children = rules
            .iter()
            .map(|rule| translate_rule(rule, tags, folders))
            .collect::<Result<Vec<_>, _>>()?;
        let mut expression = if group.get("match_mode").and_then(Value::as_str) == Some("any") {
            FilterExpr::Any(children)
        } else {
            FilterExpr::All(children)
        };
        if group
            .get("negate")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            expression = FilterExpr::Not(Box::new(expression));
        }
        expressions.push(expression);
    }
    let field = match source.sort_field.as_deref() {
        Some("captured_at") | Some("date_created") => SortField::CapturedAt,
        Some("name") => SortField::Name,
        Some("rating") => SortField::Rating,
        Some("size") | Some("total_size") => SortField::TotalSize,
        Some("random") => SortField::Random,
        Some("folder_order") => SortField::FolderOrder,
        _ => SortField::ImportedAt,
    };
    let direction = if source.sort_order.as_deref() == Some("ascending") {
        SortDirection::Ascending
    } else {
        SortDirection::Descending
    };
    Ok(ViewQuerySpec {
        filter: FilterExpr::All(expressions),
        sort: ItemSort {
            field,
            direction,
            random_seed: None,
        },
    })
}

fn translate_rule(
    rule: &Value,
    tags: &HashMap<String, TagId>,
    folders: &HashMap<i64, FolderId>,
) -> Result<FilterExpr, String> {
    let field = rule
        .get("field")
        .and_then(Value::as_str)
        .ok_or_else(|| "rule has no field".to_string())?;
    let op = rule.get("op").and_then(Value::as_str).unwrap_or("contains");
    let values = rule
        .get("values")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text_values = values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let clause = match field {
        "tags" => FilterClause::Tags {
            tag_ids: text_values
                .iter()
                .map(|name| {
                    tags.get(name)
                        .copied()
                        .ok_or_else(|| format!("smart folder references missing tag '{name}'"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            mode: if op.contains("all") {
                SetMatchMode::All
            } else if op.contains("exact") {
                SetMatchMode::Exact
            } else {
                SetMatchMode::Any
            },
        },
        "folders" => FilterClause::Folders {
            folder_ids: values
                .iter()
                .filter_map(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
                .map(|id| {
                    folders
                        .get(&id)
                        .copied()
                        .ok_or_else(|| format!("smart folder references missing folder {id}"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            mode: if op.contains("all") {
                SetMatchMode::All
            } else if op.contains("exact") {
                SetMatchMode::Exact
            } else {
                SetMatchMode::Any
            },
        },
        "name" => FilterClause::Text {
            field: TextField::Name,
            query: rule_text(rule)?,
        },
        "notes" => FilterClause::Text {
            field: TextField::Notes,
            query: rule_text(rule)?,
        },
        "source" | "source_url" => FilterClause::Text {
            field: TextField::SourceUrl,
            query: rule_text(rule)?,
        },
        "text" => FilterClause::Text {
            field: TextField::Global,
            query: rule_text(rule)?,
        },
        "mime" | "mime_type" => FilterClause::Mime {
            values: text_values,
            families: Vec::new(),
        },
        "rating" => FilterClause::Ratings {
            ratings: values
                .iter()
                .filter_map(Value::as_i64)
                .map(|v| parse_rating(Some(v)))
                .collect(),
        },
        "date_created" | "captured_at" => date_clause(rule, "captured")?,
        "date_added" | "imported_at" => date_clause(rule, "imported")?,
        "date_modified" | "modified_at" => date_clause(rule, "modified")?,
        "width" => number_clause(rule, "width")?,
        "height" => number_clause(rule, "height")?,
        "duration" => number_clause(rule, "duration")?,
        "size" | "total_size" => number_clause(rule, "size")?,
        "shape" | "has_audio" => {
            return Err(format!(
                "removed smart-folder field '{field}' must be edited before conversion"
            ))
        }
        other => return Err(format!("unsupported smart-folder field '{other}'")),
    };
    let expression = FilterExpr::Clause(clause);
    Ok(
        if op.starts_with("exclude") || op == "not" || op == "does_not_contain" {
            FilterExpr::Not(Box::new(expression))
        } else {
            expression
        },
    )
}

fn rule_text(rule: &Value) -> Result<String, String> {
    rule.get("value")
        .and_then(Value::as_str)
        .or_else(|| {
            rule.get("values")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
        .ok_or_else(|| "text rule has no value".into())
}

fn date_clause(rule: &Value, kind: &str) -> Result<FilterClause, String> {
    let minimum = rule
        .get("value")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)
        .and_then(|v| u64::try_from(v).ok());
    let maximum = rule
        .get("value2")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)
        .and_then(|v| u64::try_from(v).ok());
    Ok(match kind {
        "captured" => FilterClause::CapturedAt {
            minimum_ms: minimum,
            maximum_ms: maximum,
        },
        "modified" => FilterClause::ModifiedAt {
            minimum_ms: minimum,
            maximum_ms: maximum,
        },
        _ => FilterClause::ImportedAt {
            minimum_ms: minimum,
            maximum_ms: maximum,
        },
    })
}

fn number_clause(rule: &Value, kind: &str) -> Result<FilterClause, String> {
    let minimum = rule.get("value").and_then(value_u64);
    let maximum = rule.get("value2").and_then(value_u64);
    Ok(match kind {
        "width" => FilterClause::Width { minimum, maximum },
        "height" => FilterClause::Height { minimum, maximum },
        "duration" => FilterClause::Duration {
            minimum_ms: minimum,
            maximum_ms: maximum,
        },
        _ => FilterClause::TotalSize {
            minimum_bytes: minimum,
            maximum_bytes: maximum,
        },
    })
}

fn value_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn copy_subscriptions(library: &Library, source_root: &Path, maps: &IdMaps) -> Result<(), String> {
    let source_uri = format!(
        "file:{}?mode=ro&immutable=1",
        database_path(source_root).to_string_lossy()
    );
    library.database().maintenance_write(WorkPriority::CanonicalIngest, |transaction| {
        transaction.execute("ATTACH DATABASE ?1 AS old", [&source_uri])?;
        transaction.execute_batch("CREATE TEMP TABLE root_map(old_id INTEGER PRIMARY KEY, new_id INTEGER NOT NULL); CREATE TEMP TABLE media_map(old_id INTEGER PRIMARY KEY, new_id INTEGER NOT NULL);")?;
        { let mut insert = transaction.prepare("INSERT INTO temp.root_map VALUES (?1, ?2)")?; for (old, new) in &maps.roots { insert.execute(params![old, new.0])?; } }
        { let mut insert = transaction.prepare("INSERT INTO temp.media_map VALUES (?1, ?2)")?; for (old, new) in &maps.media { insert.execute(params![old, new.0])?; } }
        transaction.execute_batch(
            "INSERT INTO subscription SELECT * FROM old.subscription;
             INSERT INTO subscription_query(
                 query_id, query_key, subscription_id, site_id, domain_key,
                 query_kind, query_text, display_name, notes, group_posts,
                 paused, resume_cursor, initial_run_complete, last_success_at,
                 last_failure_at, last_failure_kind, last_failure_message
             )
             SELECT query_id, query_key, subscription_id, site_id, domain_key,
                    query_kind, query_text, display_name, notes, group_posts,
                    paused, resume_cursor, initial_run_complete, last_success_at,
                    last_failure_at, last_failure_kind, last_failure_message
             FROM old.subscription_query;
             INSERT INTO subscription_run SELECT * FROM old.subscription_run;
             INSERT INTO subscription_run_query SELECT * FROM old.subscription_run_query;
             INSERT INTO source_post(source_post_id, site_id, post_key, canonical_url, creator_name, title, description, captured_at, metadata_json, root_item_id, created_at, updated_at)
             SELECT post.source_post_id, post.site_id, post.post_key, post.canonical_url, post.creator_name, post.title, post.description, post.captured_at, post.metadata_json, roots.new_id, post.created_at, post.updated_at FROM old.source_post post LEFT JOIN temp.root_map roots ON roots.old_id = post.root_item_id;
             INSERT INTO subscription_source_post SELECT * FROM old.subscription_source_post;
             INSERT INTO source_item(source_item_id, source_post_id, item_key, position, media_url, canonical_url, media_item_id, state, last_error, created_at, updated_at)
             SELECT item.source_item_id, item.source_post_id, item.item_key, item.position, item.media_url, item.canonical_url, media.new_id, item.state, item.last_error, item.created_at, item.updated_at FROM old.source_item item LEFT JOIN temp.media_map media ON media.old_id = item.media_item_id;
             INSERT INTO subscription_run_source_item SELECT * FROM old.subscription_run_source_item;
             INSERT INTO subscription_issue SELECT * FROM old.subscription_issue;
             INSERT INTO credential SELECT * FROM old.credential;
             INSERT INTO credential_health SELECT * FROM old.credential_health;"
        )?;
        Ok(())
    }).map_err(|e| format!("copy subscriptions: {e}"))
}

fn copy_settings(library: &Library, source: &Connection, maps: &IdMaps) -> Result<(), String> {
    let settings = load_key_values(source, "setting", "key")?;
    let views = load_key_values(source, "view_pref", "scope")?;
    library
        .database()
        .maintenance_write(WorkPriority::CanonicalIngest, |transaction| {
            for (key, json) in &settings {
                let rewritten = rewrite_setting(key, json, maps)
                    .map_err(picto_library::LibraryError::InvalidInput)?;
                transaction.execute(
                    "INSERT OR REPLACE INTO setting(key, value_json) VALUES (?1, ?2)",
                    params![key, rewritten],
                )?;
            }
            for (scope, json) in &views {
                let Some(scope) = rewrite_scope(scope, maps) else {
                    continue;
                };
                transaction.execute(
                    "INSERT OR REPLACE INTO view_pref(scope, value_json) VALUES (?1, ?2)",
                    params![scope, json],
                )?;
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    apply_folder_covers(library, &settings, maps)
}

fn load_key_values(
    connection: &Connection,
    table: &str,
    key: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {key}, value_json FROM {table} ORDER BY {key}"
        ))
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn rewrite_setting(key: &str, json: &str, maps: &IdMaps) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(json).map_err(|e| format!("setting {key}: {e}"))?;
    if key == "application" {
        if let Some(object) = value.as_object_mut() {
            object.remove("folderCovers");
            if let Some(items) = object
                .get_mut("sidebarQuickAccess")
                .and_then(Value::as_array_mut)
            {
                *items = items
                    .drain(..)
                    .filter_map(|item| {
                        let Some(text) = item.as_str() else {
                            return Some(item);
                        };
                        if let Some(old) = text
                            .strip_prefix("folder:")
                            .and_then(|id| id.parse::<i64>().ok())
                        {
                            return maps
                                .folders
                                .get(&old)
                                .map(|id| Value::String(format!("folder:{}", id.0)));
                        }
                        if let Some(old) = text
                            .strip_prefix("smart:")
                            .and_then(|id| id.parse::<i64>().ok())
                        {
                            return maps
                                .smart_folders
                                .get(&old)
                                .map(|id| Value::String(format!("smart:{}", id.0)));
                        }
                        Some(item)
                    })
                    .collect();
            }
        }
    } else if key.ends_with(".destination") {
        rewrite_id_field(&mut value, "target_folder_id", &maps.folders);
        rewrite_id_array(&mut value, "target_folder_ids", &maps.folders);
    } else if key.ends_with(".cover") {
        rewrite_id_field(&mut value, "media_item_id", &maps.media);
    }
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

fn rewrite_id_field<T: Copy + Into<u32>>(value: &mut Value, field: &str, map: &HashMap<i64, T>) {
    let Some(old) = value.get(field).and_then(Value::as_i64) else {
        return;
    };
    if let Some(new) = map.get(&old) {
        value[field] = Value::from((*new).into());
    } else {
        value[field] = Value::Null;
    }
}

fn rewrite_id_array<T: Copy + Into<u32>>(value: &mut Value, field: &str, map: &HashMap<i64, T>) {
    let Some(values) = value.get_mut(field).and_then(Value::as_array_mut) else {
        return;
    };
    *values = values
        .iter()
        .filter_map(Value::as_i64)
        .filter_map(|id| map.get(&id))
        .map(|id| Value::from((*id).into()))
        .collect();
}

fn rewrite_scope(scope: &str, maps: &IdMaps) -> Option<String> {
    if let Some(old) = scope
        .strip_prefix("folder:")
        .and_then(|id| id.parse::<i64>().ok())
    {
        return maps.folders.get(&old).map(|id| format!("folder:{}", id.0));
    }
    if let Some(old) = scope
        .strip_prefix("smart:")
        .and_then(|id| id.parse::<i64>().ok())
    {
        return maps
            .smart_folders
            .get(&old)
            .map(|id| format!("smart:{}", id.0));
    }
    Some(scope.to_owned())
}

fn apply_folder_covers(
    library: &Library,
    settings: &[(String, String)],
    maps: &IdMaps,
) -> Result<(), String> {
    let Some((_, json)) = settings.iter().find(|(key, _)| key == "application") else {
        return Ok(());
    };
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let Some(covers) = value.get("folderCovers").and_then(Value::as_object) else {
        return Ok(());
    };
    for (folder, root) in covers {
        let Some(folder) = folder
            .parse::<i64>()
            .ok()
            .and_then(|id| maps.folders.get(&id).copied())
        else {
            continue;
        };
        let Some(root) = root.as_i64().and_then(|id| maps.roots.get(&id).copied()) else {
            continue;
        };
        library
            .set_folder_cover(folder, root)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn normalize_generation_one(path: &Path) -> Result<(), String> {
    let connection = Connection::open(path).map_err(|e| e.to_string())?;
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
         DELETE FROM cloud_journal;
         DELETE FROM cloud_outbox;
         DELETE FROM cloud_applied_mutation;
         DELETE FROM cloud_device_frontier;
         DELETE FROM cloud_field_clock;
         DELETE FROM cloud_membership_clock;
         DELETE FROM cloud_tombstone;
         DELETE FROM cloud_quarantine;
         DELETE FROM cloud_snapshot;
         DELETE FROM cloud_blob_state;
         DELETE FROM work_item;
         DELETE FROM ingest_job;
         DELETE FROM deletion_tombstone;
         DELETE FROM blob_cleanup_queue;
         DELETE FROM projection_checkpoint;
         UPDATE library_meta SET revision = 1 WHERE singleton = 1;
         UPDATE canonical_bitmap SET revision = 1;
         UPDATE ordered_membership SET revision = 1;
         COMMIT;
         PRAGMA wal_checkpoint(TRUNCATE);",
        )
        .map_err(|e| format!("normalize destination: {e}"))?;
    Ok(())
}

fn validate_destination(root: &Path) -> Result<Counts, String> {
    let library = Library::open(database_path(root)).map_err(|e| e.to_string())?;
    let connection = Connection::open(database_path(root)).map_err(|e| e.to_string())?;
    let quick: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if quick != "ok" {
        return Err(format!("destination quick_check failed: {quick}"));
    }
    let foreign_keys: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    if foreign_keys != 0 {
        return Err(format!(
            "destination has {foreign_keys} foreign-key violations"
        ));
    }
    let revision = library.database().revision().map_err(|e| e.to_string())?;
    if revision != 1 {
        return Err(format!("destination revision is {revision}, expected 1"));
    }
    Ok(Counts {
        roots: count(&connection, "library_root")?,
        media: count(&connection, "media_item")?,
        collections: connection
            .query_row(
                "SELECT COUNT(*) FROM library_item WHERE item_kind = 2",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?,
        tags: count(&connection, "tag_definition")?,
        folders: count(&connection, "folder_definition")?,
        smart_folders: count(&connection, "smart_folder_definition")?,
        subscriptions: count(&connection, "subscription")?,
        settings: count(&connection, "setting")? + count(&connection, "view_pref")?,
    })
}

fn set_stable_key(library: &Library, id: u32, key: &str) -> Result<(), String> {
    auxiliary_update(
        library,
        "UPDATE library_item SET stable_key = ?2 WHERE local_id = ?1",
        id,
        key,
    )
}
fn set_folder_stable_key(library: &Library, id: FolderId, key: &str) -> Result<(), String> {
    auxiliary_update(
        library,
        "UPDATE folder_definition SET stable_key = ?2 WHERE folder_id = ?1",
        id.0,
        key,
    )
}
fn set_smart_stable_key(library: &Library, id: SmartFolderId, key: &str) -> Result<(), String> {
    auxiliary_update(
        library,
        "UPDATE smart_folder_definition SET stable_key = ?2 WHERE smart_folder_id = ?1",
        id.0,
        key,
    )
}
fn auxiliary_update(library: &Library, sql: &str, id: u32, value: &str) -> Result<(), String> {
    library
        .database()
        .maintenance_write(WorkPriority::CanonicalIngest, |transaction| {
            transaction.execute(sql, params![id, value])?;
            Ok(())
        })
        .map_err(|e| e.to_string())
}

fn activate_library(source: &Path, destination: &Path, backup: &Path) -> Result<(), String> {
    if !backup.join(MANIFEST_FILE).is_file() {
        return Err("backup has no converter manifest".into());
    }
    validate_destination(destination)?;
    let source_db = database_path(source);
    let next_db = database_path(destination);
    let staged = source.join(format!(".library.sqlite.activate-{}", timestamp()));
    copy_database_file(&next_db, &staged)?;
    checkpoint_closed_database(&source_db)?;
    let retired = source.join(format!(".library.sqlite.schema-128-{}", timestamp()));
    fs::rename(&source_db, &retired).map_err(|e| format!("retire schema-128 database: {e}"))?;
    if let Err(error) = fs::rename(&staged, &source_db) {
        let _ = fs::rename(&retired, &source_db);
        return Err(format!("activate schema-1 database: {error}"));
    }
    if let Err(error) = validate_destination(source) {
        let failed = source.join(format!(".library.sqlite.failed-schema-1-{}", timestamp()));
        let _ = fs::rename(&source_db, &failed);
        let _ = fs::rename(&retired, &source_db);
        return Err(error);
    }
    fs::remove_file(&retired)
        .map_err(|e| format!("remove retired database after activation: {e}"))?;
    Ok(())
}

fn copy_database_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::copy(source, destination).map_err(|e| {
        format!(
            "copy {} to {}: {e}",
            source.display(),
            destination.display()
        )
    })?;
    File::open(destination)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("sync staged database {}: {e}", destination.display()))
}

fn checkpoint_closed_database(database: &Path) -> Result<(), String> {
    let connection = Connection::open(database)
        .map_err(|e| format!("open schema-128 database for activation: {e}"))?;
    let (busy, remaining): (i64, i64) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get::<_, i64>(1)? - row.get::<_, i64>(2)?))
        })
        .map_err(|e| format!("checkpoint schema-128 database: {e}"))?;
    if busy != 0 || remaining != 0 {
        return Err(format!(
            "schema-128 database is still in use (busy={busy}, remaining={remaining}); close Picto first"
        ));
    }
    drop(connection);
    for suffix in ["-wal", "-shm"] {
        let mut path = database.as_os_str().to_os_string();
        path.push(suffix);
        match fs::remove_file(PathBuf::from(path)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove SQLite sidecar: {error}")),
        }
    }
    Ok(())
}

fn write_manifest(root: &Path, source: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join(MANIFEST_FILE))
        .map_err(|e| e.to_string())?;
    writeln!(file, "format=picto-library-schema-1")
        .and_then(|_| writeln!(file, "source={}", source.display()))
        .and_then(|_| writeln!(file, "source_schema=128"))
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

fn remove_conversion_directory(path: &Path) -> Result<(), String> {
    let root = library_root(path)?;
    if !root.join(MANIFEST_FILE).is_file() {
        return Err(format!(
            "refusing to delete {} without converter manifest",
            root.display()
        ));
    }
    fs::remove_dir_all(&root).map_err(|e| e.to_string())
}

fn parse_lifecycle(value: &str) -> Result<Lifecycle, String> {
    match value {
        "active" => Ok(Lifecycle::Active),
        "inbox" => Ok(Lifecycle::Inbox),
        "trash" => Ok(Lifecycle::Trash),
        other => Err(format!("unknown lifecycle {other}")),
    }
}
fn parse_rating(value: Option<i64>) -> Rating {
    match value.unwrap_or(0) {
        1 => Rating::One,
        2 => Rating::Two,
        3 => Rating::Three,
        4 => Rating::Four,
        5 => Rating::Five,
        _ => Rating::Unrated,
    }
}
fn parse_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp_millis())
        .or_else(|| value.parse().ok())
}
fn tag_name(namespace: &str, subname: &str) -> String {
    let namespace = namespace.trim();
    if namespace.is_empty() || namespace == "general" || namespace == "default" {
        subname.trim().to_owned()
    } else {
        format!("{namespace}:{}", subname.trim())
    }
}
fn split_tag(name: &str) -> (&str, &str) {
    name.split_once(':').unwrap_or(("", name))
}
fn decode_palette(blob: Option<&[u8]>) -> Vec<LabColor> {
    let colors = blob
        .and_then(|blob| deserialize_dominant_palette_blob(blob).ok())
        .unwrap_or_default();
    let weight = if colors.is_empty() {
        0.0
    } else {
        1.0 / colors.len() as f32
    };
    colors
        .into_iter()
        .map(|color| LabColor {
            l: color.l as f32,
            a: color.a as f32,
            b: color.b as f32,
            weight,
        })
        .collect()
}
