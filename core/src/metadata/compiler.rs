use std::collections::HashSet;
use std::sync::Arc;

use crate::sqlite::SqliteDatabase;
use crate::sqlite::projections;

pub(crate) async fn compile_metadata_projections(
    db: &Arc<SqliteDatabase>,
    dirty_file_ids: &HashSet<i64>,
    rebuild_all: bool,
) -> Result<(), String> {
    let pre_version = db
        .manifest
        .published_artifact_version("metadata_projection") as i64;
    let projection_version = pre_version + 1;

    if rebuild_all {
        db.with_conn(move |conn| {
            let mut stmt = conn.prepare_cached("SELECT file_id FROM file")?;
            let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            let file_ids: rusqlite::Result<Vec<i64>> = rows.collect();
            let file_ids = file_ids?;
            projections::build_projections_batch(conn, &file_ids, projection_version)
        })
        .await?;
    } else if !dirty_file_ids.is_empty() {
        let file_ids: Vec<i64> = dirty_file_ids.iter().copied().collect();
        db.with_conn(move |conn| {
            projections::build_projections_batch(conn, &file_ids, projection_version)
        })
        .await?;
    } else {
        return Ok(());
    }

    Ok(())
}
