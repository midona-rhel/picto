use std::collections::HashMap;
use std::time::{Duration, Instant};

use picto_core::bit_sliced::{BitSlicedU64, FilteredAggregate, OptionalU8};
use roaring::RoaringBitmap;

#[test]
fn missing_zero_max_replace_and_remove_are_exact() {
    let mut index = BitSlicedU64::new();
    assert_eq!(index.set(7, 0), None);
    assert_eq!(index.set(9, u64::MAX), None);
    assert_eq!(index.set(10, u64::MAX), None);
    assert_eq!(index.get(7), Some(0));
    assert_eq!(index.get(8), None);
    assert_eq!(index.get(9), Some(u64::MAX));

    let selected = RoaringBitmap::from_iter([7, 8, 9, 10]);
    assert_eq!(
        index.filtered_aggregate(&selected),
        FilteredAggregate {
            count: 3,
            sum: u128::from(u64::MAX) * 2,
        }
    );

    assert_eq!(index.set(9, 11), Some(u64::MAX));
    assert_eq!(index.filtered_count(&selected), 3);
    assert_eq!(index.filtered_sum(&selected), u128::from(u64::MAX) + 11);
    assert_eq!(index.remove(7), Some(0));
    assert_eq!(index.remove(7), None);
    assert_eq!(index.remove(9), Some(11));
    assert_eq!(index.remove(10), Some(u64::MAX));
    assert!(index.is_empty());
}

#[test]
fn deterministic_mutation_sequence_matches_reference_map() {
    const IDS: u32 = 4_096;
    const OPERATIONS: usize = 50_000;

    let mut index = BitSlicedU64::new();
    let mut reference = HashMap::<u32, u64>::new();
    let mut random = Deterministic::new(0xd6e8_feb8_6659_fd93);

    for operation in 0..OPERATIONS {
        let item_id = (random.next() % u64::from(IDS)) as u32;
        if random.next().is_multiple_of(7) {
            assert_eq!(index.remove(item_id), reference.remove(&item_id));
        } else {
            let value = random.next();
            assert_eq!(index.set(item_id, value), reference.insert(item_id, value));
        }

        if operation % 211 == 0 {
            let filter = random_filter(IDS, &mut random);
            let expected = reference
                .iter()
                .filter(|(item_id, _)| filter.contains(**item_id))
                .fold(FilteredAggregate::default(), |mut aggregate, (_, value)| {
                    aggregate.count += 1;
                    aggregate.sum += u128::from(*value);
                    aggregate
                });
            assert_eq!(index.filtered_aggregate(&filter), expected);
        }
    }

    assert_eq!(index.len(), reference.len() as u64);
    for item_id in 0..IDS {
        assert_eq!(index.get(item_id), reference.get(&item_id).copied());
    }
}

#[test]
fn optional_u8_preserves_missing_and_zero() {
    let mut ratings = OptionalU8::new();
    assert!(ratings.is_empty());
    ratings.set(1, 0);
    ratings.set(2, 5);
    ratings.set(3, u8::MAX);
    assert_eq!(ratings.len(), 3);

    let selected = RoaringBitmap::from_iter([1, 2, 3, 4]);
    assert_eq!(ratings.filtered_count(&selected), 3);
    assert_eq!(ratings.filtered_sum(&selected), 260);
    assert_eq!(ratings.get(1), Some(0));
    assert_eq!(ratings.get(4), None);
    assert_eq!(ratings.set(2, 9), Some(5));
    assert_eq!(ratings.remove(3), Some(u8::MAX));
    assert_eq!(ratings.filtered_sum(&selected), 9);
    assert_eq!(
        ratings.filtered_aggregate(&selected),
        FilteredAggregate { count: 2, sum: 9 }
    );
    assert!(ratings.memory_usage().serialized_bytes > 0);
    ratings.clear();
    assert!(ratings.is_empty());
}

