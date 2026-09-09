-- The one derived-state journal. Triggers record every root whose resident
-- browsing facts (name, cover, dates, counts, size, cover file facts) changed;
-- the publishing transaction settles the resident model from it before commit.
-- The triggers insert only absent IDs instead of relying on OR IGNORE: a
-- triggering statement's own conflict policy would override the trigger's.
CREATE TABLE IF NOT EXISTS browsing_dirty_root (
    root_id INTEGER PRIMARY KEY
) STRICT;

CREATE TRIGGER IF NOT EXISTS browsing_root_insert AFTER INSERT ON library_root BEGIN
    INSERT INTO browsing_dirty_root SELECT NEW.root_id
        WHERE NOT EXISTS (SELECT 1 FROM browsing_dirty_root WHERE root_id = NEW.root_id);
END;
CREATE TRIGGER IF NOT EXISTS browsing_root_delete AFTER DELETE ON library_root BEGIN
    INSERT INTO browsing_dirty_root SELECT OLD.root_id
        WHERE NOT EXISTS (SELECT 1 FROM browsing_dirty_root WHERE root_id = OLD.root_id);
END;
CREATE TRIGGER IF NOT EXISTS browsing_root_update AFTER UPDATE ON library_root
WHEN OLD.root_id IS NOT NEW.root_id OR OLD.name IS NOT NEW.name
  OR OLD.cover_media_id IS NOT NEW.cover_media_id
  OR OLD.imported_at_ms IS NOT NEW.imported_at_ms
  OR OLD.captured_at_ms IS NOT NEW.captured_at_ms
  OR OLD.modified_at_ms IS NOT NEW.modified_at_ms
  OR OLD.media_count IS NOT NEW.media_count
  OR OLD.total_size_bytes IS NOT NEW.total_size_bytes
BEGIN
    INSERT INTO browsing_dirty_root SELECT OLD.root_id
        WHERE NOT EXISTS (SELECT 1 FROM browsing_dirty_root WHERE root_id = OLD.root_id);
    INSERT INTO browsing_dirty_root SELECT NEW.root_id
        WHERE NOT EXISTS (SELECT 1 FROM browsing_dirty_root WHERE root_id = NEW.root_id);
END;
CREATE TRIGGER IF NOT EXISTS browsing_media_file_update AFTER UPDATE OF file_id ON media_item
WHEN OLD.file_id IS NOT NEW.file_id BEGIN
    INSERT INTO browsing_dirty_root
        SELECT root_id FROM library_root WHERE cover_media_id = NEW.media_id
          AND root_id NOT IN (SELECT root_id FROM browsing_dirty_root);
END;
CREATE TRIGGER IF NOT EXISTS browsing_cover_facts_update AFTER UPDATE ON media_file
WHEN OLD.content_hash IS NOT NEW.content_hash OR OLD.mime IS NOT NEW.mime
  OR OLD.width IS NOT NEW.width OR OLD.height IS NOT NEW.height
  OR OLD.duration_ms IS NOT NEW.duration_ms OR OLD.frame_count IS NOT NEW.frame_count
BEGIN
    INSERT INTO browsing_dirty_root
        SELECT root.root_id FROM library_root root
        JOIN media_item media ON media.media_id = root.cover_media_id
        WHERE media.file_id = NEW.file_id
          AND root.root_id NOT IN (SELECT root_id FROM browsing_dirty_root);
END;
