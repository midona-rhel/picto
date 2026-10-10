//! Resident settled search text for every root. Loaded from the settled FTS
//! rows when the library opens and rebuilt one 1,024-root shard at a time when
//! FTS settlement or a deletion changes a root. There is no admission budget,
//! no persisted copy and no SQL fallback: one matching contract serves
//! interactive filters and smart folders alike.
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use memchr::memmem::Finder;
use parking_lot::Mutex;
use rayon::prelude::*;
use roaring::RoaringBitmap;
use rusqlite::Connection;

use crate::predicate::TextField;
use crate::Result;

const SHARD_SHIFT: u32 = 10;
const SHARD_MASK: u32 = (1 << SHARD_SHIFT) - 1;
/// Selections up to this size look each root up directly instead of scanning.
const DIRECT_LOOKUP_MAXIMUM: u64 = 4096;
static SEARCH_POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
/// Distinguishes text index contents: every mutation takes a fresh value, so a
/// remembered search can only be reused against the exact text it ran on.
static GENERATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// Recent searches, so typing a longer query narrows the previous result
/// instead of scanning every root again.
static RECENT: Mutex<Vec<Recent>> = Mutex::new(Vec::new());
const RECENT_SEARCHES: usize = 8;

struct Recent {
    generation: u64,
    field: TextField,
    query: String,
    universe: RoaringBitmap,
    result: RoaringBitmap,
}

#[derive(Debug, Clone)]
struct TextRow {
    id: u32,
    bounds: [u32; 5],
}

#[derive(Debug, Default)]
struct TextShard {
    text: String,
    rows: Vec<TextRow>,
}

fn fields_of(field: TextField) -> Range<usize> {
    match field {
        TextField::Global => 0..4,
        TextField::Name => 0..1,
        TextField::Notes => 1..2,
        TextField::SourceUrl => 2..3,
    }
}

// The matching contract for one settled field value: the lowercased,
// trimmed literal is a byte substring of the lowercased text. Every byte is
// ordinary, including an embedded NUL, so text after a NUL stays searchable
// and a query containing one matches exactly the fields containing that
// byte sequence. Alphabetical order deliberately differs: it mirrors
// SQLite's NOCASE collation, which stops at a NUL.

impl TextShard {
    fn bytes(&self) -> usize {
        self.text.capacity()
            + self.rows.capacity() * std::mem::size_of::<TextRow>()
            + std::mem::size_of::<Self>()
    }

    fn push(&mut self, row: &rusqlite::Row<'_>) -> Result<()> {
        let field = |column| -> rusqlite::Result<&str> {
            row.get_ref(column)?.as_str().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        };
        let fields = [field(1)?, field(2)?, field(3)?, field(4)?];
        self.text
            .reserve_exact(fields.iter().map(|value| value.len()).sum());
        let mut bounds = [0; 5];
        bounds[0] = self.text.len() as u32;
        for (index, value) in fields.into_iter().enumerate() {
            self.text.push_str(value);
            bounds[index + 1] = self.text.len() as u32;
        }
        self.rows.push(TextRow {
            id: row.get(0)?,
            bounds,
        });
        Ok(())
    }

    fn field(&self, row: &TextRow, index: usize) -> &str {
        &self.text[row.bounds[index] as usize..row.bounds[index + 1] as usize]
    }

    fn row(&self, id: u32) -> Option<&TextRow> {
        self.rows
            .binary_search_by_key(&id, |row| row.id)
            .ok()
            .map(|index| &self.rows[index])
    }

    fn matches(&self, row: &TextRow, fields: Range<usize>, finder: &Finder<'_>) -> bool {
        fields
            .into_iter()
            .any(|index| finder.find(self.field(row, index).as_bytes()).is_some())
    }

