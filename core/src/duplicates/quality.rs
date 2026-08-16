use crate::media_capabilities::capabilities_for_stored_media;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageQualityDecision {
    LeftBetter,
    RightBetter,
    Ambiguous,
}

#[derive(Debug, Clone)]
pub struct ComparableImageCandidate<'a> {
    pub mime_type: &'a str,
    pub size_bytes: i64,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub frame_count: Option<i64>,
}

fn is_lossless_hint(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "image/png" | "image/tiff" | "image/bmp" | "image/x-icon" | "image/qoi"
    )
}

fn pixel_count(candidate: &ComparableImageCandidate<'_>) -> i64 {
    candidate
        .pixel_width
        .unwrap_or(0)
        .saturating_mul(candidate.pixel_height.unwrap_or(0))
}

fn comparable_static_image(candidate: &ComparableImageCandidate<'_>) -> bool {
    capabilities_for_stored_media(candidate.mime_type, candidate.frame_count).can_perceptual_hash
}

pub fn compare_static_image_quality(
    left: &ComparableImageCandidate<'_>,
    right: &ComparableImageCandidate<'_>,
) -> ImageQualityDecision {
    compare_static_image_quality_with_distance(left, right, None)
}

/// Compare candidates with the duplicate detector's Hamming distance when it
/// is available. A distance of zero or one means a material encoded-quality
/// difference is safe to resolve without asking the user.
pub fn compare_static_image_quality_with_distance(
    left: &ComparableImageCandidate<'_>,
    right: &ComparableImageCandidate<'_>,
    distance: Option<u32>,
) -> ImageQualityDecision {
    if !comparable_static_image(left) || !comparable_static_image(right) {
        return ImageQualityDecision::Ambiguous;
    }

    let left_pixels = pixel_count(left);
    let right_pixels = pixel_count(right);
    if left_pixels == 0 || right_pixels == 0 {
        return ImageQualityDecision::Ambiguous;
    }

    let left_lossless = is_lossless_hint(left.mime_type);
    let right_lossless = is_lossless_hint(right.mime_type);

    if left_lossless != right_lossless {
        if left_lossless && left_pixels.saturating_mul(10) >= right_pixels.saturating_mul(9) {
            return ImageQualityDecision::LeftBetter;
        }
        if right_lossless && right_pixels.saturating_mul(10) >= left_pixels.saturating_mul(9) {
            return ImageQualityDecision::RightBetter;
        }
    }

    let left_density = left
        .size_bytes
        .saturating_mul(1_000)
        .checked_div(left_pixels);
    let right_density = right
        .size_bytes
        .saturating_mul(1_000)
        .checked_div(right_pixels);

    // This is a deterministic retained-information proxy: decoded pixel
    // count multiplied by the logarithm of encoded bits per pixel. It is not
    // source-pixel Shannon entropy, but it quantifies the detail signal that
    // is available here without decoding blobs in a database write.
    let left_information = retained_information_score(left_pixels, left.size_bytes);
    let right_information = retained_information_score(right_pixels, right.size_bytes);

    // A resolution win only counts while byte density holds up — when the
    // larger image's bytes-per-pixel falls below 2/3 of the smaller one's,
    // the extra pixels are likely recompression bloat, not detail.
    if left_pixels >= right_pixels.saturating_mul(2) {
        if left_density.unwrap_or(0).saturating_mul(3)
            >= right_density.unwrap_or(0).saturating_mul(2)
        {
            return ImageQualityDecision::LeftBetter;
        }
    }

    if right_pixels >= left_pixels.saturating_mul(2) {
        if right_density.unwrap_or(0).saturating_mul(3)
            >= left_density.unwrap_or(0).saturating_mul(2)
        {
            return ImageQualityDecision::RightBetter;
        }
    }

    // For the same format, a candidate that is materially larger in both
    // dimensions (by area) and encoded bytes is the stronger source. The
    // density guard above is useful across formats, but rejects legitimate
    // JPEG quality/resolution upgrades like 4000x3877 at 1.8 MB versus
    // 2048x1986 at 747 KB.
    if left.mime_type == right.mime_type {
        if distance.is_some_and(|value| value <= 1)
            && left.size_bytes.saturating_mul(3) >= right.size_bytes.saturating_mul(2)
            && left_information > right_information * 1.20
        {
            return ImageQualityDecision::LeftBetter;
        }
        if distance.is_some_and(|value| value <= 1)
            && right.size_bytes.saturating_mul(3) >= left.size_bytes.saturating_mul(2)
            && right_information > left_information * 1.20
        {
            return ImageQualityDecision::RightBetter;
        }

        if left_pixels.saturating_mul(2) >= right_pixels.saturating_mul(3)
            && left.size_bytes.saturating_mul(2) >= right.size_bytes.saturating_mul(3)
            && left_information > right_information * 1.20
        {
            return ImageQualityDecision::LeftBetter;
        }
        if right_pixels.saturating_mul(2) >= left_pixels.saturating_mul(3)
            && right.size_bytes.saturating_mul(2) >= left.size_bytes.saturating_mul(3)
            && right_information > left_information * 1.20
        {
            return ImageQualityDecision::RightBetter;
        }

        if left_pixels.saturating_mul(2) >= right_pixels.saturating_mul(3)
            && left_density.unwrap_or(0).saturating_mul(3)
                >= right_density.unwrap_or(0).saturating_mul(2)
        {
            return ImageQualityDecision::LeftBetter;
        }
        if right_pixels.saturating_mul(2) >= left_pixels.saturating_mul(3)
            && right_density.unwrap_or(0).saturating_mul(3)
                >= left_density.unwrap_or(0).saturating_mul(2)
        {
            return ImageQualityDecision::RightBetter;
        }
    }

    ImageQualityDecision::Ambiguous
}

