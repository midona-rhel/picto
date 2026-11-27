-- The tables of the sync file, a SQLite database next to the library
-- database. The statements create only what is missing, so they run every
-- time the file is opened.
--
-- A change record is (kind, key, field, value, timestamp). The timestamp is a
-- hybrid logical clock: wall time in milliseconds, a counter and the device
-- id of the computer that recorded it, compared in that order.

-- The device id and the clock of this library, the library revision that
-- the recorder has recorded up to, and the revision of the last library
-- write that wrote synced values into the library.
CREATE TABLE IF NOT EXISTS sync_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    device_id TEXT NOT NULL,
    clock_ms INTEGER NOT NULL,
    clock_counter INTEGER NOT NULL,
    recorded_revision INTEGER NOT NULL,
    projected_revision INTEGER NOT NULL DEFAULT 0
) STRICT;

-- Every sync key that the synced values or the library know, with the
-- number that stands for it in sync_value. local_id is the local entity
-- (item, collection, tag, tag group, folder) that had this key when the
-- recorder last recorded it; it stays after the entity is deleted, so that
-- an item that an undo brings back under the same local id is recognized. A
-- local id is unique across kinds and never used again, because every kind
-- takes its ids from library_meta.next_local_id.
CREATE TABLE IF NOT EXISTS sync_identity (
    entity INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    local_id INTEGER UNIQUE,
    UNIQUE(kind, key)
) STRICT;

-- The number that stands for each device id in sync_value.
CREATE TABLE IF NOT EXISTS sync_device (
    device INTEGER PRIMARY KEY,
    device_id TEXT NOT NULL UNIQUE
) STRICT;

-- The synced values: for each field of each entity, the change record with
-- the latest timestamp. `value` is JSON. A field `tag:<name>` or
-- `folder:<uuid>` is stored as the field `tag` or `folder` with the tag or
-- folder as `target`; other fields have the target 0. For the other fields
-- `reference` is the entity that the value names. Targets and references
-- find the entities that refer to a key when that key's entity appears or
-- disappears. The key starts with the field, so that a change of one field
-- on many entities (a rating or a tag given to 200,000 items) writes few
-- pages.
CREATE TABLE IF NOT EXISTS sync_value (
    entity INTEGER NOT NULL,
    field TEXT NOT NULL,
    target INTEGER NOT NULL,
    value TEXT NOT NULL,
    clock_ms INTEGER NOT NULL,
    clock_counter INTEGER NOT NULL,
    device INTEGER NOT NULL,
    reference INTEGER,
    PRIMARY KEY(field, target, entity)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS idx_sync_value_entity ON sync_value(entity);
CREATE INDEX IF NOT EXISTS idx_sync_value_reference
    ON sync_value(reference) WHERE reference IS NOT NULL;

-- The change records of this library that no change file holds yet, the
-- latest one for each field, stored as in sync_value. Their device is this
-- library's.
CREATE TABLE IF NOT EXISTS sync_outbox (
    entity INTEGER NOT NULL,
    field TEXT NOT NULL,
    target INTEGER NOT NULL,
    value TEXT NOT NULL,
    clock_ms INTEGER NOT NULL,
    clock_counter INTEGER NOT NULL,
    PRIMARY KEY(field, target, entity)
) WITHOUT ROWID, STRICT;

-- The entities whose synced values still have to be written into the
-- library. State 0: write what the synced values show. State 1: the same,
-- with the entity counted as visible although the synced values delete it;
-- this is an item or collection that came back in the library under its old
-- local id, as an undo of Delete Permanently brings it back. State 2: such an
-- entity was written; the recorder records its full state next.
CREATE TABLE IF NOT EXISTS sync_pending (
    entity INTEGER PRIMARY KEY,
    state INTEGER NOT NULL CHECK (state IN (0, 1, 2))
) STRICT;

-- Copies of the library's membership bitmaps and ordered lists as the
-- recorder last recorded them. Comparing a changed row of the library with
-- its copy gives the roots that entered or left a lifecycle, rating, tag,
-- folder or collection, and the order a list had before.
CREATE TABLE IF NOT EXISTS recorded_bitmap (
    domain INTEGER NOT NULL,
    key_id INTEGER NOT NULL,
    high_bits INTEGER NOT NULL,
    checksum BLOB NOT NULL,
    payload BLOB NOT NULL,
    PRIMARY KEY(domain, key_id, high_bits)
) WITHOUT ROWID, STRICT;
CREATE TABLE IF NOT EXISTS recorded_order (
    owner_kind INTEGER NOT NULL,
    owner_id INTEGER NOT NULL,
    chunk_index INTEGER NOT NULL,
    checksum BLOB NOT NULL,
    payload BLOB NOT NULL,
    PRIMARY KEY(owner_kind, owner_id, chunk_index)
) WITHOUT ROWID, STRICT;
