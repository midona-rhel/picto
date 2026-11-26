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

-- Every sync key that the synced values or the library know, with the
-- number that stands for it in sync_value. local_id is the local entity
-- (item, collection, tag, tag group, folder) that had this key when the
-- synced values last recorded it, so that a deleted or renamed entity is
-- recorded under its old key. A local id is unique across kinds because
-- every kind takes its ids from library_meta.next_local_id.
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
-- disappears. The key
-- starts with the field, so that a change of one field on many entities
-- (a rating or a tag given to 200,000 items) writes few pages.
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
