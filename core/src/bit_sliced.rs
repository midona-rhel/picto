//! Exact numeric aggregates backed by bit-sliced Roaring bitmaps.
//!
//! Each bit in a stored value has its own bitmap. Intersecting those slices
//! with a filtered root bitmap makes count and sum proportional to the value
//! width rather than to the number of selected roots.

use std::mem::size_of;

use roaring::RoaringBitmap;

/// An exact count and sum over the values present in a filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FilteredAggregate {
    pub count: u64,
    pub sum: u128,
}

/// Compact-size accounting for a bit-sliced index.
///
/// `serialized_bytes` is the exact size of the portable Roaring encoding and
/// is the stable metric used for projection budgets. `structural_bytes` covers
/// the Rust containers themselves, but deliberately does not claim to measure
/// allocator-specific resident memory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BitSlicedMemoryUsage {
    pub bitmap_count: usize,
    pub serialized_bytes: usize,
    pub structural_bytes: usize,
}

impl BitSlicedMemoryUsage {
    pub fn compact_bytes(self) -> usize {
        self.serialized_bytes + self.structural_bytes
    }
}

/// A sparse, exact `u64` column indexed by a `u32` root identifier.
///
/// Zero is a stored value and is distinct from an absent identifier.
#[derive(Debug, Clone, Default)]
pub struct BitSlicedU64 {
    present: RoaringBitmap,
    slices: Vec<RoaringBitmap>,
}

impl BitSlicedU64 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> u64 {
        self.present.len()
    }

    pub fn is_empty(&self) -> bool {
        self.present.is_empty()
    }

    /// Insert or replace a value, returning the previous value when present.
    pub fn set(&mut self, item_id: u32, value: u64) -> Option<u64> {
        let previous = self.get(item_id);
        self.ensure_width(bit_width(value));

        if previous.is_some() {
            for (bit, slice) in self.slices.iter_mut().enumerate() {
                if value & (1_u64 << bit) == 0 {
                    slice.remove(item_id);
                } else {
                    slice.insert(item_id);
                }
            }
        } else {
            self.present.insert(item_id);
            for bit in set_bits(value) {
                self.slices[bit].insert(item_id);
            }
        }

        self.trim_empty_high_slices();
        previous
    }

    /// Remove a value, returning it when the identifier was present.
    pub fn remove(&mut self, item_id: u32) -> Option<u64> {
        if !self.present.remove(item_id) {
            return None;
        }

        let mut previous = 0_u64;
        for (bit, slice) in self.slices.iter_mut().enumerate() {
            if slice.remove(item_id) {
                previous |= 1_u64 << bit;
            }
        }
        self.trim_empty_high_slices();
        Some(previous)
    }

    /// Assign one value to an arbitrary set of identifiers with bitmap
    /// algebra. Broad metadata changes must not degrade into one operation per
    /// selected root.
    pub fn set_bitmap(&mut self, item_ids: &RoaringBitmap, value: u64) {
        if item_ids.is_empty() {
            return;
        }
        self.ensure_width(bit_width(value));
        self.present |= item_ids;
        for (bit, slice) in self.slices.iter_mut().enumerate() {
            if value & (1_u64 << bit) == 0 {
                *slice -= item_ids;
            } else {
                *slice |= item_ids;
            }
        }
        self.trim_empty_high_slices();
    }

    /// Remove an arbitrary identifier set without visiting individual roots.
    pub fn remove_bitmap(&mut self, item_ids: &RoaringBitmap) {
        if item_ids.is_empty() {
            return;
        }
        self.present -= item_ids;
        for slice in &mut self.slices {
            *slice -= item_ids;
        }
        self.trim_empty_high_slices();
    }

    pub fn get(&self, item_id: u32) -> Option<u64> {
        if !self.present.contains(item_id) {
            return None;
        }

        Some(
            self.slices
                .iter()
                .enumerate()
                .fold(0_u64, |value, (bit, slice)| {
                    value | (u64::from(slice.contains(item_id)) << bit)
                }),
        )
    }

    /// Return the identifiers whose stored value exactly matches `value`.
    /// The result is restricted to `universe`, which lets categorical views
    /// such as ratings be serialized without scanning individual roots.
    pub fn value_bitmap(&self, value: u64, universe: &RoaringBitmap) -> RoaringBitmap {
        if bit_width(value) > self.slices.len() {
            return RoaringBitmap::new();
        }
        let mut matches = &self.present & universe;
        for (bit, slice) in self.slices.iter().enumerate() {
            if value & (1_u64 << bit) == 0 {
                matches -= slice;
            } else {
                matches &= slice;
            }
        }
        matches
    }

    pub fn present_bitmap(&self) -> RoaringBitmap {
        self.present.clone()
    }

    /// Count stored values selected by `filter`. Unknown IDs are ignored.
    pub fn filtered_count(&self, filter: &RoaringBitmap) -> u64 {
        self.present.intersection_len(filter)
    }

    /// Sum stored values selected by `filter` without integer overflow.
    pub fn filtered_sum(&self, filter: &RoaringBitmap) -> u128 {
        self.slices
            .iter()
            .enumerate()
            .map(|(bit, slice)| u128::from(slice.intersection_len(filter)) * (1_u128 << bit))
            .sum()
    }

    pub fn filtered_aggregate(&self, filter: &RoaringBitmap) -> FilteredAggregate {
        FilteredAggregate {
            count: self.filtered_count(filter),
            sum: self.filtered_sum(filter),
        }
    }

    /// Exact minimum and maximum without visiting selected identifiers.
    pub fn filtered_min_max(&self, filter: &RoaringBitmap) -> Option<(u64, u64)> {
        let selected = &self.present & filter;
        if selected.is_empty() {
            return None;
        }

        let mut minimum_candidates = selected.clone();
        let mut maximum_candidates = selected;
        let mut minimum = 0_u64;
        let mut maximum = 0_u64;
        for (bit, slice) in self.slices.iter().enumerate().rev() {
            let minimum_zeroes = &minimum_candidates - slice;
            if minimum_zeroes.is_empty() {
                minimum |= 1_u64 << bit;
                minimum_candidates &= slice;
            } else {
                minimum_candidates = minimum_zeroes;
            }

            let maximum_ones = &maximum_candidates & slice;
            if !maximum_ones.is_empty() {
                maximum |= 1_u64 << bit;
                maximum_candidates = maximum_ones;
            } else {
                maximum_candidates -= slice;
            }
        }
        Some((minimum, maximum))
    }

    pub fn clear(&mut self) {
        self.present.clear();
        self.slices.clear();
    }

    pub fn memory_usage(&self) -> BitSlicedMemoryUsage {
        let serialized_bytes = self.present.serialized_size()
            + self
                .slices
                .iter()
                .map(RoaringBitmap::serialized_size)
                .sum::<usize>();
        BitSlicedMemoryUsage {
            bitmap_count: 1 + self.slices.len(),
            serialized_bytes,
            structural_bytes: size_of::<Self>()
                + self.slices.capacity() * size_of::<RoaringBitmap>(),
        }
    }

    fn ensure_width(&mut self, width: usize) {
        if width > self.slices.len() {
            self.slices.resize_with(width, RoaringBitmap::new);
        }
    }

    fn trim_empty_high_slices(&mut self) {
        while self.slices.last().is_some_and(RoaringBitmap::is_empty) {
            self.slices.pop();
        }
    }
}

