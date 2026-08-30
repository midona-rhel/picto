# PBI-622: Require Three Characters For Text Search

## Observed Gap

A one- or two-character text query can invoke substring FTS across nearly the entire library, such
as searching for `a`, despite being too broad to be useful.

## Required Behavior

- Trim the text query before measuring it.
- Zero-, one-, and two-character values keep text search inactive and show no search results.
- Three or more characters use the existing deferred substring FTS path.
- Structured filters remain usable without a text query.
- The renderer and backend enforce the same boundary so direct calls cannot issue broad text scans.

## Acceptance

1. Empty, whitespace-only, one-character, and two-character queries do not invoke FTS.
2. A three-character query invokes FTS and returns its normal substring results.
3. Deleting a three-character query back to two clears its text-search result without issuing a
   broad backend query.
4. Tag, folder, rating, lifecycle, and other structured filters are unaffected.
