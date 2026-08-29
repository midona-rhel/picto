use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest as Sha2Digest, Sha256};
use tokio::sync::Semaphore;

static BACKGROUND_FILE_HASH: Semaphore = Semaphore::const_new(1);

pub fn get_hash_from_bytes(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

pub fn get_hash_from_path(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_vec())
}

pub async fn get_hash_from_path_background(path: PathBuf) -> std::io::Result<Vec<u8>> {
    let _permit = BACKGROUND_FILE_HASH
        .acquire()
        .await
        .expect("background file-hash semaphore is never closed");
    tokio::task::spawn_blocking(move || get_hash_from_path(&path))
        .await
        .map_err(std::io::Error::other)?
}
