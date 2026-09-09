//! Resident drawing records: every fact a grid tile needs besides its name,
//! which the alphabetical index already holds. Built from one canonical scan
//! when the library opens, then rebuilt one 1,024-root shard at a time when the
//! dirty-root journal says a shard changed. SQLite triggers write that journal;
//! the publication owner drains it inside the same transaction that publishes
//! the next snapshot, so readers never see a record from another revision.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{params, Connection};

use crate::{LibraryError, Result};

const SHARD_SHIFT: u32 = 10;
const SHARD_MASK: u32 = (1 << SHARD_SHIFT) - 1;

#[derive(Clone, Copy, Debug, Default)]
struct TextRef {
    start: u32,
    len: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct DrawingRow {
    pub id: u32,
    hash: TextRef,
    mime: TextRef,
    pub cover: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<i64>,
    pub frames: Option<u32>,
    pub imported: i64,
    pub captured: Option<i64>,
    pub modified: i64,
    pub count: u32,
    pub bytes: u64,
}

/// One root's drawing facts with its strings borrowed from the shard arena.
pub(crate) struct DrawingRecord<'a> {
    pub row: &'a DrawingRow,
    pub content_hash: &'a str,
    pub mime: &'a str,
}

#[derive(Debug, Default)]
struct DrawingShard {
    text: String,
    rows: Vec<DrawingRow>,
}

impl DrawingShard {
    fn row(&self, id: u32) -> Option<&DrawingRow> {
        self.rows
            .binary_search_by_key(&id, |row| row.id)
            .ok()
            .map(|index| &self.rows[index])
    }

    fn text(&self, value: TextRef) -> &str {
        &self.text[value.start as usize..value.start as usize + value.len as usize]
    }

    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.text.capacity()
            + self.rows.capacity() * std::mem::size_of::<DrawingRow>()
    }

    fn append(&mut self, text: &str) -> Result<TextRef> {
        let start = u32::try_from(self.text.len())
            .map_err(|_| LibraryError::InvalidState("drawing shard text too large".into()))?;
        let len = u32::try_from(text.len())
            .map_err(|_| LibraryError::InvalidState("drawing string too large".into()))?;
        self.text.push_str(text);
        Ok(TextRef { start, len })
    }
}

/// Streams canonical rows, already in ascending ID order, into one shard.
/// Content hashes are unique and copied once; the few MIME values of a shard
/// are shared by comparing against the ones already appended.
#[derive(Default)]
struct ShardBuilder {
    shard: DrawingShard,
    mimes: Vec<TextRef>,
}

impl ShardBuilder {
    fn push(&mut self, row: &rusqlite::Row<'_>) -> Result<()> {
        let text = |column: usize| -> rusqlite::Result<&str> {
            row.get_ref(column)?.as_str().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        };
        let hash = self.shard.append(text(1)?)?;
        let mime_text = text(2)?;
        let mime = match self
            .mimes
            .iter()
            .find(|value| self.shard.text(**value) == mime_text)
        {
            Some(value) => *value,
            None => {
                let value = self.shard.append(mime_text)?;
                self.mimes.push(value);
                value
            }
        };
        self.shard.rows.push(DrawingRow {
            id: row.get(0)?,
            hash,
            mime,
            cover: row.get(3)?,
            width: row.get(4)?,
            height: row.get(5)?,
            duration: row.get(6)?,
            frames: row.get(7)?,
            imported: row.get(8)?,
            captured: row.get(9)?,
            modified: row.get(10)?,
            count: row.get(11)?,
            bytes: row.get::<_, i64>(12)? as u64,
        });
        Ok(())
    }

    fn finish(&mut self) -> DrawingShard {
        let mut shard = std::mem::take(&mut self.shard);
        self.mimes.clear();
        shard.text.shrink_to_fit();
        shard.rows.shrink_to_fit();
        shard
    }
}

