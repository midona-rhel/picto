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

    if left.mime_type == right.mime_type {
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

#[cfg(test)]
mod tests {
    use super::{compare_static_image_quality, ComparableImageCandidate, ImageQualityDecision};

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
}
