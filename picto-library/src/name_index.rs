//! Canonical NOCASE order, projected as an immutable, structurally shared tree
//! of bounded leaves. Subtree ID bitmaps let a new filter seek by ordinal
//! without enumerating its order. Rebuilt from canonical rows at open and
//! refreshed per root from the shared dirty-root journal at settlement.
use std::cmp::Ordering;
use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{Connection, OptionalExtension};

use crate::projection::ShardedIdMap;
use crate::Result;

const LEAF_SIZE: usize = 256;

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    id: u32,
    name: Box<str>,
}

// SQLite NOCASE folds ASCII only. Its comparison stops at NUL and breaks
// equal prefixes by the complete byte length, not the remaining suffix.
fn compare_name(a: &str, b: &str) -> Ordering {
    a.bytes()
        .take_while(|&c| c != 0)
        .map(|c| c.to_ascii_lowercase())
        .cmp(
            b.bytes()
                .take_while(|&c| c != 0)
                .map(|c| c.to_ascii_lowercase()),
        )
        .then_with(|| a.len().cmp(&b.len()))
}

impl Entry {
    fn compare(&self, name: &str, id: u32) -> Ordering {
        compare_name(&self.name, name).then_with(|| self.id.cmp(&id))
    }
}

#[derive(Debug, Clone)]
enum Children {
    Leaf(Vec<Arc<Entry>>),
    Branch(Arc<Node>, Arc<Node>),
}

#[derive(Debug, Clone)]
struct Node {
    children: Children,
    ids: RoaringBitmap,
    last: Arc<Entry>,
    height: u8,
    bytes: usize,
}

impl Node {
    fn leaf(entries: Vec<Arc<Entry>>) -> Arc<Self> {
        let ids: RoaringBitmap = entries.iter().map(|entry| entry.id).collect();
        Arc::new(Self {
            last: entries.last().expect("nonempty name leaf").clone(),
            height: 1,
            bytes: std::mem::size_of::<Self>()
                + 2 * std::mem::size_of::<usize>()
                + entries.capacity() * std::mem::size_of::<Arc<Entry>>()
                + ids.serialized_size(),
            ids,
            children: Children::Leaf(entries),
        })
    }

    fn branch(left: Arc<Self>, right: Arc<Self>) -> Arc<Self> {
        let ids = &left.ids | &right.ids;
        Self::branch_with_ids(left, right, ids)
    }

    fn branch_with_ids(left: Arc<Self>, right: Arc<Self>, ids: RoaringBitmap) -> Arc<Self> {
        Arc::new(Self {
            last: right.last.clone(),
            height: 1 + left.height.max(right.height),
            bytes: std::mem::size_of::<Self>()
                + 2 * std::mem::size_of::<usize>()
                + ids.serialized_size()
                + left.bytes
                + right.bytes,
            ids,
            children: Children::Branch(left, right),
        })
    }

    // Persistent AVL rotations touch only the edited path. In-flight query
    // snapshots retain their original leaves and subtree membership.
    fn balance(left: Arc<Self>, right: Arc<Self>) -> Arc<Self> {
        if left.height > right.height + 1 {
            let Children::Branch(ll, lr) = &left.children else {
                unreachable!()
            };
            if ll.height >= lr.height {
                return Self::branch(ll.clone(), Self::branch(lr.clone(), right));
            }
            let Children::Branch(lrl, lrr) = &lr.children else {
                unreachable!()
            };
            return Self::branch(
                Self::branch(ll.clone(), lrl.clone()),
                Self::branch(lrr.clone(), right),
            );
        }
        if right.height > left.height + 1 {
            let Children::Branch(rl, rr) = &right.children else {
                unreachable!()
            };
            if rr.height >= rl.height {
                return Self::branch(Self::branch(left, rl.clone()), rr.clone());
            }
            let Children::Branch(rll, rlr) = &rl.children else {
                unreachable!()
            };
            return Self::branch(
                Self::branch(left, rll.clone()),
                Self::branch(rlr.clone(), rr.clone()),
            );
        }
        Self::branch(left, right)
    }