    /// Roots of this shard in `universe` whose `fields` contain the query.
    /// A sparse universe looks its rows up; a dense one scans the shard's
    /// text once with a vectorized substring search and maps each hit back to
    /// its row, so rows without the query cost nothing per row.
    fn search(
        &self,
        fields: Range<usize>,
        finder: &Finder<'_>,
        universe: &RoaringBitmap,
        removed: &RoaringBitmap,
    ) -> RoaringBitmap {
        let mut result = RoaringBitmap::new();
        let (Some(first), Some(last)) = (self.rows.first(), self.rows.last()) else {
            return result;
        };
        let candidates = universe.range_cardinality(first.id..=last.id);
        if candidates == 0 {
            return result;
        }
        // Below ~3% of a shard, row lookups beat scanning its text.
        if candidates * 32 < self.rows.len() as u64 {
            for id in universe.range(first.id..=last.id) {
                if !removed.contains(id)
                    && self.row(id).is_some_and(|row| self.matches(row, fields.clone(), finder))
                {
                    result.insert(id);
                }
            }
            return result;
        }
        let text = self.text.as_bytes();
        let needle = finder.needle().len();
        let Some(first_in_scope) = universe.range(first.id..=last.id).next() else {
            return result;
        };
        let mut index = self.rows.partition_point(|row| row.id < first_in_scope);
        let mut from = self.rows[index].bounds[0] as usize;
        while let Some(offset) = finder.find(&text[from..]) {
            let start = from + offset;
            // Hits only move forward, so walk the row cursor instead of
            // searching for the row; a long gap falls back to a search.
            if self.rows.get(index + 16).is_some_and(|row| row.bounds[0] as usize <= start) {
                index = self.rows.partition_point(|row| row.bounds[0] as usize <= start) - 1;
            }
            while self.rows.get(index + 1).is_some_and(|row| row.bounds[0] as usize <= start) {
                index += 1;
            }
            let row = &self.rows[index];
            // Resume at the next row in scope: rows outside it, and the rest
            // of a row already decided, are never searched again.
            let next_in_scope = |index: usize| {
                // Usually the following row is in scope; otherwise jump.
                let following = self.rows.get(index + 1)?;
                if universe.contains(following.id) {
                    return Some(following.bounds[0] as usize);
                }
                universe
                    .range(following.id..=last.id)
                    .next()
                    .map(|id| self.rows.partition_point(|row| row.id < id))
                    .and_then(|index| self.rows.get(index))
                    .map(|row| row.bounds[0] as usize)
            };
            if !universe.contains(row.id) || removed.contains(row.id) {
                match next_in_scope(index) {
                    Some(next) => from = next.max(start + 1),
                    None => break,
                }
                continue;
            }
            let hit = fields.clone().any(|field| {
                let (low, high) = (row.bounds[field] as usize, row.bounds[field + 1] as usize);
                low <= start && start + needle <= high
            });
            if hit {
                result.insert(row.id);
                match next_in_scope(index) {
                    Some(next) => from = next.max(start + 1),
                    None => break,
                }
            } else {
                // A match across two fields, or in a field not searched: a
                // later one in this row may still count.
                from = start + 1;
            }
            if from >= text.len() {
                break;
            }
        }
        result
    }
}

/// Settled search text of one snapshot. Shards are immutable and shared
/// between snapshots; a refresh replaces only the shards it touches.
#[derive(Debug, Clone, Default)]
pub struct TextIndex {
    shards: BTreeMap<u32, Arc<TextShard>>,
    bytes: usize,
    removed: RoaringBitmap,
    generation: u64,
}

impl TextIndex {
    /// One streaming scan of the settled text rows in ID order.
    pub(crate) fn load(connection: &Connection) -> Result<Self> {
        let mut index = Self::default();
        let mut statement = connection
            .prepare("SELECT rowid,name,notes,urls,source_text FROM root_fts ORDER BY rowid")?;
        let mut rows = statement.query([])?;
        let mut key = None;
        let mut current = TextShard::default();
        while let Some(row) = rows.next()? {
            let id: u32 = row.get(0)?;
            let next_key = id >> SHARD_SHIFT;
            if key.is_some_and(|key| key != next_key) {
                crate::query_request::check()?;
                index.install(key.unwrap(), std::mem::take(&mut current));
            }
            key = Some(next_key);
            current.push(row)?;
        }
        if let Some(key) = key {
            index.install(key, current);
        }
        Ok(index)
    }

