-- A media item merged into another (two items for one file) leaves its stable
-- key behind as an alias of the item that stays. Re-importing the old item, or
-- importing a Picto Pack that carries it, then finds the item that stays
-- instead of bringing the merged-away item back.
CREATE TABLE IF NOT EXISTS library_item_alias (
    stable_key TEXT PRIMARY KEY,
    local_id INTEGER NOT NULL REFERENCES library_item(local_id) ON DELETE CASCADE
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS idx_library_item_alias_item ON library_item_alias(local_id);

-- A tag merged into another leaves a row naming the tag it went to, with the
-- revision of the merge. The sync recorder reads it to record that the old
-- name forwards to the new one.
CREATE TABLE IF NOT EXISTS tag_merge (
    source_tag_id INTEGER PRIMARY KEY,
    destination_tag_id INTEGER NOT NULL,
    revision INTEGER NOT NULL
) STRICT;
