//! Gate for using bit-sliced Roaring indexes for filtered numeric aggregates.
//!
//! This remains a benchmark-side prototype until it demonstrates a material
//! win over the simplest exact scan at one million roots.

use std::time::{Duration, Instant};

use roaring::RoaringBitmap;

#[derive(Default)]
struct BitSlicedU64 {
    slices: Vec<RoaringBitmap>,
}

impl BitSlicedU64 {
    fn insert(&mut self, item_id: u32, value: u64) {
        let width = (u64::BITS - value.leading_zeros()) as usize;
        if width > self.slices.len() {
            self.slices.resize_with(width, RoaringBitmap::new);
        }
        for bit in 0..width {
            if value & (1_u64 << bit) != 0 {
                self.slices[bit].insert(item_id);
            }
        }
    }

    fn replace(&mut self, item_id: u32, previous: u64, next: u64) {
        let width = (u64::BITS - (previous | next).leading_zeros()) as usize;
        if width > self.slices.len() {
            self.slices.resize_with(width, RoaringBitmap::new);
        }
        for bit in 0..width {
            let mask = 1_u64 << bit;
            match (previous & mask != 0, next & mask != 0) {
                (false, true) => {
                    self.slices[bit].insert(item_id);
                }
                (true, false) => {
                    self.slices[bit].remove(item_id);
                }
                _ => {}
            }
        }
    }

    fn sum(&self, filter: &RoaringBitmap) -> u128 {
        self.slices
            .iter()
            .enumerate()
            .map(|(bit, slice)| u128::from(slice.intersection_len(filter)) * (1_u128 << bit))
            .sum()
    }

    fn serialized_bytes(&self) -> usize {
        self.slices.iter().map(RoaringBitmap::serialized_size).sum()
    }
}

#[test]
fn bit_sliced_sum_and_replace_are_exact() {
    let values = [0_u64, 1, 7, 1_024, u32::MAX as u64 + 7];
    let mut index = BitSlicedU64::default();
    for (item_id, value) in values.into_iter().enumerate() {
        index.insert(item_id as u32, value);
    }
    let filter = RoaringBitmap::from_iter([1, 2, 4]);
    assert_eq!(index.sum(&filter), u128::from(1 + 7 + values[4]));

    index.replace(2, 7, 33);
    assert_eq!(index.sum(&filter), u128::from(1 + 33 + values[4]));
}

#[test]
fn bit_sliced_aggregate_1_50_1k_and_100k_is_exact() {
    const ROWS: usize = 100_000;
    let values = (0..ROWS)
        .map(|index| 1_024 + ((index as u64 * 1_103_515_245 + 12_345) % 67_107_840))
        .collect::<Vec<_>>();
    let mut index = BitSlicedU64::default();
    for (item_id, value) in values.iter().copied().enumerate() {
        index.insert(item_id as u32, value);
    }

    for cardinality in [1_usize, 50, 1_000, 100_000] {
        let filter = RoaringBitmap::from_iter(0..cardinality as u32);
        let (actual, bsi_elapsed) = measured(|| index.sum(&filter));
        let (expected, scan_elapsed) = measured(|| dense_sum(&values, &filter));
        assert_eq!(actual, expected, "wrong sum at cardinality {cardinality}");
        println!(
            "bsi_metric cardinality={} bsi_us={} scan_us={} bytes={}",
            cardinality,
            bsi_elapsed.as_micros(),
            scan_elapsed.as_micros(),
            index.serialized_bytes(),
        );
    }
}

#[test]
#[ignore = "manual one-million-root BSI adoption gate"]
fn million_root_bit_sliced_aggregate_gate() {
    const ROWS: usize = 1_000_000;
    const MAX_BYTES: usize = 16 * 1024 * 1024;
    const MAX_BROAD_SUM: Duration = Duration::from_millis(5);

    let values = (0..ROWS)
        .map(|index| {
            // Deterministic 1 KiB..64 MiB distribution with enough entropy to
            // exercise dense bit slices rather than an artificial easy case.
            1_024 + ((index as u64 * 6_364_136_223_846_793_005_u64) % 67_107_840)
        })
        .collect::<Vec<_>>();
    let mut index = BitSlicedU64::default();
    for (item_id, value) in values.iter().copied().enumerate() {
        index.insert(item_id as u32, value);
    }

    let broad = RoaringBitmap::from_iter((0..ROWS as u32).filter(|item_id| item_id % 10 != 9));
    let sparse = RoaringBitmap::from_iter((0..ROWS as u32).step_by(100));
    let (bsi_broad, bsi_broad_time) = measured(|| index.sum(&broad));
    let (scan_broad, scan_broad_time) = measured(|| dense_sum(&values, &broad));
    let (bsi_sparse, bsi_sparse_time) = measured(|| index.sum(&sparse));
    let (scan_sparse, scan_sparse_time) = measured(|| dense_sum(&values, &sparse));

    assert_eq!(bsi_broad, scan_broad);
    assert_eq!(bsi_sparse, scan_sparse);
    let bytes = index.serialized_bytes();
    println!(
        "bsi_gate: bytes={} broad_bsi_us={} broad_scan_us={} sparse_bsi_us={} sparse_scan_us={}",
        bytes,
        bsi_broad_time.as_micros(),
        scan_broad_time.as_micros(),
        bsi_sparse_time.as_micros(),
        scan_sparse_time.as_micros(),
    );
    assert!(bytes <= MAX_BYTES, "BSI uses {bytes} bytes");
    assert!(
        bsi_broad_time <= MAX_BROAD_SUM,
        "broad BSI sum took {bsi_broad_time:?}"
    );
    assert!(
        bsi_broad_time.saturating_mul(3) <= scan_broad_time,
        "broad BSI did not beat the exact scan by 3x"
    );
}

fn dense_sum(values: &[u64], filter: &RoaringBitmap) -> u128 {
    filter
        .iter()
        .map(|item_id| u128::from(values[item_id as usize]))
        .sum()
}

fn measured<T>(operation: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let value = operation();
    (value, started.elapsed())
}
