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
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
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
}
