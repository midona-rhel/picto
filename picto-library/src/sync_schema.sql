-- The tables of cloud sync. The statements create only what is missing, so
-- they run when a library is created and in every migration.
--
-- A change record is (kind, key, field, value, timestamp). The timestamp is a
-- hybrid logical clock: wall time in milliseconds, a counter and the device
-- id of the computer that recorded it, compared in that order.

-- The device id and the clock of this library. The library records its
-- changes only while this row exists.
CREATE TABLE IF NOT EXISTS sync_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    device_id TEXT NOT NULL,
    clock_ms INTEGER NOT NULL,
    clock_counter INTEGER NOT NULL
) STRICT;

-- The sync key of each local entity (item, collection, tag, tag group,
-- folder) as the synced values last knew it. A local id is unique across
-- kinds because every kind takes its ids from library_meta.next_local_id.
CREATE TABLE IF NOT EXISTS sync_identity (
    local_id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    UNIQUE(kind, key)
) STRICT;

-- The synced values: for each field of each entity, the change record with
-- the latest timestamp. `value` is JSON. `reference` names the entity that
-- the value refers to as `<kind>:<key>`, so that the entities that refer to
-- a key can be found when that key's entity appears or disappears.
CREATE TABLE IF NOT EXISTS sync_value (
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    field TEXT NOT NULL,
    value TEXT NOT NULL,
    clock_ms INTEGER NOT NULL,
    clock_counter INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    reference TEXT,
    PRIMARY KEY(kind, key, field)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS idx_sync_value_reference
    ON sync_value(reference) WHERE reference IS NOT NULL;

-- The change records of this library that no change file holds yet, the
-- latest one for each field.
CREATE TABLE IF NOT EXISTS sync_outbox (
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    field TEXT NOT NULL,
    value TEXT NOT NULL,
    clock_ms INTEGER NOT NULL,
    clock_counter INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    PRIMARY KEY(kind, key, field)
) WITHOUT ROWID, STRICT;
