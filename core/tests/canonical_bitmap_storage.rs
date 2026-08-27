use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use picto_core::canonical_bitmap::{
    load_bitmap, load_order, replace_bitmap, replace_order, BitmapDomain,
};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};

const CARDINALITIES: [u32; 4] = [1, 50, 1_000, 100_000];

fn canonical_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("open in-memory database");
    connection
        .execute_batch(
            "CREATE TABLE canonical_bitmap (
                 domain INTEGER NOT NULL,
                 key_id INTEGER NOT NULL,
                 shard INTEGER NOT NULL CHECK (shard BETWEEN 0 AND 65535),
                 revision INTEGER NOT NULL CHECK (revision >= 1),
                 cardinality INTEGER NOT NULL CHECK (cardinality > 0),
                 format_version INTEGER NOT NULL CHECK (format_version = 1),
                 checksum TEXT NOT NULL CHECK (length(checksum) = 64),
                 payload BLOB NOT NULL,
                 PRIMARY KEY (domain, key_id, shard)
             ) WITHOUT ROWID;
             CREATE TABLE canonical_order (
                 owner_kind TEXT NOT NULL CHECK (owner_kind IN ('group', 'folder')),
                 owner_id INTEGER NOT NULL,
                 revision INTEGER NOT NULL CHECK (revision >= 1),
                 cardinality INTEGER NOT NULL CHECK (cardinality >= 0),
                 format_version INTEGER NOT NULL CHECK (format_version = 1),
                 checksum TEXT NOT NULL CHECK (length(checksum) = 64),
                 payload BLOB NOT NULL,
                 PRIMARY KEY (owner_kind, owner_id)
             ) WITHOUT ROWID;",
        )
        .expect("create canonical storage tables");
    connection
}

fn fixture(cardinality: u32, salt: u32) -> RoaringBitmap {
    // A non-unit stride crosses shard boundaries while retaining realistic,
    // monotonically allocated local IDs.
    (0..cardinality)
        .map(|index| index.saturating_mul(37).saturating_add(salt))
        .collect()
}

fn shard_count(bitmap: &RoaringBitmap) -> usize {
    bitmap
        .iter()
        .map(|item_id| item_id >> 16)
        .collect::<BTreeSet<_>>()
        .len()
}

fn stored_shape(connection: &Connection, domain: BitmapDomain, key_id: i64) -> (usize, u64) {
    let (rows, cardinality): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(cardinality), 0)
             FROM canonical_bitmap
             WHERE domain = ?1 AND key_id = ?2",
            params![domain.as_i64(), key_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read canonical bitmap shape");
    (
        usize::try_from(rows).expect("non-negative shard row count"),
        u64::try_from(cardinality).expect("non-negative bitmap cardinality"),
    )
}

#[test]
fn exact_replacement_scales_with_touched_shards_not_selected_roots() {
    for cardinality in CARDINALITIES {
        let mut connection = canonical_connection();
        let initial = fixture(cardinality, 11);
        let replacement = fixture(cardinality, 29);

        let started = Instant::now();
        let transaction = connection.transaction().expect("begin bitmap replacement");
        replace_bitmap(&transaction, BitmapDomain::Tag, 42, 1, &initial)
            .expect("store initial bitmap");
        transaction.commit().expect("commit initial bitmap");
        let initial_elapsed = started.elapsed();

        assert_eq!(
            stored_shape(&connection, BitmapDomain::Tag, 42),
            (shard_count(&initial), u64::from(cardinality))
        );
        assert_eq!(
            load_bitmap(&connection, BitmapDomain::Tag, 42).expect("load initial bitmap"),
            initial
        );

        let started = Instant::now();
        let transaction = connection.transaction().expect("begin exact replacement");
        replace_bitmap(&transaction, BitmapDomain::Tag, 42, 2, &replacement)
            .expect("replace bitmap");
        transaction.commit().expect("commit exact replacement");
        let replacement_elapsed = started.elapsed();

        let expected_shards = shard_count(&replacement);
        assert_eq!(
            stored_shape(&connection, BitmapDomain::Tag, 42),
            (expected_shards, u64::from(cardinality))
        );
        assert!(
            expected_shards < cardinality as usize || cardinality == 1,
            "canonical row count must scale with shards rather than roots"
        );
        assert_eq!(
            load_bitmap(&connection, BitmapDomain::Tag, 42).expect("load replacement bitmap"),
            replacement
        );

        eprintln!(
            "canonical bitmap roots={cardinality} shards={expected_shards} initial={initial_elapsed:?} replacement={replacement_elapsed:?}"
        );
    }
}