    fn touch(&mut self) {
        self.generation = GENERATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn install(&mut self, key: u32, mut shard: TextShard) {
        self.touch();
        if let Some(previous) = self.shards.remove(&key) {
            self.bytes -= previous.bytes() + 64;
        }
        let first = key << SHARD_SHIFT;
        self.removed.remove_range(first..=first | SHARD_MASK);
        if !shard.rows.is_empty() {
            shard.text.shrink_to_fit();
            shard.rows.shrink_to_fit();
            self.bytes += shard.bytes() + 64;
            self.shards.insert(key, Arc::new(shard));
        }
    }

    pub fn estimated_bytes(&self) -> usize {
        self.bytes + self.removed.serialized_size()
    }

    pub fn len(&self) -> u64 {
        self.shards
            .values()
            .map(|shard| shard.rows.len() as u64)
            .sum::<u64>()
            - self.removed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Mask roots immediately; their shards are rebuilt at the next refresh.
    pub(crate) fn remove(&mut self, roots: &RoaringBitmap) {
        self.touch();
        self.removed |= roots;
    }

    /// Rebuild every shard holding one of `roots` from the settled text rows.
    pub(crate) fn refresh(&mut self, connection: &Connection, roots: &RoaringBitmap) -> Result<()> {
        let keys = roots
            .iter()
            .map(|id| id >> SHARD_SHIFT)
            .collect::<BTreeSet<_>>();
        let mut statement = connection.prepare_cached(
            "SELECT rowid,name,notes,urls,source_text FROM root_fts
             WHERE rowid BETWEEN ?1 AND ?2 ORDER BY rowid",
        )?;
        for key in keys {
            crate::query_request::check()?;
            let first = key << SHARD_SHIFT;
            let mut shard = TextShard::default();
            let mut rows = statement.query([first, first | SHARD_MASK])?;
            while let Some(row) = rows.next()? {
                shard.push(row)?;
            }
            self.install(key, shard);
        }
        Ok(())
    }

    /// Literal substring search within `universe`. The query is trimmed and
    /// lowercased and must keep at least three characters.
    pub fn search(
        &self,
        field: TextField,
        query: &str,
        universe: &RoaringBitmap,
    ) -> Result<RoaringBitmap> {
        let query = query.trim();
        if query.chars().count() < crate::fts::MINIMUM_TEXT_SEARCH_CHARACTERS || universe.is_empty()
        {
            return Ok(RoaringBitmap::new());
        }
        let query = query.to_lowercase();
        let reused = self.narrowed_universe(field, &query, universe);
        let universe = reused.as_ref().unwrap_or(universe);
        let result = self.scan(field, &query, universe)?;
        // Remember only selective searches: narrowing a result that is most
        // of the scope saves nothing and copying it costs.
        if result.len() * 4 > universe.len() {
            return Ok(result);
        }
        let mut recent = RECENT.lock();
        recent.retain(|entry| entry.generation == self.generation);
        recent.push(Recent {
            generation: self.generation,
            field,
            query,
            universe: universe.clone(),
            result: result.clone(),
        });
        if recent.len() > RECENT_SEARCHES {
            recent.remove(0);
        }
        Ok(result)
    }

    /// Every match of a query that contains an earlier query (typing one more
    /// letter) is among the earlier matches, so search only those.
    fn narrowed_universe(
        &self,
        field: TextField,
        query: &str,
        universe: &RoaringBitmap,
    ) -> Option<RoaringBitmap> {
        let recent = RECENT.lock();
        recent
            .iter()
            .rev()
            .find(|entry| {
                entry.generation == self.generation
                    && entry.field == field
                    && query.contains(entry.query.as_str())
                    && universe.is_subset(&entry.universe)
            })
            .map(|entry| &entry.result & universe)
    }

    fn scan(&self, field: TextField, query: &str, universe: &RoaringBitmap) -> Result<RoaringBitmap> {
        let fields = fields_of(field);
        let finder = Finder::new(query.as_bytes());
        if universe.len() <= DIRECT_LOOKUP_MAXIMUM {
            let mut matches = RoaringBitmap::new();
            for (index, id) in universe.iter().enumerate() {
                if index % 64 == 0 {
                    crate::query_request::check()?;
                }
                if self.removed.contains(id) {
                    continue;
                }
                let Some(shard) = self.shards.get(&(id >> SHARD_SHIFT)) else {
                    continue;
                };
                if let Some(row) = shard.row(id) {
                    if shard.matches(row, fields.clone(), &finder) {
                        matches.insert(id);
                    }
                }
            }
            return Ok(matches);
        }
        let selected = self.shards.values().collect::<Vec<_>>();
        let cancellation = crate::query_request::current();
        let search = |shard: &&Arc<TextShard>| -> Result<RoaringBitmap> {
            if let Some(token) = cancellation.as_ref() {
                token.check()?;
            }
            Ok(shard.search(fields.clone(), &finder, universe, &self.removed))
        };
        if selected.len() >= 32 {
            let pool = SEARCH_POOL.get_or_init(|| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(4)
                    .thread_name(|i| format!("picto-text-{i}"))
                    .build()
                    .ok()
            });
            if let Some(pool) = pool {
                return pool.install(|| {
                    selected
                        .par_iter()
                        .map(search)
                        .try_reduce(RoaringBitmap::new, |a, b| Ok(a | b))
                });
            }
        }
        selected
            .iter()
            .map(search)
            .try_fold(RoaringBitmap::new(), |a, b| Ok(a | b?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE VIRTUAL TABLE root_fts USING fts5(name,notes,urls,source_text,tokenize='trigram',detail='none',columnsize=0);
            INSERT INTO root_fts VALUES ('alphabet 猫猫猫 i̇xyz a_b 100% quote\"','note','https://example.test','source');
            INSERT INTO root_fts VALUES ('cross','field','','');").unwrap();
        connection
    }

    fn assert_parity(connection: &Connection, index: &TextIndex) {
        let universe = (1..=100_000).collect();
        for field in [
            TextField::Global,
            TextField::Name,
            TextField::Notes,
            TextField::SourceUrl,
        ] {
            for query in [
                "ALP",
                " 猫猫猫 ",
                "İxy",
                "İx",
                "a_b",
                "100%",
                "te\"",
                "note",
                "source",
                "example",
                "ssfi",
                "absent",
                "   ",
                "new",
                "AND OR NOT",
            ] {
                assert_eq!(
                    index.search(field, query, &universe).unwrap(),
                    crate::fts::search(connection, field, query).unwrap(),
                    "{field:?}: {query:?}"
                );
            }
        }
    }

    #[test]
    fn exact_fields_and_unicode_literals_match_the_sql_oracle() {
        let connection = fixture();
        let index = TextIndex::load(&connection).unwrap();
        assert_eq!(index.len(), 2);
        assert_parity(&connection, &index);
        for field in [TextField::Global, TextField::Name, TextField::Notes] {
            for query in ["alphabet", "a_b", "100%", "İxy", "source", "absent"] {
                let candidates = [1].into_iter().collect::<RoaringBitmap>();
                assert_eq!(
                    index.search(field, query, &candidates).unwrap(),
                    crate::fts::search(&connection, field, query).unwrap() & candidates
                );
            }
        }
    }

    #[test]
    fn an_embedded_nul_is_an_ordinary_byte_in_text_and_queries() {
        let connection = fixture();
        for (id, name) in [
            (12, "before\0after"),
            (13, "axb\0a_b"),
            (14, "\0abc"),
            (15, "ab\0cd\0ef"),
            (16, "axb"),
        ] {
            connection
                .execute(
                    "INSERT INTO root_fts(rowid,name,notes,urls,source_text) VALUES (?1,?2,'','','')",
                    rusqlite::params![id, name],
                )
                .unwrap();
        }
        let index = TextIndex::load(&connection).unwrap();
        let universe = (1..=100).collect::<RoaringBitmap>();
        let expect = |query: &str, expected: &[u32]| {
            for field in [TextField::Name, TextField::Global] {
                let matches = index.search(field, query, &universe).unwrap();
                assert_eq!(
                    matches.iter().filter(|id| *id >= 12).collect::<Vec<_>>(),
                    expected,
                    "{field:?} {query:?}"
                );
                // The byte-substring oracle agrees on every literal, NUL included.
                assert_eq!(
                    crate::fts::search(&connection, field, query).unwrap() & &universe,
                    matches,
                    "oracle {field:?} {query:?}"
                );
            }
        };
        expect("before", &[12]);
        expect("after", &[12]);
        expect("efo", &[12]);
        expect("fter", &[12]);
        expect("e\0a", &[12]);
        expect("before\0", &[12]);
        expect("\0after", &[12]);
        expect("abc\0def", &[]);
        // Wildcard characters are literals; a literal after the NUL still matches.
        expect("a_b", &[13]);
        expect("abc", &[14]);
        expect("cd\0ef", &[15]);
        expect("axb", &[13, 16]);
    }

    #[test]
    fn refresh_and_removal_preserve_old_snapshots_and_untouched_shards() {
        let connection = fixture();
        connection.execute_batch("INSERT INTO root_fts(rowid,name,notes,urls,source_text) VALUES (65536,'new distant','','','')").unwrap();
        let before = TextIndex::load(&connection).unwrap();
        let mut after = before.clone();
        connection.execute_batch("DELETE FROM root_fts WHERE rowid=1; INSERT INTO root_fts(rowid,name,notes,urls,source_text) VALUES (1,'new name','','','')").unwrap();
        after
            .refresh(&connection, &[1].into_iter().collect())
            .unwrap();
        assert!(Arc::ptr_eq(&before.shards[&64], &after.shards[&64]));
        let universe = [1].into_iter().collect();
        assert_eq!(
            before
                .search(TextField::Name, "alphabet", &universe)
                .unwrap(),
            universe
        );
        assert!(after
            .search(TextField::Name, "alphabet", &universe)
            .unwrap()
            .is_empty());
        assert_parity(&connection, &after);
        connection
            .execute("DELETE FROM root_fts WHERE rowid=1", [])
            .unwrap();
        after.remove(&universe);
        assert_parity(&connection, &after);
        assert_eq!(after.len(), 2);
        after.refresh(&connection, &universe).unwrap();
        assert_parity(&connection, &after);
        assert!(after.removed.is_empty());
    }

    #[test]
    fn parallel_and_sparse_scans_respect_the_selection() {
        let mut connection = fixture();
        let transaction = connection.transaction().unwrap();
        for id in 3..40_000 {
            transaction.execute("INSERT INTO root_fts(rowid,name,notes,urls,source_text) VALUES (?1,?2,'','','')",
                rusqlite::params![id, if id%3==0 {"needle"} else {"other"}]).unwrap();
        }
        transaction.commit().unwrap();
        let index = TextIndex::load(&connection).unwrap();
        let all = (1..40_000).collect::<RoaringBitmap>();
        let sparse = (1..40_000)
            .filter(|id| id % 19 == 0)
            .collect::<RoaringBitmap>();
        let tiny = (1..40_000)
            .filter(|id| id % 97 == 0)
            .collect::<RoaringBitmap>();
        for universe in [all, sparse, tiny] {
            assert_eq!(
                index.search(TextField::Name, "needle", &universe).unwrap(),
                crate::fts::search(&connection, TextField::Name, "needle").unwrap() & universe
            );
        }
    }

    #[test]
    fn substring_scans_match_the_sql_oracle_across_fields_universes_and_typing() {
        // Short field values from a tiny alphabet make matches that start in
        // one field and end in the next common, which the shard scan must not
        // count, while a later in-field match in the same row must count.
        let mut connection = fixture();
        let transaction = connection.transaction().unwrap();
        let mut state = 0x9e37_79b9_u64;
        let word = |state: &mut u64| {
            (0..(*state % 6) as usize)
                .map(|_| {
                    *state ^= *state << 13;
                    *state ^= *state >> 7;
                    *state ^= *state << 17;
                    ["a", "b", "ab", "猫", "z"][(*state % 5) as usize]
                })
                .collect::<String>()
        };
        for id in 3..9_000 {
            let fields = [word(&mut state), word(&mut state), word(&mut state), word(&mut state)];
            transaction
                .execute(
                    "INSERT INTO root_fts(rowid,name,notes,urls,source_text) VALUES (?1,?2,?3,?4,?5)",
                    rusqlite::params![id, fields[0], fields[1], fields[2], fields[3]],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        let index = TextIndex::load(&connection).unwrap();
        let universes = [
            (1..9_000).collect::<RoaringBitmap>(),
            (1..9_000).filter(|id| id % 3 != 0).collect(),
            (1..9_000).filter(|id| id % 11 == 0).collect(),
            // Over the direct-lookup limit, but sparse in the later shards.
            (1..9_000).filter(|id| *id < 4_500 || id % 50 == 0).collect(),
        ];
        for field in [TextField::Global, TextField::Name, TextField::Notes, TextField::SourceUrl] {
            // Each query extends the previous one, as typing does.
            for query in ["aba", "abab", "ababz", "b猫a", "b猫ab", "zzz", "azb"] {
                for universe in &universes {
                    assert_eq!(
                        index.search(field, query, universe).unwrap(),
                        crate::fts::search(&connection, field, query).unwrap() & universe,
                        "{field:?} {query:?}"
                    );
                }
            }
        }
    }
}