/// A small optional numeric column suitable for ratings and similar values.
///
/// It retains missing-vs-zero semantics while reusing the exact BSI behavior.
#[derive(Debug, Clone, Default)]
pub struct OptionalU8 {
    values: BitSlicedU64,
}

impl OptionalU8 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> u64 {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn set(&mut self, item_id: u32, value: u8) -> Option<u8> {
        self.values
            .set(item_id, u64::from(value))
            .map(|old| old as u8)
    }

    pub fn remove(&mut self, item_id: u32) -> Option<u8> {
        self.values.remove(item_id).map(|old| old as u8)
    }

    pub fn set_bitmap(&mut self, item_ids: &RoaringBitmap, value: u8) {
        self.values.set_bitmap(item_ids, u64::from(value));
    }

    pub fn remove_bitmap(&mut self, item_ids: &RoaringBitmap) {
        self.values.remove_bitmap(item_ids);
    }

    pub fn get(&self, item_id: u32) -> Option<u8> {
        self.values.get(item_id).map(|value| value as u8)
    }

    pub fn value_bitmap(&self, value: u8, universe: &RoaringBitmap) -> RoaringBitmap {
        self.values.value_bitmap(u64::from(value), universe)
    }

    pub fn present_bitmap(&self) -> RoaringBitmap {
        self.values.present_bitmap()
    }

    pub fn filtered_count(&self, filter: &RoaringBitmap) -> u64 {
        self.values.filtered_count(filter)
    }

    pub fn filtered_sum(&self, filter: &RoaringBitmap) -> u64 {
        self.values.filtered_sum(filter) as u64
    }

    pub fn filtered_aggregate(&self, filter: &RoaringBitmap) -> FilteredAggregate {
        self.values.filtered_aggregate(filter)
    }

    pub fn filtered_min_max(&self, filter: &RoaringBitmap) -> Option<(u8, u8)> {
        self.values
            .filtered_min_max(filter)
            .map(|(minimum, maximum)| (minimum as u8, maximum as u8))
    }

    pub fn memory_usage(&self) -> BitSlicedMemoryUsage {
        self.values.memory_usage()
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }
}

fn bit_width(value: u64) -> usize {
    (u64::BITS - value.leading_zeros()) as usize
}

fn set_bits(mut value: u64) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        if value == 0 {
            return None;
        }
        let bit = value.trailing_zeros() as usize;
        value &= value - 1;
        Some(bit)
    })
}

#[cfg(test)]
mod tests {
    use roaring::RoaringBitmap;

    use super::{BitSlicedU64, OptionalU8};

    #[test]
    fn bitmap_assignment_replaces_existing_values_exactly() {
        let mut index = BitSlicedU64::new();
        index.set(1, 3);
        index.set(2, 8);
        index.set(4, 15);

        let changed = RoaringBitmap::from_iter([1, 2, 3]);
        index.set_bitmap(&changed, 5);

        assert_eq!(index.get(1), Some(5));
        assert_eq!(index.get(2), Some(5));
        assert_eq!(index.get(3), Some(5));
        assert_eq!(index.get(4), Some(15));
        assert_eq!(index.filtered_sum(&changed), 15);

        index.remove_bitmap(&RoaringBitmap::from_iter([2, 4]));
        assert_eq!(index.get(1), Some(5));
        assert_eq!(index.get(2), None);
        assert_eq!(index.get(4), None);
    }

    #[test]
    fn optional_rating_bitmap_preserves_missing_values() {
        let mut ratings = OptionalU8::new();
        let roots = RoaringBitmap::from_iter([10, 20, 30]);
        ratings.set_bitmap(&roots, 4);
        assert_eq!(ratings.filtered_count(&roots), 3);
        assert_eq!(ratings.filtered_sum(&roots), 12);

        ratings.remove_bitmap(&RoaringBitmap::from_iter([20, 30]));
        assert_eq!(ratings.get(10), Some(4));
        assert_eq!(ratings.get(20), None);
        assert_eq!(ratings.filtered_count(&roots), 1);
    }
}
