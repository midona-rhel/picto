use crate::blob_store::BlobStore;
use crate::sqlite::SqliteDatabase;

pub struct LifecycleController;

impl LifecycleController {
    pub async fn update_file_status(
        db: &SqliteDatabase,
        hash: String,
        status: i64,
    ) -> Result<(), String> {
        db.update_file_status(&hash, status).await?;
        Ok(())
    }

    pub async fn delete_file(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        hash: String,
    ) -> Result<(), String> {
        db.delete_file_by_hash(&hash).await?;
        blob_store.delete(&hash).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn wipe_all_files(db: &SqliteDatabase, blob_store: &BlobStore) -> Result<(), String> {
        db.wipe_all_files().await?;
        blob_store.wipe().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn delete_files(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        hashes: Vec<String>,
    ) -> Result<usize, String> {
        let count = hashes.len();
        for hash in &hashes {
            db.delete_file_by_hash(hash).await?;
            blob_store.delete(hash).map_err(|e| e.to_string())?;
        }
        Ok(count)
    }
}