const CANONICAL_COLUMNS: &str = "root.root_id, file.content_hash, file.mime, root.cover_media_id,
        file.width, file.height, file.duration_ms, file.frame_count,
        root.imported_at_ms, root.captured_at_ms, root.modified_at_ms,
        root.media_count, root.total_size_bytes
        FROM library_root root
        JOIN media_item media ON media.media_id = root.cover_media_id
        JOIN media_file file ON file.file_id = media.file_id";

/// The resident drawing model of one snapshot. Shards are immutable and
/// shared between snapshots; a change replaces only the shards it touches.
#[derive(Debug, Default, Clone)]
pub struct DrawingIndex {
    shards: BTreeMap<u32, Arc<DrawingShard>>,
    bytes: usize,
    rows: u64,
}

impl DrawingIndex {
    /// One streaming scan of the canonical cover join in ID order.
    pub(crate) fn load(connection: &Connection) -> Result<Self> {
        let mut statement =
            connection.prepare(&format!("SELECT {CANONICAL_COLUMNS} ORDER BY root.root_id"))?;
        let mut rows = statement.query([])?;
        let mut index = Self::default();
        let mut key = None;
        let mut builder = ShardBuilder::default();
        while let Some(row) = rows.next()? {
            let id: u32 = row.get(0)?;
            let next_key = id >> SHARD_SHIFT;
            if key.is_some_and(|key| key != next_key) {
                crate::query_request::check()?;
                index.install(key.unwrap(), builder.finish());
            }
            key = Some(next_key);
            builder.push(row)?;
        }
        if let Some(key) = key {
            index.install(key, builder.finish());
        }
        Ok(index)
    }

    fn install(&mut self, key: u32, shard: DrawingShard) {
        if let Some(previous) = self.shards.remove(&key) {
            self.bytes -= previous.bytes();
            self.rows -= previous.rows.len() as u64;
        }
        if !shard.rows.is_empty() {
            self.bytes += shard.bytes();
            self.rows += shard.rows.len() as u64;
            self.shards.insert(key, Arc::new(shard));
        }
    }

