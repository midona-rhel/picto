-- Foreground tasks: long user-started jobs (imports) that report progress and
-- can be paused, stopped, or stopped and undone. Idempotent; runs at create
-- and in the migration that introduced it.
CREATE TABLE IF NOT EXISTS task (
    task_id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('import')),
    title TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'running', 'paused', 'reverting', 'finished', 'stopped', 'reverted'
    )),
    -- What the task was asked to do, so an interrupted scan can be resumed.
    input_json TEXT NOT NULL,
    scan_complete INTEGER NOT NULL DEFAULT 0 CHECK (scan_complete IN (0, 1)),
    -- Files found so far; final once the scan is complete.
    total_units INTEGER NOT NULL DEFAULT 0,
    -- Files that became new items, and files already in the library.
    added_units INTEGER NOT NULL DEFAULT 0,
    existing_units INTEGER NOT NULL DEFAULT 0,
    revertible INTEGER NOT NULL DEFAULT 1 CHECK (revertible IN (0, 1)),
    created_at_ms INTEGER NOT NULL,
    finished_at_ms INTEGER,
    dismissed INTEGER NOT NULL DEFAULT 0 CHECK (dismissed IN (0, 1)),
    -- Why the task could not continue, when it stopped on its own.
    error TEXT
) STRICT;

-- Roots a task created (not ones it reused), which "stop and undo" removes.
CREATE TABLE IF NOT EXISTS task_created_root (
    task_id INTEGER NOT NULL REFERENCES task(task_id) ON DELETE CASCADE,
    root_id INTEGER NOT NULL,
    PRIMARY KEY(task_id, root_id)
) WITHOUT ROWID, STRICT;

-- Tags a task added to roots that already existed, which undo takes back.
CREATE TABLE IF NOT EXISTS task_added_tag (
    task_id INTEGER NOT NULL REFERENCES task(task_id) ON DELETE CASCADE,
    root_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY(task_id, root_id, tag_id)
) WITHOUT ROWID, STRICT;
