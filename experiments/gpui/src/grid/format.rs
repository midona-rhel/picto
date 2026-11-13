//! Grid labels, ported from `src/features/grid/canvas/primitives.ts`.

fn extension_for_subtype(subtype: &str) -> &str {
    match subtype {
        "jpeg" => "jpg",
        "svg+xml" => "svg",
        "jxl" => "jpgxl",
        "x-icon" => "ico",
        "vnd.adobe.photoshop" => "psd",
        "quicktime" => "mov",
        "x-matroska" => "mkv",
        "x-flv" => "flv",
        "x-msvideo" => "avi",
        "mpeg" => "mp3",
        "x-wav" => "wav",
        "mp4a" => "m4a",
        "epub+zip" => "epub",
        "x-shockwave-flash" => "swf",
        "vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "vnd.comicbook+zip" => "cbz",
        "vnd.djvu" => "djvu",
        "plain" => "txt",
        "markdown" => "md",
        "woff2" => "wof2",
        other => other,
    }
}

/// Compact grid label: four characters, except the five-letter JPGXL.
/// The extension an original of this type is stored under.
pub fn extension(mime: &str) -> String {
    let subtype = mime.split_once('/').map_or("", |(_, subtype)| subtype);
    match subtype.to_lowercase().as_str() {
        // The file keeps the usual extension, not the grid label's spelling.
        "jxl" => "jxl".to_owned(),
        "woff2" => "woff2".to_owned(),
        other => extension_for_subtype(other).to_owned(),
    }
}

pub fn format_label(mime: &str) -> String {
    let Some((_, subtype)) = mime.split_once('/') else {
        return String::new();
    };
    let subtype = subtype.to_lowercase();
    let extension = extension_for_subtype(&subtype);
    let label = if extension == "jpgxl" {
        extension.to_owned()
    } else {
        extension.chars().take(4).collect()
    };
    label.to_uppercase()
}

pub fn format_duration(ms: u64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours}:{:02}:{:02}", minutes % 60, seconds % 60)
    } else {
        format!("{minutes}:{:02}", seconds % 60)
    }
}

/// Thousands separators, as the sidebar counts show them.
pub fn grouped(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A palette entry as an sRGB colour (`labToHex` in `labColor.ts`).
pub fn lab_to_rgb(l: f32, a: f32, b: f32) -> u32 {
    let fy = (l + 16.) / 116.;
    let fx = a / 500. + fy;
    let fz = fy - b / 200.;
    let inverse = |part: f32| {
        if part.powi(3) > 0.008856 {
            part.powi(3)
        } else {
            (part - 16. / 116.) / 7.787
        }
    };
    let (x, y, z) = (0.95047 * inverse(fx), inverse(fy), 1.08883 * inverse(fz));
    let channel = |linear: f32| {
        let encoded = if linear <= 0.0031308 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1. / 2.4) - 0.055
        };
        (encoded.clamp(0., 1.) * 255.).round() as u32
    };
    channel(x * 3.2406 + y * -1.5372 + z * -0.4986) << 16
        | channel(x * -0.9689 + y * 1.8758 + z * 0.0415) << 8
        | channel(x * 0.0557 + y * -0.204 + z * 1.057)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_the_electron_grid() {
        assert_eq!(format_label("image/jpeg"), "JPG");
        assert_eq!(format_label("image/png"), "PNG");
        assert_eq!(format_label("image/jxl"), "JPGXL");
        assert_eq!(format_label("video/quicktime"), "MOV");
        assert_eq!(format_label("application/x-shockwave-flash"), "SWF");
        assert_eq!(format_label("image/svg+xml"), "SVG");
        assert_eq!(format_label("application/octet-stream"), "OCTE");
        assert_eq!(format_label("nonsense"), "");
    }

    #[test]
    fn durations_and_counts() {
        assert_eq!(format_duration(65_000), "1:05");
        assert_eq!(format_duration(3_725_000), "1:02:05");
        assert_eq!(format_duration(900), "0:00");
        assert_eq!(grouped(2673), "2,673");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000_000), "1,000,000");
    }

    #[test]
    fn lab_colours_convert_to_srgb() {
        assert_eq!(lab_to_rgb(0., 0., 0.), 0x000000);
        assert_eq!(lab_to_rgb(100., 0., 0.), 0xffffff);
        // sRGB primaries in CIE Lab (D65).
        assert_eq!(lab_to_rgb(53.2408, 80.0925, 67.2032), 0xff0000);
        assert_eq!(lab_to_rgb(32.2970, 79.1875, -107.8602), 0x0000ff);
    }
}