    pub(crate) fn record(&self, id: u32) -> Option<DrawingRecord<'_>> {
        let shard = self.shards.get(&(id >> SHARD_SHIFT))?;
        let row = shard.row(id)?;
        Some(DrawingRecord {
            row,
            content_hash: shard.text(row.hash),
            mime: shard.text(row.mime),
        })
    }

    pub fn len(&self) -> u64 {
        self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }

    pub fn estimated_bytes(&self) -> usize {
        self.bytes + self.shards.len() * 64
    }

    /// Rebuild every shard holding one of `roots` from canonical rows.
    pub(crate) fn refresh(&mut self, connection: &Connection, roots: &RoaringBitmap) -> Result<()> {
        let keys = roots
            .iter()
            .map(|id| id >> SHARD_SHIFT)
            .collect::<BTreeSet<_>>();
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {CANONICAL_COLUMNS} WHERE root.root_id BETWEEN ?1 AND ?2 ORDER BY root.root_id"
        ))?;
        let mut builder = ShardBuilder::default();
        for key in keys {
            crate::query_request::check()?;
            let first = key << SHARD_SHIFT;
            let mut rows = statement.query(params![first, first | SHARD_MASK])?;
            while let Some(row) = rows.next()? {
                builder.push(row)?;
            }
            self.install(key, builder.finish());
        }
        Ok(())
    }

    /// Drain the dirty-root journal written by the canonical triggers and
    /// rebuild the affected shards. Called by the publication owner inside the
    /// transaction whose snapshot this index belongs to. Returns the roots.
    pub(crate) fn settle(&mut self, connection: &Connection) -> Result<RoaringBitmap> {
        let roots = connection
            .prepare_cached("SELECT root_id FROM browsing_dirty_root")?
            .query_map([], |row| row.get::<_, u32>(0))?
            .collect::<rusqlite::Result<RoaringBitmap>>()?;
        if roots.is_empty() {
            return Ok(roots);
        }
        self.refresh(connection, &roots)?;
        connection.execute("DELETE FROM browsing_dirty_root", [])?;
        Ok(roots)
    }

    /// True when a transaction left drawing facts unsettled.
    pub(crate) fn journal_is_dirty(connection: &Connection) -> Result<bool> {
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM browsing_dirty_root)",
            [],
            |row| row.get::<_, bool>(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database::WorkPriority, LibraryDatabase};

    fn insert(connection: &Connection, id: u32) {
        connection
            .execute(
                "INSERT INTO library_item VALUES (?1,?2,1)",
                params![id, format!("item-{id}")],
            )
            .unwrap();
        connection.execute("INSERT INTO media_file(file_id,content_hash,file_path,mime,size_bytes,width,height) VALUES (?1,?2,'synthetic','image/png',12,800,600)", params![id,format!("hash-{id}")]).unwrap();
        connection
            .execute("INSERT INTO media_item VALUES (?1,'item',NULL,?1)", [id])
            .unwrap();
        connection.execute("INSERT INTO library_root(root_id,name,cover_media_id,imported_at_ms,captured_at_ms,modified_at_ms,media_count,total_size_bytes) VALUES (?1,'猫 🐈',?1,-10,NULL,0,1,12)", [id]).unwrap();
    }

    fn dirty(connection: &Connection) -> u32 {
        connection
            .query_row("SELECT COUNT(*) FROM browsing_dirty_root", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn load_packs_sparse_roots_into_shared_shards() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        let db = LibraryDatabase::create(&path).unwrap();
        let ids = [1, 17, 1023, 1024, 4_000_000];
        db.write(WorkPriority::VisibleRead, |tx, _| {
            for id in ids {
                insert(tx, id);
            }
            Ok(())
        })
        .unwrap();
        let connection = Connection::open(&path).unwrap();
        let index = DrawingIndex::load(&connection).unwrap();
        assert_eq!(index.len(), 5);
        assert_eq!(index.shards.len(), 3);
        for id in ids {
            let record = index.record(id).unwrap();
            assert_eq!(record.content_hash, format!("hash-{id}"));
            assert_eq!(record.mime, "image/png");
            assert_eq!(record.row.width, Some(800));
            assert_eq!(record.row.cover, id);
            assert_eq!(record.row.bytes, 12);
        }
        assert!(index.record(2).is_none());
        assert!(index.record(5_000_000).is_none());
        // The shared MIME string is stored once per shard.
        assert_eq!(index.shards[&0].text.matches("image/png").count(), 1);
    }

    #[test]
    fn settlement_rebuilds_only_dirty_shards_and_keeps_old_readers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        let db = LibraryDatabase::create(&path).unwrap();
        db.write(WorkPriority::VisibleRead, |tx, _| {
            insert(tx, 1);
            insert(tx, 17);
            insert(tx, 2049);
            Ok(())
        })
        .unwrap();
        let connection = Connection::open(&path).unwrap();
        let mut index = DrawingIndex::load(&connection).unwrap();
        // Triggers journal the inserts; a load without settlement leaves them.
        assert_eq!(dirty(&connection), 3);
        let settled = index.settle(&connection).unwrap();
        assert_eq!(settled.len(), 3);
        assert_eq!(dirty(&connection), 0);
        let before = index.clone();
        let untouched = Arc::clone(&index.shards[&2]);
        db.write(WorkPriority::VisibleRead, |tx, _| {
            tx.execute(
                "UPDATE media_file SET width=1920, content_hash='repaired' WHERE file_id=1",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(dirty(&connection), 1);
        let settled = index.settle(&connection).unwrap();
        assert_eq!(settled.iter().collect::<Vec<_>>(), vec![1]);
        assert_eq!(dirty(&connection), 0);
        let record = index.record(1).unwrap();
        assert_eq!(record.content_hash, "repaired");
        assert_eq!(record.row.width, Some(1920));
        // The earlier snapshot still reads its own record.
        assert_eq!(before.record(1).unwrap().content_hash, "hash-1");
        // Shards without dirty roots keep their identity across the rebuild.
        assert!(Arc::ptr_eq(&untouched, &index.shards[&2]));
        assert!(!Arc::ptr_eq(&before.shards[&0], &index.shards[&0]));
        // Neighbours in a rebuilt shard are re-read, not lost.
        assert_eq!(index.record(17).unwrap().content_hash, "hash-17");
    }

    #[test]
    fn deletion_settles_through_the_journal_without_touching_other_shards() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        let db = LibraryDatabase::create(&path).unwrap();
        db.write(WorkPriority::VisibleRead, |tx, _| {
            insert(tx, 1);
            insert(tx, 2);
            insert(tx, 5000);
            Ok(())
        })
        .unwrap();
        let connection = Connection::open(&path).unwrap();
        let mut index = DrawingIndex::load(&connection).unwrap();
        index.settle(&connection).unwrap();
        let far = Arc::clone(&index.shards[&(5000 >> SHARD_SHIFT)]);
        db.write(WorkPriority::VisibleRead, |tx, _| {
            tx.execute("DELETE FROM library_root WHERE root_id IN (1, 2)", [])?;
            Ok(())
        })
        .unwrap();
        let settled = index.settle(&connection).unwrap();
        assert_eq!(settled.iter().collect::<Vec<_>>(), vec![1, 2]);
        assert!(index.record(1).is_none());
        assert!(index.record(2).is_none());
        assert_eq!(index.len(), 1);
        assert!(!index.shards.contains_key(&0));
        assert!(Arc::ptr_eq(&far, &index.shards[&(5000 >> SHARD_SHIFT)]));
        assert_eq!(
            index.estimated_bytes(),
            index.shards[&(5000 >> SHARD_SHIFT)].bytes() + 64
        );
    }

    #[test]
    fn a_failed_publication_rolls_its_journal_back_with_its_canonical_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        let db = LibraryDatabase::create(&path).unwrap();
        let before = db.revision().unwrap();
        let result = db.published_write(
            WorkPriority::ForegroundMutation,
            |_| Ok(()),
            |tx, _, _, ()| {
                insert(tx, 4);
                assert!(DrawingIndex::journal_is_dirty(tx).unwrap());
                Err::<((), ()), _>(crate::LibraryError::InvalidState("rejected".into()))
            },
            |_, ()| unreachable!("a failed transaction publishes nothing"),
        );
        assert!(matches!(result, Err(crate::LibraryError::InvalidState(_))));
        assert_eq!(db.revision().unwrap(), before);
        let connection = Connection::open(&path).unwrap();
        assert!(!DrawingIndex::journal_is_dirty(&connection).unwrap());
        assert!(DrawingIndex::load(&connection).unwrap().is_empty());
    }

    #[test]
    fn maintenance_writes_that_change_browsing_facts_are_rejected_and_rolled_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        let db = LibraryDatabase::create(&path).unwrap();
        let result = db.maintenance_write(WorkPriority::Maintenance, |tx| {
            insert(tx, 3);
            Ok(())
        });
        assert!(matches!(result, Err(crate::LibraryError::InvalidState(_))));
        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM library_root", [], |row| row
                    .get::<_, u32>(0))
                .unwrap(),
            0
        );
        assert!(!DrawingIndex::journal_is_dirty(&connection).unwrap());
        // Writes that leave browsing facts alone still commit.
        db.maintenance_write(WorkPriority::Maintenance, |tx| {
            tx.execute(
                "INSERT INTO setting (key, value_json) VALUES ('kept', 'true')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_dirty_journal_is_reported_and_an_empty_one_settles_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        let db = LibraryDatabase::create(&path).unwrap();
        let connection = Connection::open(&path).unwrap();
        assert!(!DrawingIndex::journal_is_dirty(&connection).unwrap());
        let mut index = DrawingIndex::load(&connection).unwrap();
        assert!(index.is_empty());
        assert!(index.settle(&connection).unwrap().is_empty());
        db.write(WorkPriority::VisibleRead, |tx, _| {
            insert(tx, 9);
            Ok(())
        })
        .unwrap();
        assert!(DrawingIndex::journal_is_dirty(&connection).unwrap());
        index.settle(&connection).unwrap();
        assert_eq!(index.record(9).unwrap().row.count, 1);
    }
}
