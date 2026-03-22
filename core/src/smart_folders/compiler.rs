use std::sync::Arc;

use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::SqliteDatabase;

use super::db::{
    build_effective_predicate_for_smart_folder, compile_predicate, get_smart_folder,
    list_smart_folders,
};

pub(crate) async fn compile_all_smart_folders(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let smart_folders = list_smart_folders(conn)?;
        for smart_folder in smart_folders {
            let predicate = match build_effective_predicate_for_smart_folder(
                conn,
                smart_folder.smart_folder_id,
            ) {
                Ok(predicate) => predicate,
                Err(e) => {
                    tracing::warn!(
                        "Failed to resolve smart folder {} effective predicate: {e}",
                        smart_folder.smart_folder_id
                    );
                    continue;
                }
            };

            match compile_predicate(conn, &predicate, &bitmaps) {
                Ok(bitmap) => {
                    bitmaps.set(BitmapKey::SmartFolder(smart_folder.smart_folder_id), bitmap);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to compile smart folder {}: {e}",
                        smart_folder.smart_folder_id
                    );
                }
            }
        }

        Ok(())
    })
    .await?;
    Ok(())
}

pub(crate) async fn compile_smart_folder(
    db: &Arc<SqliteDatabase>,
    smart_folder_id: i64,
) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        if get_smart_folder(conn, smart_folder_id)?.is_none() {
            tracing::debug!(
                smart_folder_id,
                "Smart folder no longer exists, skipping compilation"
            );
            bitmaps.remove_key(&BitmapKey::SmartFolder(smart_folder_id));
            return Ok(());
        }
        let predicate = build_effective_predicate_for_smart_folder(conn, smart_folder_id)?;
        let bitmap = compile_predicate(conn, &predicate, &bitmaps)?;
        bitmaps.set(BitmapKey::SmartFolder(smart_folder_id), bitmap);
        Ok(())
    })
    .await?;
    Ok(())
}