fn retained_information_score(pixel_count: i64, size_bytes: i64) -> f64 {
    let pixels = pixel_count as f64;
    let encoded_bits_per_pixel = (size_bytes.max(0) as f64 * 8.0) / pixels;
    pixels * (1.0 + (1.0 + encoded_bits_per_pixel).ln())
}

#[cfg(test)]
mod tests {
    use super::{
        compare_static_image_quality, compare_static_image_quality_with_distance,
        ComparableImageCandidate, ImageQualityDecision,
    };

    #[test]
    fn prefers_lossless_when_detail_is_comparable() {
        let png = ComparableImageCandidate {
            mime_type: "image/png",
            size_bytes: 600_000,
            pixel_width: Some(1200),
            pixel_height: Some(800),
            frame_count: Some(1),
        };
        let jpeg = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 550_000,
            pixel_width: Some(1200),
            pixel_height: Some(800),
            frame_count: Some(1),
        };
        assert_eq!(
            compare_static_image_quality(&png, &jpeg),
            ImageQualityDecision::LeftBetter
        );
    }

    #[test]
    fn keeps_resolution_only_decisions_ambiguous_when_density_collapses() {
        let large_jpeg = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 250_000,
            pixel_width: Some(2000),
            pixel_height: Some(1200),
            frame_count: Some(1),
        };
        let smaller_png = ComparableImageCandidate {
            mime_type: "image/png",
            size_bytes: 240_000,
            pixel_width: Some(1400),
            pixel_height: Some(840),
            frame_count: Some(1),
        };
        assert_eq!(
            compare_static_image_quality(&large_jpeg, &smaller_png),
            ImageQualityDecision::Ambiguous
        );
    }

    #[test]
    fn prefers_the_larger_lossless_candidate_from_duplicate_review() {
        let png = ComparableImageCandidate {
            mime_type: "image/png",
            size_bytes: 1_200_000,
            pixel_width: Some(4570),
            pixel_height: Some(1191),
            frame_count: Some(1),
        };
        let jpeg = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 225_600,
            pixel_width: Some(4096),
            pixel_height: Some(1067),
            frame_count: Some(1),
        };

        assert_eq!(
            compare_static_image_quality(&png, &jpeg),
            ImageQualityDecision::LeftBetter
        );
    }

    #[test]
    fn prefers_same_format_candidate_with_material_resolution_and_size_gain() {
        let larger = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 1_800_000,
            pixel_width: Some(4000),
            pixel_height: Some(3877),
            frame_count: Some(1),
        };
        let smaller = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 747_500,
            pixel_width: Some(2048),
            pixel_height: Some(1986),
            frame_count: Some(1),
        };

        assert_eq!(
            compare_static_image_quality(&larger, &smaller),
            ImageQualityDecision::LeftBetter
        );
    }

    #[test]
    fn keeps_same_format_candidates_with_negligible_difference_ambiguous() {
        let left = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 1_000_000,
            pixel_width: Some(2000),
            pixel_height: Some(1200),
            frame_count: Some(1),
        };
        let right = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 1_010_000,
            pixel_width: Some(2000),
            pixel_height: Some(1200),
            frame_count: Some(1),
        };

        assert_eq!(
            compare_static_image_quality(&left, &right),
            ImageQualityDecision::Ambiguous
        );
    }

    #[test]
    fn resolves_near_identical_same_size_content_by_material_encoded_information() {
        let larger = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 2_000_000,
            pixel_width: Some(2000),
            pixel_height: Some(1200),
            frame_count: Some(1),
        };
        let smaller = ComparableImageCandidate {
            mime_type: "image/jpeg",
            size_bytes: 900_000,
            pixel_width: Some(2000),
            pixel_height: Some(1200),
            frame_count: Some(1),
        };

        assert_eq!(
            compare_static_image_quality_with_distance(&larger, &smaller, Some(0)),
            ImageQualityDecision::LeftBetter
        );
    }
}