    fn insert(self: Arc<Self>, entry: Arc<Entry>) -> Arc<Self> {
        // A batch owns its new path after the first edit. Clone only nodes still
        // shared with the published snapshot; reuse the batch's bitmap storage.
        let Self {
            children, mut ids, ..
        } = Arc::unwrap_or_clone(self);
        match children {
            Children::Leaf(mut entries) => {
                let at =
                    entries.partition_point(|item| item.compare(&entry.name, entry.id).is_lt());
                entries.insert(at, entry);
                if entries.len() <= LEAF_SIZE {
                    return Self::leaf(entries);
                }
                let right = entries.split_off(entries.len() / 2);
                Self::branch(Self::leaf(entries), Self::leaf(right))
            }
            Children::Branch(left, right) => {
                ids.insert(entry.id);
                let (left, right) = if left.last.compare(&entry.name, entry.id).is_lt() {
                    (left, right.insert(entry))
                } else {
                    (left.insert(entry), right)
                };
                if left.height.abs_diff(right.height) > 1 {
                    Self::balance(left, right)
                } else {
                    Self::branch_with_ids(left, right, ids)
                }
            }
        }
    }

    fn remove(self: Arc<Self>, entry: &Entry) -> Option<Arc<Self>> {
        let Self {
            children, mut ids, ..
        } = Arc::unwrap_or_clone(self);
        match children {
            Children::Leaf(mut entries) => {
                entries.retain(|item| item.id != entry.id);
                (!entries.is_empty()).then(|| Self::leaf(entries))
            }
            Children::Branch(left, right) => {
                let (left, right) = if left.last.compare(&entry.name, entry.id).is_lt() {
                    match right.remove(entry) {
                        Some(right) => (left, right),
                        None => return Some(left),
                    }
                } else {
                    match left.remove(entry) {
                        Some(left) => (left, right),
                        None => return Some(right),
                    }
                };
                if let (Children::Leaf(a), Children::Leaf(b)) = (&left.children, &right.children) {
                    if a.len() + b.len() <= LEAF_SIZE {
                        return Some(Self::leaf(a.iter().chain(b).cloned().collect()));
                    }
                }
                ids.remove(entry.id);
                Some(if left.height.abs_diff(right.height) > 1 {
                    Self::balance(left, right)
                } else {
                    Self::branch_with_ids(left, right, ids)
                })
            }
        }
    }

    fn window(
        &self,
        selection: &RoaringBitmap,
        skip: &mut usize,
        limit: usize,
        descending: bool,
        out: &mut Vec<u32>,
    ) -> Result<()> {
        crate::query_request::check()?;
        if out.len() == limit {
            return Ok(());
        }
        let count = self.ids.intersection_len(selection) as usize;
        if *skip >= count {
            *skip -= count;
            return Ok(());
        }
        match &self.children {
            Children::Leaf(entries) => {
                for offset in 0..entries.len() {
                    let entry = &entries[if descending {
                        entries.len() - 1 - offset
                    } else {
                        offset
                    }];
                    if !selection.contains(entry.id) {
                        continue;
                    }
                    if *skip > 0 {
                        *skip -= 1;
                    } else {
                        out.push(entry.id);
                    }
                    if out.len() == limit {
                        break;
                    }
                }
            }
            Children::Branch(left, right) => {
                let (first, second) = if descending {
                    (right, left)
                } else {
                    (left, right)
                };
                first.window(selection, skip, limit, descending, out)?;
                second.window(selection, skip, limit, descending, out)?;
            }
        }
        Ok(())
    }

