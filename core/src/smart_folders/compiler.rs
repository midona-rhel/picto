use std::sync::Arc;

use crate::sqlite::SqliteDatabase;
use crate::sqlite::bitmaps::BitmapKey;

use super::db::{SmartFolderPredicate, compile_predicate, get_smart_folder, list_smart_folders};

pub(crate) async fn compile_all_smart_folders(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let smart_folders = list_smart_folders(conn)?;
        for smart_folder in smart_folders {
            let predicate: SmartFolderPredicate =
                match serde_json::from_str(&smart_folder.predicate_json) {
                    Ok(predicate) => predicate,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse smart folder {} predicate: {e}",
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
        let smart_folder =
            get_smart_folder(conn, smart_folder_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let predicate: SmartFolderPredicate = serde_json::from_str(&smart_folder.predicate_json)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let bitmap = compile_predicate(conn, &predicate, &bitmaps)?;
        bitmaps.set(BitmapKey::SmartFolder(smart_folder_id), bitmap);
        Ok(())
    })
    .await?;
    Ok(())
}
