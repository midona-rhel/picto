//! Dominant color extraction from images using k-means clustering.
//!
//! Extracts perceptually distinct dominant colors from an image, sorted by abundance.
//! Uses Lab color space for clustering and Delta E for merging similar colors.
//! A black-and-white lineart image will correctly return just black and white.

use image::DynamicImage;
use kmeans_colors::{get_kmeans_hamerly, Sort};
use palette::{cast::from_component_slice, IntoColor, Lab, Srgb};

/// A dominant color associated with an image.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DominantColor {
    /// Hex color string, e.g., "#3a7bd5".
    pub hex: String,
    /// CIE Lab L* (lightness, 0–100). Stored for indexed color search.
    pub l: f64,
    /// CIE Lab a* (green–red, roughly -128 to +127).
    pub a: f64,
    /// CIE Lab b* (blue–yellow, roughly -128 to +127).
    pub b: f64,
}

/// Delta E (CIE76) distance between two Lab colors.
fn lab_distance(a: &Lab, b: &Lab) -> f64 {
    let dl = (a.l - b.l) as f64;
    let da = (a.a - b.a) as f64;
    let db = (a.b - b.b) as f64;
    (dl * dl + da * da + db * db).sqrt()
}

/// Merge threshold in Lab space.
/// ~15 is a good balance: distinct hues stay separate, but slight variations merge.
const MERGE_THRESHOLD: f64 = 15.0;

/// Minimum weight for a color to be kept (3% of image).
const MIN_WEIGHT: f64 = 0.03;

/// Extract dominant colors from an image.
///
/// Over-clusters into 12 buckets, then merges perceptually similar colors and
/// drops insignificant ones. Returns only truly distinct colors, sorted by
/// abundance (most dominant first).
pub fn extract_dominant_colors(img: &DynamicImage, max_colors: usize) -> Vec<DominantColor> {
    let max_out = max_colors.min(10).max(1);

    // Downscale for performance — k-means on full resolution is wasteful.
    let small = img.resize(128, 128, image::imageops::FilterType::Triangle);
    let rgb_img = small.to_rgb8();
    let raw = rgb_img.as_raw();

    // Convert u8 RGB buffer → palette Srgb → linear → Lab
    let srgb_pixels: &[Srgb<u8>] = from_component_slice(raw);
    let lab_pixels: Vec<Lab> = srgb_pixels
        .iter()
        .map(|c| c.into_linear::<f32>().into_color())
        .collect();

    if lab_pixels.is_empty() {
        return Vec::new();
    }

    // Over-cluster: use more buckets than we want, then merge similar ones.
    let k = 12usize.min(lab_pixels.len());
    let result = get_kmeans_hamerly(k, 20, 5.0, false, &lab_pixels, 42);

    // Use the library's sort to get centroids with percentages.
    let mut sorted = Lab::sort_indexed_colors(&result.centroids, &result.indices);
    sorted.sort_unstable_by(|a, b| b.percentage.total_cmp(&a.percentage));

    // Build (Lab, weight) clusters, filtering out near-zero.
    let clusters: Vec<(Lab, f64)> = sorted
        .iter()
        .filter(|c| c.percentage > 0.005)
        .map(|c| (c.centroid, c.percentage as f64))
        .collect();

    // Merge similar colors: walk the list, merge any color within threshold
    // into the nearest already-accepted color (weighted average in Lab space).
    let mut merged: Vec<(Lab, f64)> = Vec::new();
    for (lab, weight) in &clusters {
        let mut found = false;
        for (existing_lab, existing_weight) in merged.iter_mut() {
            if lab_distance(lab, existing_lab) < MERGE_THRESHOLD {
                let total_w = *existing_weight + weight;
                existing_lab.l = (existing_lab.l as f64 * *existing_weight / total_w
                    + lab.l as f64 * weight / total_w) as f32;
                existing_lab.a = (existing_lab.a as f64 * *existing_weight / total_w
                    + lab.a as f64 * weight / total_w) as f32;
                existing_lab.b = (existing_lab.b as f64 * *existing_weight / total_w
                    + lab.b as f64 * weight / total_w) as f32;
                *existing_weight = total_w;
                found = true;
                break;
            }
        }
        if !found {
            merged.push((*lab, *weight));
        }
    }

    // Re-sort by weight after merging (weight used only for filtering/ordering).
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Filter out insignificant colors and cap at max_out.
    merged
        .into_iter()
        .filter(|(_, w)| *w >= MIN_WEIGHT)
        .take(max_out)
        .map(|(lab, _)| {
            let srgb: Srgb<u8> = Srgb::from_linear(lab.into_color());
            DominantColor {
                hex: format!("#{:02x}{:02x}{:02x}", srgb.red, srgb.green, srgb.blue),
                l: lab.l as f64,
                a: lab.a as f64,
                b: lab.b as f64,
            }
        })
        .collect()
}
