/**
 * Search ranking shared by every flat search list: an exact match first, then
 * a prefix, then a match at the start of a word (after a character that is not
 * a letter or digit, such as `_`, `(` or a space), then any other substring.
 * The backend ranks tag searches with the same rule (`match_rank` in
 * `picto-library/src/mutation/tags.rs`).
 */

const WORD_CHARACTER = /[\p{L}\p{N}]/u;

/** 0 exact, 1 prefix, 2 word start, 3 substring; null when absent. Lowercased input. */
export function matchRank(text: string, needle: string): number | null {
  if (text === needle) return 0;
  if (text.startsWith(needle)) return 1;
  let found = false;
  for (let index = text.indexOf(needle); index >= 0; index = text.indexOf(needle, index + 1)) {
    found = true;
    // A non-BMP predecessor occupies two UTF-16 code units.
    const paired = (text.codePointAt(index - 2) ?? 0) > 0xffff;
    const before = text.slice(index - (paired ? 2 : 1), index);
    if (index > 0 && !WORD_CHARACTER.test(before)) return 2;
  }
  return found ? 3 : null;
}

/**
 * The items matching `query` in any of their `keys`, best rank first. Items
 * of the same rank keep their input order; a blank query returns the input.
 */
export function rankMatches<T>(items: readonly T[], query: string, keys: (item: T) => readonly string[]): T[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...items];
  const ranked: Array<{ item: T; rank: number; index: number }> = [];
  items.forEach((item, index) => {
    let best: number | null = null;
    for (const key of keys(item)) {
      const rank = matchRank(key.toLowerCase(), needle);
      if (rank != null && (best == null || rank < best)) best = rank;
    }
    if (best != null) ranked.push({ item, rank: best, index });
  });
  ranked.sort((left, right) => left.rank - right.rank || left.index - right.index);
  return ranked.map(({ item }) => item);
}
