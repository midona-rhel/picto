-- Disposable writer-local index, rebuilt from canonical attempts on open.
-- Outcome IDs let cascaded deletes remove their original attribution even
-- after the parent query/run has disappeared. Members deduplicate redownloads.
CREATE TEMP TABLE subscription_outcome (
    kind INTEGER NOT NULL, id INTEGER NOT NULL,
    query_id INTEGER NOT NULL, subscription_id INTEGER NOT NULL,
    source_id INTEGER NOT NULL, PRIMARY KEY(kind, id)
) WITHOUT ROWID;
CREATE INDEX temp.subscription_outcome_query ON subscription_outcome(query_id);
CREATE TEMP TABLE subscription_outcome_member (
    scope INTEGER NOT NULL, owner_id INTEGER NOT NULL, kind INTEGER NOT NULL,
    source_id INTEGER NOT NULL, refs INTEGER NOT NULL,
    PRIMARY KEY(scope, owner_id, kind, source_id)
) WITHOUT ROWID;
CREATE TEMP TABLE subscription_outcome_total (
    scope INTEGER NOT NULL, owner_id INTEGER NOT NULL, kind INTEGER NOT NULL,
    total INTEGER NOT NULL, PRIMARY KEY(scope, owner_id, kind)
) WITHOUT ROWID;
CREATE TEMP TABLE subscription_counts_dirty (dirty INTEGER NOT NULL);
INSERT INTO subscription_counts_dirty VALUES (0);

CREATE TEMP VIEW subscription_post_outcome AS
SELECT 0 AS kind, a.attempt_id AS id, q.query_id, q.subscription_id,
       a.source_post_id AS source_id
FROM main.source_post_attempt a
JOIN main.subscription_run_query r USING(run_query_id)
JOIN main.subscription_query q USING(query_id)
WHERE a.state = 'added';
CREATE TEMP VIEW subscription_file_outcome AS
SELECT 1 AS kind, f.file_attempt_id AS id, q.query_id, q.subscription_id,
       f.source_item_id AS source_id
FROM main.source_file_attempt f
JOIN main.source_post_attempt a USING(attempt_id)
JOIN main.subscription_run_query r USING(run_query_id)
JOIN main.subscription_query q USING(query_id)
WHERE f.state = 'retained';
CREATE TEMP VIEW subscription_run_outcome AS
SELECT 2 AS kind, r.run_query_id AS id, q.query_id, q.subscription_id,
       r.run_query_id AS source_id
FROM main.subscription_run_query r
JOIN main.subscription_query q USING(query_id)
WHERE r.status = 'succeeded';

-- Bulk initialization precedes the incremental triggers. No per-outcome
-- trigger work or GROUP BY/DISTINCT remains on the workspace read path.
INSERT INTO subscription_outcome SELECT * FROM subscription_post_outcome;
INSERT INTO subscription_outcome SELECT * FROM subscription_file_outcome;
INSERT INTO subscription_outcome SELECT * FROM subscription_run_outcome;
INSERT INTO subscription_outcome_member
SELECT 0, query_id, kind, source_id, COUNT(*) FROM subscription_outcome
GROUP BY query_id, kind, source_id;
INSERT INTO subscription_outcome_member
SELECT 1, subscription_id, 0, source_id, COUNT(*) FROM subscription_outcome
WHERE kind = 0 GROUP BY subscription_id, source_id;
INSERT INTO subscription_outcome_total
SELECT scope, owner_id, kind, COUNT(*) FROM subscription_outcome_member
GROUP BY scope, owner_id, kind;

CREATE TEMP TRIGGER subscription_member_insert AFTER INSERT ON subscription_outcome_member BEGIN
    INSERT INTO subscription_outcome_total VALUES (NEW.scope, NEW.owner_id, NEW.kind, 1)
    ON CONFLICT(scope, owner_id, kind) DO UPDATE SET total = total + 1;
    UPDATE subscription_counts_dirty SET dirty = 1;
END;
CREATE TEMP TRIGGER subscription_member_delete AFTER DELETE ON subscription_outcome_member BEGIN
    UPDATE subscription_outcome_total SET total = total - 1
    WHERE scope = OLD.scope AND owner_id = OLD.owner_id AND kind = OLD.kind;
    DELETE FROM subscription_outcome_total
    WHERE scope = OLD.scope AND owner_id = OLD.owner_id AND kind = OLD.kind AND total = 0;
    UPDATE subscription_counts_dirty SET dirty = 1;
END;
CREATE TEMP TRIGGER subscription_outcome_insert AFTER INSERT ON subscription_outcome BEGIN
    UPDATE subscription_counts_dirty SET dirty = 1;
    INSERT INTO subscription_outcome_member
    SELECT 0, NEW.query_id, NEW.kind, NEW.source_id, 1
    UNION ALL SELECT 1, NEW.subscription_id, 0, NEW.source_id, 1 WHERE NEW.kind = 0
    ON CONFLICT(scope, owner_id, kind, source_id) DO UPDATE SET refs = refs + 1;
END;
CREATE TEMP TRIGGER subscription_outcome_delete AFTER DELETE ON subscription_outcome BEGIN
    UPDATE subscription_counts_dirty SET dirty = 1;
    UPDATE subscription_outcome_member SET refs = refs - 1
    WHERE (scope, owner_id, kind, source_id) IN
        ((0, OLD.query_id, OLD.kind, OLD.source_id),
         (1, OLD.subscription_id, OLD.kind, OLD.source_id));
    DELETE FROM subscription_outcome_member
    WHERE (scope, owner_id, kind, source_id) IN
        ((0, OLD.query_id, OLD.kind, OLD.source_id),
         (1, OLD.subscription_id, OLD.kind, OLD.source_id)) AND refs = 0;
END;