#[test]
fn replacement_removes_obsolete_shards() {
    let mut connection = canonical_connection();
    let broad = fixture(100_000, 5);
    let narrow = RoaringBitmap::from_iter([7, 8, 9]);

    let transaction = connection.transaction().expect("begin broad write");
    replace_bitmap(&transaction, BitmapDomain::Folder, 8, 1, &broad).expect("store broad bitmap");
    transaction.commit().expect("commit broad bitmap");
    assert!(stored_shape(&connection, BitmapDomain::Folder, 8).0 > 1);

    let transaction = connection.transaction().expect("begin narrow replacement");
    replace_bitmap(&transaction, BitmapDomain::Folder, 8, 2, &narrow).expect("store narrow bitmap");
    transaction.commit().expect("commit narrow bitmap");

    assert_eq!(stored_shape(&connection, BitmapDomain::Folder, 8), (1, 3));
    assert_eq!(
        load_bitmap(&connection, BitmapDomain::Folder, 8).expect("load narrow bitmap"),
        narrow
    );
}

#[test]
fn corrupted_bitmap_payload_is_rejected_by_checksum() {
    let mut connection = canonical_connection();
    let transaction = connection.transaction().expect("begin bitmap write");
    replace_bitmap(
        &transaction,
        BitmapDomain::Lifecycle,
        1,
        1,
        &fixture(1_000, 3),
    )
    .expect("store bitmap");
    transaction.commit().expect("commit bitmap");

    let (shard, mut payload): (i64, Vec<u8>) = connection
        .query_row(
            "SELECT shard, payload FROM canonical_bitmap
             WHERE domain = ?1 AND key_id = ?2 ORDER BY shard LIMIT 1",
            params![BitmapDomain::Lifecycle.as_i64(), 1],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load one shard to corrupt");
    payload[0] ^= 0xff;
    connection
        .execute(
            "UPDATE canonical_bitmap SET payload = ?1
             WHERE domain = ?2 AND key_id = ?3 AND shard = ?4",
            params![payload, BitmapDomain::Lifecycle.as_i64(), 1, shard],
        )
        .expect("corrupt one shard without updating its checksum");

    let error = load_bitmap(&connection, BitmapDomain::Lifecycle, 1)
        .expect_err("checksum corruption must reject canonical data");
    assert!(error.to_string().contains("checksum mismatch"), "{error}");
}

fn assert_group_order_invariant(connection: &Connection, group_id: i64) -> Result<(), String> {
    let members = load_bitmap(connection, BitmapDomain::GroupMember, group_id)
        .map_err(|error| error.to_string())?;
    let order = load_order(connection, "group", group_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "group order is missing".to_string())?;
    let ordered_members = order.iter().copied().collect::<RoaringBitmap>();
    if order.len() != ordered_members.len() as usize {
        return Err("group order contains duplicate members".to_string());
    }
    if ordered_members != members {
        return Err("group order and membership bitmap diverge".to_string());
    }
    Ok(())
}

#[test]
fn group_membership_and_order_are_separate_but_exactly_equivalent() {
    let mut connection = canonical_connection();
    let order = vec![65_537, 9, 131_079, 22];
    let members = order.iter().copied().collect::<RoaringBitmap>();

    let transaction = connection.transaction().expect("begin group write");
    replace_bitmap(&transaction, BitmapDomain::GroupMember, 77, 1, &members)
        .expect("store group membership");
    replace_order(&transaction, "group", 77, 1, &order).expect("store group order");
    transaction
        .commit()
        .expect("commit group membership and order");

    assert_group_order_invariant(&connection, 77).expect("matching group state");
    assert_eq!(
        load_order(&connection, "group", 77)
            .expect("load group order")
            .expect("group order exists"),
        order,
        "the independent ordering vector must preserve user order"
    );

    let transaction = connection.transaction().expect("begin invalid order write");
    replace_order(&transaction, "group", 77, 2, &[9, 22])
        .expect("store deliberately divergent order");
    transaction
        .commit()
        .expect("commit divergent order fixture");
    assert!(assert_group_order_invariant(&connection, 77)
        .expect_err("the invariant check must detect divergence")
        .contains("diverge"));
}

#[test]
#[ignore = "release-mode performance characterization"]
fn benchmark_100k_exact_replacement() {
    let mut connection = canonical_connection();
    let bitmap = fixture(100_000, 17);
    let expected_shards = shard_count(&bitmap);

    let mut samples = Vec::with_capacity(25);
    for revision in 1..=25 {
        let started = Instant::now();
        let transaction = connection.transaction().expect("begin benchmark write");
        replace_bitmap(&transaction, BitmapDomain::Tag, 99, revision, &bitmap)
            .expect("replace benchmark bitmap");
        transaction.commit().expect("commit benchmark bitmap");
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let p50 = samples[samples.len() / 2];
    let p95 = samples[samples.len() * 95 / 100];
    let max = *samples.last().unwrap_or(&Duration::ZERO);

    assert_eq!(
        stored_shape(&connection, BitmapDomain::Tag, 99),
        (expected_shards, 100_000)
    );
    eprintln!(
        "canonical bitmap 100k exact replacement shards={expected_shards} p50={p50:?} p95={p95:?} max={max:?}"
    );
}
