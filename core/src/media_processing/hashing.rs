use sha2::{Digest as Sha2Digest, Sha256};

pub fn get_hash_from_bytes(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}
