# PBI-621: Preserve Smart Names When Merging Subscription Results

## Observed Gap

Byte-exact ingestion prefers a meaningful human name over empty, numeric, hash-like, or generated
names. Merging existing subscription results does not consistently use that same choice, so a merge
can retain or introduce a weaker name than direct ingestion would select.

## Required Behavior

- One shared name-quality policy chooses between existing and incoming media names.
- Meaningful human names replace weak names; weak names never replace meaningful names.
- Collection root names remain collection-owned and are not replaced by member names.
- Merge and byte-exact ingest apply the same rule without transferring unrelated collection
  metadata.

## Acceptance

1. Every ordering of empty, numeric, hash-like, generated, and meaningful names produces the same
   winner through merge and byte-exact ingestion.
2. Merging duplicate standalone media upgrades a weak retained name when a meaningful name exists.
3. Merging members or collections does not overwrite a collection root name.
4. Tags follow the existing duplicate metadata policy independently of the name decision.
