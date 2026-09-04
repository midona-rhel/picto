//! Isolates cursor SQL and broad FTS result enumeration without changing production behavior.
use picto_library::predicate::TextField;
use roaring::RoaringBitmap;
use rusqlite::{params, Connection, StatementStatus};
use serde_json::json;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::Builder::new()
        .prefix("picto-query-plan-")
        .tempdir()?;
    let connection = Connection::open(directory.path().join("probe.sqlite"))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
         CREATE TABLE library_root(root_id INTEGER PRIMARY KEY, imported_at_ms INTEGER NOT NULL, name TEXT NOT NULL);
         CREATE INDEX idx_root_imported ON library_root(imported_at_ms,root_id);
         CREATE VIRTUAL TABLE root_fts USING fts5(name,notes,urls,source_text,tokenize='trigram',detail='none',columnsize=0);
         BEGIN;
         WITH RECURSIVE ids(id) AS (VALUES(1) UNION ALL SELECT id+1 FROM ids WHERE id<1000000)
         INSERT INTO library_root SELECT id,id,
           (CASE WHEN id%997=0 THEN 'needle' WHEN id%2=0 THEN 'landscape' ELSE 'portrait' END)||printf('-%08d',id)
         FROM ids;
         INSERT INTO root_fts(rowid,name,notes,urls,source_text) SELECT root_id,name,'','','' FROM library_root;
         COMMIT; PRAGMA wal_checkpoint(TRUNCATE);"
    )?;
    let matches = (1u32..=1_000_000)
        .filter(|id| id % 20 > 1 && id.wrapping_mul(2654435761).is_multiple_of(100))
        .collect::<RoaringBitmap>();
    let current = "SELECT root.root_id,root.imported_at_ms FROM library_root root
        WHERE (root.imported_at_ms < ?1 OR (root.imported_at_ms = ?2 AND root.root_id < ?3))
        ORDER BY root.imported_at_ms DESC,root.root_id DESC LIMIT 1024";
    let seek = "SELECT root.root_id,root.imported_at_ms FROM library_root root
        WHERE (root.imported_at_ms,root.root_id) < (?1,?3) AND ?2 IS NOT NULL
        ORDER BY root.imported_at_ms DESC,root.root_id DESC LIMIT 1024";
    let mut reports = Vec::new();
    for cursor in [1_000_001, 100_000] {
        let mut expected = None;
        for (label, sql) in [
            ("current_or_predicate", current),
            ("diagnostic_tuple_seek", seek),
        ] {
            let plan = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?
                .query_map(params![cursor, cursor, cursor], |row| {
                    row.get::<_, String>(3)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let started = Instant::now();
            let mut next = cursor;
            let mut output = Vec::new();
            let mut scans = 0i64;
            let mut steps = 0i64;
            let mut chunks = 0;
            while output.len() < 500 {
                let mut statement = connection.prepare(sql)?;
                let rows = statement
                    .query_map(params![next, next, next], |row| row.get::<_, u32>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                scans += statement.get_status(StatementStatus::FullscanStep) as i64;
                steps += statement.get_status(StatementStatus::VmStep) as i64;
                chunks += 1;
                if rows.is_empty() {
                    break;
                }
                for id in rows {
                    next = id;
                    if matches.contains(id) {
                        output.push(id);
                    }
                    if output.len() == 500 {
                        break;
                    }
                }
            }
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if let Some(ref ids) = expected {
                assert_eq!(&output, ids);
            } else {
                expected = Some(output.clone());
            }
            let report = json!({"case":label,"cursor":cursor,"ms":elapsed_ms,"chunks":chunks,
                "returned":output.len(),"sqlite_fullscan_steps":scans,"sqlite_vm_steps":steps,"plan":plan});
            println!("PLAN {report}");
            reports.push(report);
        }
    }
    let mut fts = Vec::new();
    for term in ["needle", "landscape"] {
        let started = Instant::now();
        let all = picto_library::fts::search(&connection, TextField::Name, term)?;
        let all_ms = started.elapsed().as_secs_f64() * 1000.0;
        let sql = "SELECT rowid FROM root_fts WHERE name LIKE ?1 AND instr(name,?2)>0 LIMIT 500";
        let pattern = format!("%{term}%");
        let plan = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?
            .query_map(params![pattern, term], |row| row.get::<_, String>(3))?
            .collect::<Result<Vec<_>, _>>()?;
        let started = Instant::now();
        let limited = connection
            .prepare(sql)?
            .query_map(params![pattern, term], |row| row.get::<_, u32>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let limited_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert!(limited.iter().all(|id| all.contains(*id)));
        let report = json!({"term":term,"all_matches":all.len(),"all_matches_ms":all_ms,
            "limited_matches":limited.len(),"limit_500_ms":limited_ms,"plan":plan,
            "note":"LIMIT comparison omits exact totals and global sorting; not an equivalent replacement"});
        println!("FTS {report}");
        fts.push(report);
    }
    println!(
        "REPORT {}",
        json!({"rows":1_000_000,"pagination":reports,"fts":fts})
    );
    drop(connection);
    directory.close()?;
    println!("CLEANED");
    Ok(())
}
