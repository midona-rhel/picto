//! This benchmark measures what cloud sync adds to the cost of an import. The
//! same items are imported into a library that has never synced and into one
//! that records its imports, and the entries of the second library are then
//! merged into a third library and imported there.

use std::time::Instant;

use picto_library::database::WorkPriority;
use picto_library::{Library, SharedEntry, SharedImport};

use super::{prepared, R};

/// The number of entries that are merged in one call, which stands for one
/// entry file.
const FILE_ENTRIES: usize = 4_000;

/// Creates a library in `directory`. With `syncing` it records the items
/// that it imports, as a library does that was connected to a cloud folder.
fn library(directory: &std::path::Path, name: &str, syncing: bool) -> R<Library> {
    let library = Library::create(directory.join(name))?;
    if syncing {
        library
            .database()
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                picto_library::reset_published_entries(transaction)
            })?;
    }
    Ok(library)
}

/// Imports `roots` items in batches of the size that the ingest worker uses
/// and returns the seconds it took.
fn import(library: &Library, roots: usize) -> R<f64> {
    let started = Instant::now();
    for start in (0..roots).step_by(picto_library::ingest::MAX_INGEST_BATCH) {
        let end = (start + picto_library::ingest::MAX_INGEST_BATCH).min(roots);
        library.ingest_batch(&(start..end).map(prepared).collect::<Vec<_>>())?;
    }
    Ok(started.elapsed().as_secs_f64())
}

/// The size of the library's database file after its log was written into
/// it.
fn database_bytes(library: &Library, path: &std::path::Path) -> R<u64> {
    library.database().checkpoint_wal()?;
    Ok(std::fs::metadata(path)?.len())
}

pub fn run(roots: usize) -> R<()> {
    let directory = tempfile::tempdir()?;
    let plain = library(directory.path(), "plain.sqlite", false)?;
    let plain_seconds = import(&plain, roots)?;
    let plain_bytes = database_bytes(&plain, &directory.path().join("plain.sqlite"))?;
    println!("ROW|import, sync off|{roots}|{plain_seconds:.2} s|{plain_bytes} bytes of database");

    let syncing = library(directory.path(), "syncing.sqlite", true)?;
    let syncing_seconds = import(&syncing, roots)?;
    let syncing_bytes = database_bytes(&syncing, &directory.path().join("syncing.sqlite"))?;
    println!(
        "ROW|import, sync on|{roots}|{syncing_seconds:.2} s|{syncing_bytes} bytes of database"
    );
    println!(
        "ROW|cost of sync per item|{roots}|{:.1} microseconds|{:.0} bytes of database",
        (syncing_seconds - plain_seconds) * 1e6 / roots as f64,
        (syncing_bytes as f64 - plain_bytes as f64) / roots as f64,
    );

    // This measures what a sync reads when the device has made no entry since
    // its last sync. The sync reads the device's entries that are later than
    // the library's clock.
    let clock = syncing.entry_clock()?;
    let started = Instant::now();
    let mut later = 0;
    syncing.visit_own_entries(Some(clock.now), &mut |_| later += 1)?;
    println!(
        "ROW|look for new own entries|{roots}|{:.3} s|{later} found",
        started.elapsed().as_secs_f64()
    );

    let started = Instant::now();
    let mut entries = Vec::<SharedEntry>::with_capacity(roots);
    syncing.visit_own_entries(None, &mut |entry| entries.push(entry))?;
    println!(
        "ROW|read own entries|{}|{:.2} s|",
        entries.len(),
        started.elapsed().as_secs_f64()
    );
    let started = Instant::now();
    syncing.attach_values(&mut entries)?;
    let read_seconds = started.elapsed().as_secs_f64();
    if let Ok(path) = std::env::var("PICTO_BENCHMARK_ENTRIES") {
        // The entries are written to the file that PICTO_BENCHMARK_ENTRIES
        // names, as the lines of the cloud files, so that they can be
        // compressed and measured outside this program.
        let mut lines = Vec::new();
        for entry in &entries {
            serde_json::to_writer(&mut lines, entry)?;
            lines.push(b'\n');
        }
        std::fs::write(path, lines)?;
    }
    let json_bytes = entries
        .iter()
        .map(|entry| serde_json::to_vec(entry).map(|line| line.len() + 1))
        .sum::<Result<usize, _>>()?;
    println!(
        "ROW|make the records of own entries|{}|{read_seconds:.2} s|{:.0} bytes of JSON per entry",
        entries.len(),
        json_bytes as f64 / entries.len().max(1) as f64,
    );

    let receiver = library(directory.path(), "receiver.sqlite", true)?;
    let started = Instant::now();
    let mut queued = 0;
    for file in entries.chunks(FILE_ENTRIES) {
        queued += receiver.apply_shared_entries("benchmark-device", file)?.queued;
    }
    println!(
        "ROW|merge entries|{}|{:.2} s|{queued} items queued",
        entries.len(),
        started.elapsed().as_secs_f64()
    );

    let started = Instant::now();
    let mut imported = 0;
    loop {
        let awaited = receiver.awaited_shared_imports(2_000, "9999-01-01T00:00:00Z")?;
        if awaited.is_empty() {
            break;
        }
        let jobs = awaited.iter().map(|item| item.ingest_job_id).collect::<Vec<_>>();
        let imports = awaited
            .into_iter()
            .map(|item| SharedImport {
                paths: item
                    .import
                    .record
                    .files
                    .iter()
                    .map(|file| format!("/fixture/{}", file.facts.content_hash))
                    .collect(),
                ..item.import
            })
            .collect::<Vec<_>>();
        for batch in imports.chunks(picto_library::ingest::MAX_INGEST_BATCH) {
            imported += receiver.ingest_shared_items(batch)?.len();
        }
        receiver
            .database()
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                transaction.execute(
                    "DELETE FROM ingest_job WHERE ingest_job_id IN (SELECT value FROM json_each(?1))",
                    [serde_json::to_string(&jobs)?],
                )?;
                Ok(())
            })?;
    }
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "ROW|import merged items|{imported}|{seconds:.2} s|{:.1} microseconds per item",
        seconds * 1e6 / imported.max(1) as f64
    );
    Ok(())
}

