//! Duplicate file detection using perceptual hashing.
//!
//! Uses the `img_hash` crate for perceptual hashing (DCT-based).
//! Duplicate relationships are stored in SQLite (see sqlite/duplicates.rs).

use img_hash::{HasherConfig, ImageHash};

// --- Perceptual hash ---

/// Default Hamming distance threshold for "likely duplicate" (0 = identical, lower = more similar).
/// 32 at 16x16 (256-bit hash) is proportional to the old 8 at 8x8 (64-bit hash).
pub const DEFAULT_DISTANCE_THRESHOLD: u32 = 32;

/// Hash size per dimension (16x16 = 256-bit hash).
const HASH_SIZE: u32 = 16;

/// Generate a perceptual hash for an image from raw bytes (decodes internally).
pub fn compute_phash(image_data: &[u8]) -> Result<ImageHash, image::ImageError> {
    let img = image::load_from_memory(image_data)?;
    compute_phash_from_image(&img)
}

/// Generate a perceptual hash from a pre-decoded image (avoids redundant decode).
pub fn compute_phash_from_image(img: &image::DynamicImage) -> Result<ImageHash, image::ImageError> {
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());

    let hasher = HasherConfig::new()
        .hash_size(HASH_SIZE, HASH_SIZE)
        .to_hasher();

    Ok(hasher.hash_image(
        &img_hash::image::RgbaImage::from_raw(w, h, rgba.into_raw())
            .expect("Failed to create image for hashing"),
    ))
}

/// Compute phash and return as base64 string for DB storage.
pub fn compute_phash_base64(image_data: &[u8]) -> Result<String, image::ImageError> {
    let hash = compute_phash(image_data)?;
    Ok(hash.to_base64())
}

/// Compute phash from a pre-decoded image and return as base64 string.
pub fn compute_phash_base64_from_image(img: &image::DynamicImage) -> Result<String, image::ImageError> {
    let hash = compute_phash_from_image(img)?;
    Ok(hash.to_base64())
}

// --- BK-tree for Hamming distance ---

/// A BK-tree node for efficient near-neighbor search in Hamming space.
struct BkNode {
    file_hash: String,
    phash: ImageHash<Vec<u8>>,
    children: Vec<(u32, BkNode)>,
}

/// BK-tree for O(n log n) near-duplicate detection.
pub struct BkTree {
    root: Option<BkNode>,
    size: usize,
}

impl BkTree {
    pub fn new() -> Self {
        Self {
            root: None,
            size: 0,
        }
    }

    pub fn insert(&mut self, file_hash: String, phash: ImageHash<Vec<u8>>) {
        self.size += 1;
        match self.root {
            None => {
                self.root = Some(BkNode {
                    file_hash,
                    phash,
                    children: Vec::new(),
                });
            }
            Some(ref mut root) => {
                Self::insert_into(root, file_hash, phash);
            }
        }
    }

    fn insert_into(node: &mut BkNode, file_hash: String, phash: ImageHash<Vec<u8>>) {
        let dist = node.phash.dist(&phash);
        for (child_dist, child_node) in &mut node.children {
            if *child_dist == dist {
                Self::insert_into(child_node, file_hash, phash);
                return;
            }
        }
        node.children.push((
            dist,
            BkNode {
                file_hash,
                phash,
                children: Vec::new(),
            },
        ));
    }

    /// Find all entries within `threshold` Hamming distance of `query`.
    pub fn find_within(&self, query: &ImageHash<Vec<u8>>, threshold: u32) -> Vec<(String, u32)> {
        let mut results = Vec::new();
        if let Some(ref root) = self.root {
            Self::search(root, query, threshold, &mut results);
        }
        results
    }

    fn search(
        node: &BkNode,
        query: &ImageHash<Vec<u8>>,
        threshold: u32,
        results: &mut Vec<(String, u32)>,
    ) {
        let dist = node.phash.dist(query);
        if dist <= threshold {
            results.push((node.file_hash.clone(), dist));
        }
        let lo = dist.saturating_sub(threshold);
        let hi = dist + threshold;
        for (child_dist, child_node) in &node.children {
            if *child_dist >= lo && *child_dist <= hi {
                Self::search(child_node, query, threshold, results);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(pixel: [u8; 4]) -> ImageHash<Vec<u8>> {
        let hasher = HasherConfig::new()
            .hash_size(HASH_SIZE, HASH_SIZE)
            .to_hasher();
        let img = img_hash::image::RgbaImage::from_pixel(
            HASH_SIZE,
            HASH_SIZE,
            img_hash::image::Rgba(pixel),
        );
        let hash = hasher.hash_image(&img);
        let b64 = hash.to_base64();
        ImageHash::<Vec<u8>>::from_base64(&b64).unwrap()
    }

    #[test]
    fn test_bk_tree_no_match_beyond_threshold() {
        let hasher = HasherConfig::new()
            .hash_size(HASH_SIZE, HASH_SIZE)
            .to_hasher();

        let mut img_a = img_hash::image::RgbaImage::new(64, 64);
        for (x, y, pixel) in img_a.enumerate_pixels_mut() {
            let val = if (x / HASH_SIZE + y / HASH_SIZE) % 2 == 0 {
                255
            } else {
                0
            };
            *pixel = img_hash::image::Rgba([val as u8, val as u8, val as u8, 255]);
        }
        let hash_a: ImageHash<Vec<u8>> = {
            let h = hasher.hash_image(&img_a);
            ImageHash::from_base64(&h.to_base64()).unwrap()
        };

        let mut img_b = img_hash::image::RgbaImage::new(64, 64);
        for (x, _y, pixel) in img_b.enumerate_pixels_mut() {
            let val = (x * 255 / 63) as u8;
            *pixel = img_hash::image::Rgba([val, val, val, 255]);
        }
        let hash_b: ImageHash<Vec<u8>> = {
            let h = hasher.hash_image(&img_b);
            ImageHash::from_base64(&h.to_base64()).unwrap()
        };

        let dist = hash_a.dist(&hash_b);
        assert!(
            dist > 0,
            "Expected different hashes for checkerboard vs gradient"
        );

        let mut tree = BkTree::new();
        tree.insert("checker".to_string(), hash_a.clone());
        tree.insert("gradient".to_string(), hash_b.clone());

        let results = tree.find_within(&hash_a, 0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "checker");
    }

    #[test]
    fn test_bk_tree_finds_similar() {
        let light_gray = make_hash([200, 200, 200, 255]);
        let medium_gray = make_hash([180, 180, 180, 255]);

        let mut tree = BkTree::new();
        tree.insert("light".to_string(), light_gray.clone());
        tree.insert("medium".to_string(), medium_gray.clone());

        let results = tree.find_within(&light_gray, DEFAULT_DISTANCE_THRESHOLD);
        assert!(results.len() >= 1);
        assert!(results.iter().any(|(h, _)| h == "light"));
    }
}
