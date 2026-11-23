//! The rows of `--sync-only`: what recording cloud sync entries costs.
//!
//! - The start of recording on the whole library: the call itself, then
//!   writing the `add` entries in batches. The longest batch is how long the
//!   library waits at most while that runs.
//! - One tag on every item: the action, then turning it into entries.
//! - An import of 100,000 items into an empty library, with recording off and
//!   with it on.
//! - A merge of the 100,000 `add` entries of that import, into a library that
//!   holds the same files and into an empty one, and the same merge when the
//!   entries arrive as 16 files, the way a sync reads them.
//! - Deleting the 100,000 imported items permanently, which first turns the
//!   items' pending changes into entries, and compacting their entries as
//!   if 8 days had passed.

use std::time::Instant;

use picto_library::database::WorkPriority;
use picto_library::{Library, RootId};

use super::{explicit, ms, prepared, R};

/// The number of roots that one batch turns into entries.
const BATCH: usize = 20_000;
/// The size of the import and of the merge.
const IMPORT: usize = 100_000;

pub fn run(library: Library, ids: &[RootId], directory: &std::path::Path) -> R<()> {
    let roots = ids.len();

    let started = Instant::now();
    library.start_recording()?;
    println!("ROW|start recording (call)|{roots}|{:.1}|-|n=1", ms(started));
    let (total, longest, batches) = settle(&library)?;
    println!("ROW|start recording (add entries, all batches)|{roots}|{total:.1}|-|n=1 batches={batches} longest_batch_ms={longest:.1}");
    println!("BYTES|add entry|{:.1}", bytes_per_entry(&library, "add")?);

    let started = Instant::now();
    library.add_tag(&explicit(ids), "synced:everything")?;
    println!("ROW|tag every item (action)|{roots}|{:.1}|-|n=1", ms(started));
    let (total, longest, batches) = settle(&library)?;
    println!("ROW|tag every item (entries, all batches)|{roots}|{total:.1}|-|n=1 batches={batches} longest_batch_ms={longest:.1}");
    println!("BYTES|item_tag entry|{:.1}", bytes_per_entry(&library, "item_tag")?);
    drop(library);

    let off = Library::create(directory.join("import-off.sqlite"))?;
    let started = Instant::now();
    import(&off)?;
    println!("ROW|import, recording off|{IMPORT}|{:.1}|-|n=1", ms(started));

    let on = Library::create(directory.join("import-on.sqlite"))?;
    on.start_recording()?;
    let started = Instant::now();
    import(&on)?;
    println!("ROW|import, recording on (action)|{IMPORT}|{:.1}|-|n=1", ms(started));
    let (total, longest, batches) = settle(&on)?;
    println!("ROW|import, recording on (entries, all batches)|{IMPORT}|{total:.1}|-|n=1 batches={batches} longest_batch_ms={longest:.1}");

    let mut entries = Vec::new();
    on.visit_own_entries(None, &mut |entry| entries.push(entry))?;
    let adds = entries.len();

    off.start_recording()?;
    settle(&off)?;
    let started = Instant::now();
    off.merge_entries(&entries)?;
    println!("ROW|merge add entries into a library with the same files|{adds}|{:.1}|-|n=1", ms(started));

    let split = Library::create(directory.join("merge-split.sqlite"))?;
    import(&split)?;
    split.start_recording()?;
    settle(&split)?;
    let started = Instant::now();
    for file in entries.chunks(adds.div_ceil(16)) {
        split.store_entries(file)?;
    }
    split.apply_stored(false)?;
    println!("ROW|merge add entries as 16 files into a library with the same files|{adds}|{:.1}|-|n=1", ms(started));

    let empty = Library::create(directory.join("merge-empty.sqlite"))?;
    let started = Instant::now();
    empty.merge_entries(&entries)?;
    let wanted = empty.wanted_files(None, IMPORT + 1)?.len();
    println!("ROW|merge add entries into an empty library|{adds}|{:.1}|-|n=1 wanted={wanted}", ms(started));

    let imported = on.auxiliary_read(WorkPriority::Maintenance, |connection| {
        let mut statement = connection.prepare("SELECT root_id FROM library_root")?;
        let rows = statement.query_map([], |row| row.get::<_, u32>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    let imported = imported.into_iter().map(RootId).collect::<Vec<_>>();
    on.add_tag(&explicit(&imported), "synced:before-deletion")?;
    on.set_lifecycle(&explicit(&imported), picto_library::Lifecycle::Trash)?;
    let started = Instant::now();
    on.permanently_delete(&explicit(&imported), 0, 0, false)?;
    println!("ROW|delete permanently with pending tag and status changes|{IMPORT}|{:.1}|-|n=1", ms(started));
    on.set_entries_published(on.entry_clock()?.now)?;
    let before = entry_count(&on)?;
    let eight_days_later = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64
        + 8 * 24 * 60 * 60 * 1_000;
    let started = Instant::now();
    on.compact_entries(eight_days_later)?;
    println!(
        "ROW|compact the entries of the deleted items|{IMPORT}|{:.1}|-|n=1 entries_before={before} entries_after={}",
        ms(started),
        entry_count(&on)?
    );
    Ok(())
}

fn entry_count(library: &Library) -> R<i64> {
    Ok(library.auxiliary_read(WorkPriority::Maintenance, |connection| {
        Ok(connection.query_row("SELECT COUNT(*) FROM shared_entry", [], |row| row.get(0))?)
    })?)
}

fn import(library: &Library) -> R<()> {
    for start in (0..IMPORT).step_by(picto_library::ingest::MAX_INGEST_BATCH) {
        let end = (start + picto_library::ingest::MAX_INGEST_BATCH).min(IMPORT);
        library.ingest_batch(&(start..end).map(prepared).collect::<Vec<_>>())?;
    }
    Ok(())
}

/// Turns the pending changes into entries and returns the total time, the
/// longest batch, both in milliseconds, and the number of batches.
fn settle(library: &Library) -> R<(f64, f64, usize)> {
    let started = Instant::now();
    let mut longest = 0.0_f64;
    let mut batches = 0;
    loop {
        let batch = Instant::now();
        let more = library.settle_entries(BATCH)?;
        longest = longest.max(ms(batch));
        batches += 1;
        if !more {
            break;
        }
    }
    Ok((ms(started), longest, batches))
}

/// The average size of the stored entries of `kind`: the text of all columns
/// and 16 bytes for the two clock numbers.
fn bytes_per_entry(library: &Library, kind: &str) -> R<f64> {
    Ok(library.auxiliary_read(WorkPriority::Maintenance, |connection| {
        Ok(connection.query_row(
            "SELECT AVG(length(subject_key) + length(kind) + length(second_key)
                        + length(COALESCE(value, '')) + length(device_id) + 16)
             FROM shared_entry WHERE kind = ?1",
            [kind],
            |row| row.get::<_, f64>(0),
        )?)
    })?)
}