/// Measures what one tag on every item of a library of `roots` items costs
/// with cloud sync on: the change itself, turning its journal row into
/// entries, the rows in the database, the entries as the cloud files hold
/// them, and merging them in a copy of the library.
pub fn run_tag(roots: usize) -> R<()> {
    let directory = tempfile::tempdir()?;
    // The items are imported before the library syncs, so that no entry is
    // written for them.
    let path = directory.path().join("tagged.sqlite");
    let tagged = library(directory.path(), "tagged.sqlite", false)?;
    let mut ids = Vec::with_capacity(roots);
    for start in (0..roots).step_by(picto_library::ingest::MAX_INGEST_BATCH) {
        let end = (start + picto_library::ingest::MAX_INGEST_BATCH).min(roots);
        let batch = (start..end).map(prepared).collect::<Vec<_>>();
        ids.extend(tagged.ingest_batch(&batch)?.into_iter().map(|(id, _)| id));
    }
    let before_bytes = database_bytes(&tagged, &path)?;
    // The other device is a copy of the library as it is now.
    drop(tagged);
    std::fs::copy(&path, directory.path().join("other.sqlite"))?;
    let tagged = Library::open(&path)?;
    let other = Library::open(directory.path().join("other.sqlite"))?;
    for library in [&tagged, &other] {
        library
            .database()
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                picto_library::reset_published_entries(transaction)
            })?;
    }

    let target = picto_library::selection::SelectionTarget::Explicit { root_ids: ids };
    let started = Instant::now();
    tagged.apply_tags(&target, &["bulk:tag".to_owned()], true)?;
    println!(
        "ROW|tag every item, sync on|{roots}|{:.3} s|one journal row",
        started.elapsed().as_secs_f64()
    );

    let started = Instant::now();
    while tagged.settle_journal(20_000)? {}
    let settle_seconds = started.elapsed().as_secs_f64();
    let after_bytes = database_bytes(&tagged, &path)?;
    println!(
        "ROW|turn the row into entries|{roots}|{settle_seconds:.2} s|{:.0} bytes of database per entry",
        (after_bytes as f64 - before_bytes as f64) / roots as f64
    );

    let started = Instant::now();
    let mut entries = Vec::<SharedEntry>::with_capacity(roots);
    tagged.visit_own_entries(None, &mut |entry| entries.push(entry))?;
    tagged.attach_values(&mut entries)?;
    let json_bytes = entries
        .iter()
        .map(|entry| serde_json::to_vec(entry).map(|line| line.len() + 1))
        .sum::<Result<usize, _>>()?;
    println!(
        "ROW|read the entries with their values|{}|{:.2} s|{:.0} bytes of JSON per entry",
        entries.len(),
        started.elapsed().as_secs_f64(),
        json_bytes as f64 / entries.len().max(1) as f64,
    );
    if let Ok(path) = std::env::var("PICTO_BENCHMARK_ENTRIES") {
        // The entries are written to the file that PICTO_BENCHMARK_ENTRIES
        // names, as the lines of the cloud files, so that they can be
        // compressed and measured outside this program.
        let mut lines = Vec::new();
        for entry in &entries {
            serde_json::to_writer(&mut lines, entry)?;
            lines.push(b'\n');
        }
        std::fs::write(path, lines)?;
    }

    let started = Instant::now();
    let mut edited = 0;
    for file in entries.chunks(FILE_ENTRIES) {
        edited += other.apply_shared_entries("benchmark-device", file)?.edited;
    }
    println!(
        "ROW|merge the entries on another device|{}|{:.2} s|{edited} changes of a tag",
        entries.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
