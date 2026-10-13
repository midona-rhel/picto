-- A media item merged into another (two items for one file) leaves its stable
-- key behind as an alias of the item that stays, so re-importing the old item,
-- a Picto Pack that carries it, or a cloud copy resolves to the survivor
-- instead of bringing the merged-away item back.
CREATE TABLE IF NOT EXISTS library_item_alias (
    stable_key TEXT PRIMARY KEY,
    local_id INTEGER NOT NULL REFERENCES library_item(local_id) ON DELETE CASCADE
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS idx_library_item_alias_item ON library_item_alias(local_id);
