//! Position keys: strings that sort between their neighbours, so that moving
//! one member of a folder or collection changes only that member's field.
//! Members with equal position keys sort by their sync keys. Key generation
//! is deterministic, so two computers that add at the same place make equal
//! keys.
//!
//! A key is an integer part followed by a fraction, both in the 62 digits
//! `0-9A-Za-z`, which sort in byte order. The first character of the integer
//! part gives its length: `a` to `z` start a positive integer of 1 to 26
//! digits, `Z` down to `A` a negative one of 1 to 26 digits. A key after the
//! last one increments the integer part, so appending to a list makes keys
//! that grow by one character for each factor of 62 in its length. A key
//! between two others uses the fraction. The fraction never ends in `0`.

const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn digit(byte: u8) -> Option<usize> {
    DIGITS.iter().position(|value| *value == byte)
}

/// A fraction strictly between the fractions `low` and `high` (`None` is
/// the end).
fn midpoint(low: &[u8], high: Option<&[u8]>) -> Option<Vec<u8>> {
    if let Some(high) = high {
        let shared = high
            .iter()
            .enumerate()
            .take_while(|(index, byte)| low.get(*index).copied().unwrap_or(DIGITS[0]) == **byte)
            .count();
        if shared > 0 {
            let mut key = high[..shared].to_vec();
            key.extend(midpoint(low.get(shared..).unwrap_or_default(), Some(&high[shared..]))?);
            return Some(key);
        }
    }
    let low_digit = match low.first() {
        Some(byte) => digit(*byte)?,
        None => 0,
    };
    let high_digit = match high {
        Some(high) => digit(*high.first()?)?,
        None => DIGITS.len(),
    };
    if high_digit > low_digit + 1 {
        return Some(vec![DIGITS[(low_digit + high_digit).div_ceil(2)]]);
    }
    if let Some(high) = high.filter(|high| high.len() > 1) {
        return Some(high[..1].to_vec());
    }
    let mut key = vec![DIGITS[low_digit]];
    key.extend(midpoint(low.get(1..).unwrap_or_default(), None)?);
    Some(key)
}

/// The length of the integer part that starts with `head`, head included.
fn integer_length(head: u8) -> Option<usize> {
    match head {
        b'a'..=b'z' => Some(usize::from(head - b'a') + 2),
        b'A'..=b'Z' => Some(usize::from(b'Z' - head) + 2),
        _ => None,
    }
}

/// Splits a key into its integer part and its fraction, or `None` when it
/// is not a key in this form.
fn split(key: &[u8]) -> Option<(&[u8], &[u8])> {
    let length = integer_length(*key.first()?)?;
    if key.len() < length || key[1..].iter().any(|byte| digit(*byte).is_none()) {
        return None;
    }
    let (integer, fraction) = key.split_at(length);
    (fraction.last() != Some(&DIGITS[0])).then_some((integer, fraction))
}

/// The next integer part, or `None` after the largest one.
fn increment(integer: &[u8]) -> Option<Vec<u8>> {
    let mut digits = integer[1..].to_vec();
    for index in (0..digits.len()).rev() {
        let next = digit(digits[index])? + 1;
        if next < DIGITS.len() {
            digits[index] = DIGITS[next];
            let mut key = vec![integer[0]];
            key.extend(digits);
            return Some(key);
        }
        digits[index] = DIGITS[0];
    }
    match integer[0] {
        b'z' => None,
        b'Z' => Some(vec![b'a', DIGITS[0]]),
        head => {
            let head = head + 1;
            if head > b'a' {
                digits.push(DIGITS[0]);
            } else {
                digits.pop();
            }
            let mut key = vec![head];
            key.extend(digits);
            Some(key)
        }
    }
}

/// The previous integer part, or `None` before the smallest one.
fn decrement(integer: &[u8]) -> Option<Vec<u8>> {
    let last = DIGITS[DIGITS.len() - 1];
    let mut digits = integer[1..].to_vec();
    for index in (0..digits.len()).rev() {
        let value = digit(digits[index])?;
        if value > 0 {
            digits[index] = DIGITS[value - 1];
            let mut key = vec![integer[0]];
            key.extend(digits);
            return Some(key);
        }
        digits[index] = last;
    }
    match integer[0] {
        b'A' => None,
        b'a' => Some(vec![b'Z', last]),
        head => {
            let head = head - 1;
            if head < b'Z' {
                digits.push(last);
            } else {
                digits.pop();
            }
            let mut key = vec![head];
            key.extend(digits);
            Some(key)
        }
    }
}