#[test]
fn clear_discards_all_u64_values() {
    let mut index = BitSlicedU64::new();
    index.set(1, u64::MAX);
    index.set(2, 0);
    index.clear();
    assert!(index.is_empty());
    assert_eq!(index.get(1), None);
}

#[test]
fn hundred_thousand_projection_performance() {
    run_performance_case(100_000, Duration::from_millis(50));
}

#[test]
#[ignore = "manual million-root projection performance gate"]
fn million_projection_performance() {
    run_performance_case(1_000_000, Duration::from_millis(100));
}

fn run_performance_case(rows: u32, aggregate_budget: Duration) {
    let mut index = BitSlicedU64::new();
    let mut expected = Vec::with_capacity(rows as usize);
    let build_started = Instant::now();
    for item_id in 0..rows {
        let value = deterministic_value(item_id);
        expected.push(value);
        index.set(item_id, value);
    }
    let build_elapsed = build_started.elapsed();

    let broad = RoaringBitmap::from_iter((0..rows).filter(|item_id| item_id % 10 != 9));
    let sparse = RoaringBitmap::from_iter((0..rows).step_by(97));
    let expected_broad = exact_scan(&expected, &broad);
    let expected_sparse = exact_scan(&expected, &sparse);

    let (broad_result, broad_elapsed) = measured(|| index.filtered_aggregate(&broad));
    let (sparse_result, sparse_elapsed) = measured(|| index.filtered_aggregate(&sparse));
    assert_eq!(broad_result, expected_broad);
    assert_eq!(sparse_result, expected_sparse);

    let replacements_started = Instant::now();
    for item_id in (0..rows).step_by(10) {
        let replacement = u64::from(item_id) * 17;
        assert_eq!(
            index.set(item_id, replacement),
            Some(expected[item_id as usize])
        );
        expected[item_id as usize] = replacement;
    }
    let replacements_elapsed = replacements_started.elapsed();
    assert_eq!(
        index.filtered_aggregate(&broad),
        exact_scan(&expected, &broad)
    );

    let memory = index.memory_usage();
    println!(
        "bit_sliced rows={rows} build_ms={:.3} broad_us={} sparse_us={} replace_10pct_ms={:.3} serialized_bytes={} compact_bytes={} bitmaps={}",
        build_elapsed.as_secs_f64() * 1_000.0,
        broad_elapsed.as_micros(),
        sparse_elapsed.as_micros(),
        replacements_elapsed.as_secs_f64() * 1_000.0,
        memory.serialized_bytes,
        memory.compact_bytes(),
        memory.bitmap_count,
    );

    assert!(
        broad_elapsed < aggregate_budget,
        "{rows}-row broad aggregate took {broad_elapsed:?}"
    );
    assert!(
        sparse_elapsed < aggregate_budget,
        "{rows}-row sparse aggregate took {sparse_elapsed:?}"
    );
    assert!(
        memory.serialized_bytes < rows as usize * 16,
        "compact representation used {} bytes for {rows} rows",
        memory.serialized_bytes
    );
}

fn exact_scan(values: &[u64], filter: &RoaringBitmap) -> FilteredAggregate {
    filter
        .iter()
        .fold(FilteredAggregate::default(), |mut aggregate, item_id| {
            aggregate.count += 1;
            aggregate.sum += u128::from(values[item_id as usize]);
            aggregate
        })
}

fn deterministic_value(item_id: u32) -> u64 {
    1_024 + (u64::from(item_id).wrapping_mul(6_364_136_223_846_793_005) % 67_107_840)
}

fn random_filter(ids: u32, random: &mut Deterministic) -> RoaringBitmap {
    RoaringBitmap::from_iter((0..ids).filter(|_| random.next() & 3 != 0))
}

fn measured<T>(operation: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let value = operation();
    (value, started.elapsed())
}

struct Deterministic(u64);

impl Deterministic {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}
