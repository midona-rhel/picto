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

/// `mimeToExt`: the grid's extension for a type, as its labels spell it.
pub fn label_extension(mime: &str) -> String {
    mime.split_once('/')
        .map(|(_, subtype)| extension_for_subtype(&subtype.to_lowercase()).to_owned())
        .unwrap_or_default()
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

/// `toLocaleString()` for a count: digits grouped by thousands with commas.
pub fn grouped(count: impl Into<i128>) -> String {
    let count = count.into();
    let digits = count.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if count < 0 {
        out.push('-');
    }
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

/// `truncateText`: the text whole when it fits in `max`, else as many of
/// its characters as fit with "…" after them, measured by `measure`.
pub fn truncate_to(text: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(text) <= max {
        return text.to_owned();
    }
    let chars = text.chars().collect::<Vec<_>>();
    let mut end = chars.len().saturating_sub(1);
    let cut = |end: usize| chars[..end].iter().collect::<String>() + "…";
    while end > 0 && measure(&cut(end)) > max {
        end -= 1;
    }
    cut(end)
}

/// CSS `grayscale(1)` of an sRGB colour (`0xRRGGBB`): each channel the
/// colour's luminance, by the filter's coefficients.
pub fn grayscale(color: u32) -> u32 {
    let channel = |shift: u32| ((color >> shift) & 0xff) as f32;
    let gray = (0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0))
        .round()
        .clamp(0., 255.) as u32;
    (gray << 16) | (gray << 8) | gray
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grayscale_keeps_the_luminance_of_a_colour() {
        assert_eq!(grayscale(0xffffff), 0xffffff);
        assert_eq!(grayscale(0x000000), 0x000000);
        // Pure green is mostly light; pure blue barely.
        assert_eq!(grayscale(0x00ff00), 0xb6b6b6);
        assert_eq!(grayscale(0x0000ff), 0x121212);
    }

    #[test]
    fn names_truncate_with_an_ellipsis_to_what_fits() {
        // Ten units a character.
        let measure = |text: &str| text.chars().count() as f32 * 10.;
        assert_eq!(truncate_to("Tribal", 60., measure), "Tribal");
        assert_eq!(
            truncate_to("Main Four - Communal", 100., measure),
            "Main Four…"
        );
        // A space before the cut stays, as `slice` keeps it.
        assert_eq!(truncate_to("Main Four - C", 60., measure), "Main …");
        assert_eq!(truncate_to("ab", 5., measure), "…");
    }

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
        assert_eq!(grouped(2673u64), "2,673");
        assert_eq!(grouped(999u64), "999");
        assert_eq!(grouped(1_000_000u64), "1,000,000");
        assert_eq!(grouped(-1234567i64), "-1,234,567");
        assert_eq!(grouped(0i64), "0");
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