/// A key strictly between `low` and `high`, where `None` (or an empty key,
/// which sorts first) for `low` is the start and `None` for `high` the end.
/// Returns `None` when `low` does not sort before `high` or a key is not in
/// the form above.
pub(crate) fn key_between(low: Option<&str>, high: Option<&str>) -> Option<String> {
    let low = low.filter(|low| !low.is_empty());
    if let (Some(low), Some(high)) = (low, high) {
        if low >= high {
            return None;
        }
    }
    let key = match (low.map(str::as_bytes), high.map(str::as_bytes)) {
        (None, None) => vec![b'a', DIGITS[0]],
        (None, Some(high)) => {
            let (integer, fraction) = split(high)?;
            if integer[0] == b'A' && integer[1..].iter().all(|byte| *byte == DIGITS[0]) {
                let mut key = integer.to_vec();
                key.extend(midpoint(b"", Some(fraction))?);
                key
            } else if !fraction.is_empty() {
                integer.to_vec()
            } else {
                decrement(integer)?
            }
        }
        (Some(low), None) => {
            let (integer, fraction) = split(low)?;
            match increment(integer) {
                Some(key) => key,
                None => {
                    let mut key = integer.to_vec();
                    key.extend(midpoint(fraction, None)?);
                    key
                }
            }
        }
        (Some(low), Some(high)) => {
            let (low_integer, low_fraction) = split(low)?;
            let (high_integer, high_fraction) = split(high)?;
            if low_integer == high_integer {
                let mut key = low_integer.to_vec();
                key.extend(midpoint(low_fraction, Some(high_fraction))?);
                key
            } else {
                match increment(low_integer).filter(|key| key.as_slice() < high) {
                    Some(key) => key,
                    None => {
                        let mut key = low_integer.to_vec();
                        key.extend(midpoint(low_fraction, None)?);
                        key
                    }
                }
            }
        }
    };
    String::from_utf8(key).ok()
}

/// `count` keys in order strictly between `low` and `high`. A run at the
/// end or the start of a list counts on from its neighbour; a run between
/// two neighbours is split in halves, so the keys stay short.
pub(crate) fn keys_between(low: Option<&str>, high: Option<&str>, count: usize) -> Option<Vec<String>> {
    let low = low.filter(|low| !low.is_empty());
    match (count, low, high) {
        (0, _, _) => Some(Vec::new()),
        (1, _, _) => Some(vec![key_between(low, high)?]),
        (_, _, None) => {
            let mut keys: Vec<String> = Vec::with_capacity(count);
            for _ in 0..count {
                let key = key_between(keys.last().map(String::as_str).or(low), None)?;
                keys.push(key);
            }
            Some(keys)
        }
        (_, None, Some(_)) => {
            let mut keys: Vec<String> = Vec::with_capacity(count);
            for _ in 0..count {
                let key = key_between(None, keys.last().map(String::as_str).or(high))?;
                keys.push(key);
            }
            keys.reverse();
            Some(keys)
        }
        (_, Some(_), Some(_)) => {
            let middle = key_between(low, high)?;
            let half = count / 2;
            let mut keys = keys_between(low, Some(&middle), half)?;
            keys.push(middle.clone());
            keys.extend(keys_between(Some(&middle), high, count - half - 1)?);
            Some(keys)
        }
    }
}

/// One member of a list, in the library's order: its sync key and the
/// position key that the synced values give it in this list, if any.
pub(crate) struct Member {
    pub key: String,
    pub position: Option<String>,
}