    fn through(&self, selection: &RoaringBitmap, name: &str, id: u32, inclusive: bool) -> usize {
        let before = |entry: &Entry| {
            let ordering = entry.compare(name, id);
            ordering.is_lt() || (inclusive && ordering.is_eq())
        };
        if before(&self.last) {
            return self.ids.intersection_len(selection) as usize;
        }
        match &self.children {
            Children::Leaf(entries) => entries
                .iter()
                .take_while(|e| before(e))
                .filter(|e| selection.contains(e.id))
                .count(),
            Children::Branch(left, right) => {
                if before(&left.last) {
                    left.ids.intersection_len(selection) as usize
                        + right.through(selection, name, id, inclusive)
                } else {
                    left.through(selection, name, id, inclusive)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NameIndex {
    root: Option<Arc<Node>>,
    by_id: ShardedIdMap<Arc<Entry>>,
    key_bytes: usize,
}

impl NameIndex {
    /// One streaming scan in canonical NOCASE order, packed into leaves and
    /// balanced bottom-up. Nothing about this order is persisted separately.
    pub(crate) fn load(connection: &Connection) -> Result<Self> {
        let mut index = Self::default();
        let mut statement = connection.prepare(
            "SELECT root_id, name FROM library_root ORDER BY name COLLATE NOCASE, root_id",
        )?;
        let mut rows = statement.query([])?;
        let mut entries = Vec::with_capacity(LEAF_SIZE);
        let mut leaves = Vec::new();
        while let Some(row) = rows.next()? {
            if entries.is_empty() {
                crate::query_request::check()?;
            }
            let entry = Arc::new(Entry {
                id: row.get(0)?,
                name: row.get::<_, String>(1)?.into_boxed_str(),
            });
            index.key_bytes += Self::entry_bytes(&entry);
            index.by_id.insert(entry.id, entry.clone());
            entries.push(entry);
            if entries.len() == LEAF_SIZE {
                leaves.push(Node::leaf(std::mem::replace(
                    &mut entries,
                    Vec::with_capacity(LEAF_SIZE),
                )));
            }
        }
        if !entries.is_empty() {
            leaves.push(Node::leaf(entries));
        }
        fn build(leaves: &[Arc<Node>]) -> Arc<Node> {
            if leaves.len() == 1 {
                return leaves[0].clone();
            }
            let mid = leaves.len() / 2;
            Node::branch(build(&leaves[..mid]), build(&leaves[mid..]))
        }
        index.root = (!leaves.is_empty()).then(|| build(&leaves));
        Ok(index)
    }

    fn entry_bytes(entry: &Entry) -> usize {
        std::mem::size_of::<Entry>() + entry.name.len() + 2 * std::mem::size_of::<usize>()
    }

    pub fn estimated_bytes(&self) -> usize {
        self.key_bytes
            + self.by_id.estimated_bytes()
            + self.root.as_ref().map_or(0, |root| root.bytes)
    }

    pub(crate) fn insert(&mut self, id: u32, name: &str) {
        if self.by_id.get(id).is_some_and(|entry| &*entry.name == name) {
            return;
        }
        self.remove(id);
        let entry = Arc::new(Entry {
            id,
            name: Box::from(name),
        });
        self.key_bytes += Self::entry_bytes(&entry);
        self.root = Some(self.root.take().map_or_else(
            || Node::leaf(vec![entry.clone()]),
            |root| root.insert(entry.clone()),
        ));
        self.by_id.insert(id, entry);
    }

    pub(crate) fn remove(&mut self, id: u32) {
        if let Some(entry) = self.by_id.remove(id) {
            self.key_bytes -= Self::entry_bytes(&entry);
            self.root = self.root.take().and_then(|root| root.remove(&entry));
        }
    }

    pub(crate) fn refresh(&mut self, connection: &Connection, ids: &RoaringBitmap) -> Result<()> {
        let mut statement =
            connection.prepare_cached("SELECT name FROM library_root WHERE root_id = ?1")?;
        for id in ids {
            match statement
                .query_row([id], |row| row.get::<_, String>(0))
                .optional()?
            {
                Some(name) => self.insert(id, &name),
                None => self.remove(id),
            }
        }
        Ok(())
    }

    pub(crate) fn window(
        &self,
        selection: &RoaringBitmap,
        mut start: usize,
        limit: usize,
        descending: bool,
    ) -> Result<Vec<u32>> {
        let mut out = Vec::with_capacity(limit.min(selection.len() as usize));
        if let Some(root) = &self.root {
            root.window(selection, &mut start, limit, descending, &mut out)?;
        }
        Ok(out)
    }

    pub(crate) fn after(
        &self,
        selection: &RoaringBitmap,
        name: &str,
        id: u32,
        descending: bool,
    ) -> usize {
        self.root.as_ref().map_or(0, |root| {
            if descending {
                root.ids.intersection_len(selection) as usize
                    - root.through(selection, name, id, false)
            } else {
                root.through(selection, name, id, true)
            }
        })
    }

    pub(crate) fn name(&self, id: u32) -> Option<&str> {
        self.by_id.get(id).map(|entry| &*entry.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "million-row index benchmark; run in release mode with --ignored --nocapture"]
    fn million_name_index_build_and_incremental_writes() {
        use std::time::Instant;
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE library_root(root_id INTEGER PRIMARY KEY, name TEXT NOT NULL); CREATE INDEX names ON library_root(name COLLATE NOCASE, root_id)").unwrap();
        let transaction = connection.transaction().unwrap();
        {
            let mut insert = transaction
                .prepare("INSERT INTO library_root VALUES (?1, ?2)")
                .unwrap();
            for id in 1u32..=1_000_000 {
                insert
                    .execute(rusqlite::params![
                        id,
                        format!("Image {:010}", id.wrapping_mul(2654435761))
                    ])
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
        let started = Instant::now();
        let mut index = NameIndex::load(&connection).unwrap();
        let build_ms = started.elapsed().as_secs_f64() * 1000.0;
        let all = (1..=1_000_000).collect::<RoaringBitmap>();
        let filter = (1..=1_000_000)
            .filter(|id| id % 2 == 0)
            .collect::<RoaringBitmap>();
        let old = index.clone();
        let original_middle = old.window(&filter, 250_000, 1500, false).unwrap();
        let mut writes = Vec::new();
        for i in 1..=100 {
            let before = index.clone();
            let started = Instant::now();
            index.insert(i * 997, &format!("Renamed {i:05}"));
            writes.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_ne!(index.name(i * 997), before.name(i * 997));
        }
        assert_eq!(
            old.window(&filter, 250_000, 1500, false).unwrap(),
            original_middle
        );
        let mut reads = Vec::new();
        for selection in [&all, &filter] {
            for start in [0, 100_000, 250_000, 498_500] {
                let started = Instant::now();
                let result = index.window(selection, start, 1500, false).unwrap();
                reads.push(started.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(result.len(), 1500);
            }
        }
        writes.sort_by(f64::total_cmp);
        let before_batch = index.clone();
        let started = Instant::now();
        for id in 1_000_001..=1_001_000 {
            index.insert(id, &format!("New import {id}"));
        }
        let batch_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert!(before_batch.name(1_000_001).is_none());
        assert!(index.name(1_000_001).is_some());
        println!(
            "NAME_INDEX_SCALE {}",
            serde_json::json!({"roots":1_000_000,"build_ms":build_ms,"estimated_bytes":index.estimated_bytes(),"rename_p50_ms":writes[50],"rename_p95_ms":writes[95],"rename_max_ms":writes[99],"batch_insert_1000_ms":batch_ms,"window_ms":reads})
        );
    }

    fn oracle(connection: &Connection, selection: &RoaringBitmap, descending: bool) -> Vec<u32> {
        let direction = if descending { "DESC" } else { "ASC" };
        connection.prepare(&format!("SELECT root_id FROM library_root ORDER BY name COLLATE NOCASE {direction}, root_id {direction}"))
            .unwrap().query_map([], |row| row.get::<_, u32>(0)).unwrap()
            .map(|id| id.unwrap()).filter(|id| selection.contains(*id)).collect()
    }

    fn validate(node: &Node) {
        match &node.children {
            Children::Leaf(entries) => {
                assert!(!entries.is_empty() && entries.len() <= LEAF_SIZE);
                assert!(entries
                    .windows(2)
                    .all(|w| w[0].compare(&w[1].name, w[1].id).is_lt()));
                assert_eq!(node.ids, entries.iter().map(|e| e.id).collect());
                assert_eq!(node.height, 1);
                assert_eq!(node.last, *entries.last().unwrap());
            }
            Children::Branch(left, right) => {
                validate(left);
                validate(right);
                assert!(left.height.abs_diff(right.height) <= 1);
                assert_eq!(node.height, 1 + left.height.max(right.height));
                assert_eq!(node.ids, &left.ids | &right.ids);
                assert_eq!(node.last, right.last);
                assert!(left.last.compare(&right.last.name, right.last.id).is_lt());
            }
        }
    }

    fn check(connection: &Connection, index: &NameIndex, selection: &RoaringBitmap) {
        if let Some(root) = &index.root {
            validate(root);
        }
        for descending in [false, true] {
            let expected = oracle(connection, selection, descending);
            assert_eq!(
                index.window(selection, 0, usize::MAX, descending).unwrap(),
                expected
            );
            for start in [
                0,
                1,
                expected.len() / 2,
                expected.len().saturating_sub(31),
                expected.len(),
            ] {
                let end = (start + 31).min(expected.len());
                assert_eq!(
                    index.window(selection, start, 31, descending).unwrap(),
                    expected[start.min(end)..end]
                );
            }
            for (at, &id) in expected.iter().enumerate().step_by(71) {
                assert_eq!(
                    index.after(selection, index.name(id).unwrap(), id, descending),
                    at + 1
                );
            }
        }
    }

    #[test]
    fn alphabetical_windows_mutations_and_old_snapshots_match_sqlite() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE library_root(root_id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            )
            .unwrap();
        let names = [
            "", "alpha", "Alpha", "ALPHA", "é", "É", "東京", "a\0b", "A\0z", "a\0long", "a", "a\0",
            "\0", "\0xx", "ß",
        ];
        let mut index = NameIndex::default();
        for id in 1..=6000 {
            let name = format!("{}{}", names[id as usize % names.len()], id % 73);
            connection
                .execute(
                    "INSERT INTO library_root VALUES (?1, ?2)",
                    rusqlite::params![id, name],
                )
                .unwrap();
            index.insert(id, &name);
        }
        let all = (1..=6000).collect::<RoaringBitmap>();
        let old = index.clone();
        let old_order = oracle(&connection, &all, false);
        check(&connection, &index, &all);
        check(&connection, &NameIndex::load(&connection).unwrap(), &all);
        let sparse = (1..=6000)
            .filter(|id| id % 227 == 0)
            .collect::<RoaringBitmap>();
        check(&connection, &index, &sparse);
        check(&connection, &index, &RoaringBitmap::new());
        // Rename into and out of tied keys; deleting nearly everything exercises
        // both rotation directions, leaf merges, and removal of the last root.
        for id in 1..=6000 {
            if id % 3 == 0 {
                let name = names[id as usize % names.len()];
                connection
                    .execute(
                        "UPDATE library_root SET name = ?2 WHERE root_id = ?1",
                        rusqlite::params![id, name],
                    )
                    .unwrap();
                index.insert(id, name);
            } else {
                connection
                    .execute("DELETE FROM library_root WHERE root_id = ?1", [id])
                    .unwrap();
                index.remove(id);
            }
            if id % 1000 == 0 {
                check(&connection, &index, &all);
            }
        }
        assert_eq!(old.window(&all, 0, 6000, false).unwrap(), old_order);
        check(&connection, &index, &all);
        for id in (1..=6000).rev() {
            index.remove(id);
        }
        assert!(index.root.is_none());
        assert_eq!(index.key_bytes, 0);
    }

    #[test]
    fn name_comparator_matches_sqlite_including_embedded_nul() {
        let connection = Connection::open_in_memory().unwrap();
        let names = [
            "", "a", "A", "aa", "a\0", "a\0b", "a\0z", "a\0long", "\0", "\0xx", "ä", "Ä", "😀",
            "東京",
        ];
        for a in names {
            for b in names {
                let expected: i32 = connection.query_row("SELECT CASE WHEN ?1 = ?2 COLLATE NOCASE THEN 0 WHEN ?1 < ?2 COLLATE NOCASE THEN -1 ELSE 1 END", [a, b], |row| row.get(0)).unwrap();
                assert_eq!(compare_name(a, b), expected.cmp(&0), "{a:?} / {b:?}");
            }
        }
    }
}