/// Decides which members of `members` keep their position keys and gives the
/// others new keys between their neighbours. `lower` and `upper` are the
/// (position, sync key) of the members just outside `members`, which keep
/// their keys. Returns (index, new position key) for each member that needs a
/// new key, or `None` when no key fits between the bounds.
///
/// The members that keep their keys are the longest run that is already in
/// order. When a member must go between two kept neighbours with equal keys,
/// the right neighbour also gets a new key, so that the new key can sort
/// strictly between them; only that case re-keys a tie.
pub(crate) fn assign(
    members: &[Member],
    lower: Option<(&str, &str)>,
    upper: Option<(&str, &str)>,
) -> Option<Vec<(usize, String)>> {
    let candidates = members
        .iter()
        .enumerate()
        .filter_map(|(index, member)| {
            let position = member.position.as_deref()?;
            let value = (position, member.key.as_str());
            (lower.is_none_or(|lower| value > lower) && upper.is_none_or(|upper| value < upper))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let value = |index: usize| {
        (
            members[index].position.as_deref().unwrap_or_default(),
            members[index].key.as_str(),
        )
    };
    let mut kept = vec![false; members.len()];
    for index in longest_increasing(&candidates, value) {
        kept[index] = true;
    }
    // A gap between two kept members with equal keys cannot take a new key,
    // so the right one of them gets a new key too.
    let mut previous = lower.map(|lower| lower.0);
    let mut gap = false;
    for index in 0..members.len() {
        if kept[index] {
            let position = members[index].position.as_deref();
            if gap && previous.is_some() && position == previous {
                kept[index] = false;
                continue;
            }
            previous = position;
            gap = false;
        } else {
            gap = true;
        }
    }
    let mut next = vec![upper.map(|upper| upper.0); members.len() + 1];
    for index in (0..members.len()).rev() {
        next[index] = if kept[index] {
            members[index].position.as_deref()
        } else {
            next[index + 1]
        };
    }
    // Each run of members between two kept ones gets its keys at once.
    let mut previous = lower.map(|lower| lower.0);
    let mut assigned = Vec::new();
    let mut index = 0;
    while index < members.len() {
        if kept[index] {
            previous = members[index].position.as_deref();
            index += 1;
            continue;
        }
        let end = (index..members.len()).find(|end| kept[*end]).unwrap_or(members.len());
        let keys = keys_between(previous, next[end], end - index)?;
        assigned.extend((index..end).zip(keys));
        index = end;
    }
    Some(assigned)
}

/// The indexes of the longest strictly increasing run of `candidates` by
/// `value`, in O(n log n).
fn longest_increasing<'a, V: Ord + 'a>(
    candidates: &[usize],
    value: impl Fn(usize) -> V,
) -> Vec<usize> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![usize::MAX; candidates.len()];
    for (slot, &index) in candidates.iter().enumerate() {
        let current = value(index);
        let at = tails.partition_point(|&tail| value(candidates[tail]) < current);
        if at > 0 {
            previous[slot] = tails[at - 1];
        }
        if at == tails.len() {
            tails.push(slot);
        } else {
            tails[at] = slot;
        }
    }
    let mut run = Vec::with_capacity(tails.len());
    let mut slot = tails.last().copied().unwrap_or(usize::MAX);
    while slot != usize::MAX {
        run.push(candidates[slot]);
        slot = previous[slot];
    }
    run.reverse();
    run
}

/// A key after `low`, for the rare case that no key fits between two
/// neighbours. The full comparison after the write then shows whether the
/// order still matches.
pub(crate) fn key_after(low: Option<&str>) -> String {
    key_between(low, None).unwrap_or_else(|| key_between(None, None).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys made for appends, prepends and inserts between neighbours sort
    /// in order and stay short.
    #[test]
    fn keys_sort_between_their_neighbours_and_stay_short() {
        let appended = keys_between(None, None, 100_000).unwrap();
        assert!(appended.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(appended.iter().all(|key| key.len() <= 4));
        let inserted = keys_between(Some(&appended[10]), Some(&appended[11]), 100_000).unwrap();
        assert!(inserted.first().unwrap() > &appended[10] && inserted.last().unwrap() < &appended[11]);
        assert!(inserted.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(inserted.iter().all(|key| key.len() <= 10));
        let prepended = keys_between(None, Some(&appended[0]), 1_000).unwrap();
        assert!(prepended.windows(2).all(|pair| pair[0] < pair[1]) && prepended.last().unwrap() < &appended[0]);
        assert_eq!(key_between(Some("a1"), Some("a1")), None);
    }
}
