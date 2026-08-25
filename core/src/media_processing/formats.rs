//! File formats accepted by Picto.
//!
//! Acceptance and preview support are intentionally separate. Picto preserves every
//! accepted file, while `media_capabilities` decides which derivatives and previews
//! can be generated today.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptedFormat {
    pub extension: &'static str,
    pub mime_type: &'static str,
}

// reference application's documented 115 previewable formats, plus formats Picto already supported
// and format-extension plugins explicitly selected for the release backlog.
pub const ACCEPTED_FORMATS: &[AcceptedFormat] = &[
    // Images and textures.
    format("jpg", "image/jpeg"),
    format("jpeg", "image/jpeg"),
    format("jpe", "image/jpeg"),
    format("jfif", "image/jpeg"),
    format("png", "image/png"),
    format("apng", "image/apng"),
    format("gif", "image/gif"),
    format("webp", "image/webp"),
    format("bmp", "image/bmp"),
    format("dib", "image/bmp"),
    format("tif", "image/tiff"),
    format("tiff", "image/tiff"),
    format("svg", "image/svg+xml"),
    format("svgz", "image/svg+xml"),
    format("ico", "image/x-icon"),
    format("icns", "image/x-icns"),
    format("cur", "image/x-icon"),
    format("heic", "image/heic"),
    format("heics", "image/heic-sequence"),
    format("heif", "image/heif"),
    format("heifs", "image/heif-sequence"),
    format("hif", "image/heif"),
    format("avif", "image/avif"),
    format("avifs", "image/avif-sequence"),
    format("jxl", "image/jxl"),
    format("qoi", "image/qoi"),
    format("base64", "text/x-base64"),
    format("insp", "application/x-insp"),
    format("svga", "image/x-svga"),
    format("pag", "application/x-pag"),
    format("lottie", "application/vnd.lottie+zip"),
    format("dds", "image/vnd-ms.dds"),
    format("exr", "image/x-exr"),
    format("hdr", "image/vnd.radiance"),
    format("tga", "image/x-tga"),
    format("iff", "image/x-ilbm"),
    format("lbm", "image/x-ilbm"),
    // Video.
    format("mp4", "video/mp4"),
    format("m4v", "video/x-m4v"),
    format("webm", "video/webm"),
    format("mkv", "video/x-matroska"),
    format("mov", "video/quicktime"),
    format("qt", "video/quicktime"),
    format("avi", "video/x-msvideo"),
    format("flv", "video/x-flv"),
    format("f4v", "video/x-f4v"),
    format("wmv", "video/x-ms-wmv"),
    format("ogv", "video/ogg"),
    format("mpeg", "video/mpeg"),
    format("mpg", "video/mpeg"),
    format("mpe", "video/mpeg"),
    format("rm", "video/vnd.rn-realvideo"),
    format("rmvb", "video/vnd.rn-realvideo"),
    format("3gp", "video/3gpp"),
    format("3g2", "video/3gpp2"),
    format("ts", "video/mp2t"),
    format("mts", "video/mp2t"),
    format("m2ts", "video/mp2t"),
    // Audio.
    format("aac", "audio/aac"),
    format("flac", "audio/flac"),
    format("m4a", "audio/mp4"),
    format("mp3", "audio/mpeg"),
    format("ogg", "audio/ogg"),
    format("oga", "audio/ogg"),
    format("opus", "audio/opus"),
    format("wav", "audio/wav"),
    format("wave", "audio/wav"),
    format("wma", "audio/x-ms-wma"),
    format("mka", "audio/x-matroska"),
    format("wv", "audio/wavpack"),
    format("tta", "audio/x-tta"),
    // 3D.
    format("fbx", "model/x-fbx"),
    format("obj", "model/obj"),
    format("3ds", "model/x-3ds"),
    format("3mf", "model/3mf"),
    format("dae", "model/vnd.collada+xml"),
    format("ifc", "model/x-ifc"),
    format("ply", "model/ply"),
    format("stl", "model/stl"),
    format("glb", "model/gltf-binary"),
    format("gltf", "model/gltf+json"),
    format("vrm", "model/vrm"),
    // Design source files.
    format("af", "application/x-affinity"),
    format("afdesign", "application/x-affinity-designer"),
    format("afphoto", "application/x-affinity-photo"),
    format("afpub", "application/x-affinity-publisher"),
    format("ai", "application/postscript"),
    format("c4d", "application/x-cinema4d"),
    format("cdr", "application/x-coreldraw"),
    format("clip", "application/x-clip-studio-paint"),
    format("dwg", "image/vnd.dwg"),
    format("graffle", "application/x-omnigraffle"),
    format("idml", "application/vnd.adobe.indesign-idml-package"),
    format("indd", "application/x-indesign"),
    format("indt", "application/x-indesign-template"),
    format("mindnode", "application/x-mindnode"),
    format("psd", "image/vnd.adobe.photoshop"),
    format("psb", "image/vnd.adobe.photoshop.large"),
    format("psdt", "image/vnd.adobe.photoshop.template"),
    format("pxd", "application/x-pixelmator"),
    format("principle", "application/x-principle"),
    format("sketch", "application/x-sketch"),
    format("skt", "application/x-sketch-template"),
    format("skp", "model/vnd.sketchup.skp"),
    format("xd", "application/x-adobe-xd"),
    format("xmind", "application/x-xmind"),
    // Fonts.
    format("ttf", "font/ttf"),
    format("ttc", "font/collection"),
    format("otf", "font/otf"),
    format("woff", "font/woff"),
    // Camera RAW.
    format("3fr", "image/x-hasselblad-3fr"),
    format("arw", "image/x-sony-arw"),
    format("cr2", "image/x-canon-cr2"),
    format("cr3", "image/x-canon-cr3"),
    format("crw", "image/x-canon-crw"),
    format("dng", "image/x-adobe-dng"),
    format("erf", "image/x-epson-erf"),
    format("mrw", "image/x-minolta-mrw"),
    format("nef", "image/x-nikon-nef"),
    format("nrw", "image/x-nikon-nrw"),
    format("orf", "image/x-olympus-orf"),
    format("pef", "image/x-pentax-pef"),
    format("raf", "image/x-fuji-raf"),
    format("raw", "image/x-raw"),
    format("rw2", "image/x-panasonic-rw2"),
    format("sr2", "image/x-sony-sr2"),
    format("srw", "image/x-samsung-srw"),
    format("x3f", "image/x-sigma-x3f"),
    // Documents and web files.
    format("txt", "text/plain"),
    format("md", "text/markdown"),
    format("markdown", "text/markdown"),
    format("json", "application/json"),
    format("rtf", "application/rtf"),
    format("key", "application/x-iwork-keynote-sffkey"),
    format("numbers", "application/x-iwork-numbers-sffnumbers"),
    format("pages", "application/x-iwork-pages-sffpages"),
    format("pdf", "application/pdf"),
    format(
        "potx",
        "application/vnd.openxmlformats-officedocument.presentationml.template",
    ),
    format("ppt", "application/vnd.ms-powerpoint"),
    format(
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    format("xls", "application/vnd.ms-excel"),
    format(
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    format("doc", "application/msword"),
    format(
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    format("eddx", "application/x-edraw"),
    format("emmx", "application/x-edraw-mindmap"),
    format("html", "text/html"),
    format("htm", "text/html"),
    format("mhtml", "multipart/related"),
    format("url", "application/internet-shortcut"),
    // Visualization and production formats exposed by reference application format extensions.
    format("cube", "application/x-cube-lut"),
    format("3dl", "application/x-3dl-lut"),
    format("ies", "application/x-ies"),
    // Explicit format-extension plugin targets and existing Picto formats.
    format("zip", "application/zip"),
    format("epub", "application/epub+zip"),
    format("cbz", "application/vnd.comicbook+zip"),
    format("djvu", "image/vnd.djvu"),
    format("djv", "image/vnd.djvu"),
    format("swf", "application/x-shockwave-flash"),
    format("eps", "application/x-eps"),
    format("livp", "application/x-live-photo"),
    format("sai2", "application/x-sai2"),
    format("kra", "application/x-krita"),
    format("xcf", "image/x-xcf"),
    format("procreate", "application/x-procreate"),
    format("pdn", "application/x-paint-dot-net"),
];

const fn format(extension: &'static str, mime_type: &'static str) -> AcceptedFormat {
    AcceptedFormat {
        extension,
        mime_type,
    }
}

pub fn format_for_extension(extension: &str) -> Option<AcceptedFormat> {
    ACCEPTED_FORMATS
        .iter()
        .copied()
        .find(|format| format.extension.eq_ignore_ascii_case(extension))
}

pub fn format_for_path(path: &Path) -> Option<AcceptedFormat> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(format_for_extension)
}

pub fn has_supported_extension(path: &Path) -> bool {
    format_for_path(path).is_some()
}

pub fn extension_for_mime(mime_type: &str) -> Option<&'static str> {
    ACCEPTED_FORMATS
        .iter()
        .find(|format| format.mime_type == mime_type)
        .map(|format| format.extension)
}

pub fn is_supported_mime(mime_type: &str) -> bool {
    ACCEPTED_FORMATS
        .iter()
        .any(|format| format.mime_type == mime_type)
}

#[cfg(test)]
mod tests {
    use super::{extension_for_mime, format_for_extension, has_supported_extension};
    use std::path::Path;

    #[test]
    fn reference_app_audio_source_and_document_formats_are_accepted() {
        for extension in ["mp3", "flac", "fbx", "afdesign", "cr3", "docx", "mhtml"] {
            assert!(format_for_extension(extension).is_some(), "{extension}");
        }
    }

    #[test]
    fn plugin_formats_and_zip_are_accepted() {
        for path in [
            "document.pdf",
            "photo.jxl",
            "book.epub",
            "animation.swf",
            "art.eps",
            "photo.livp",
            "animation.svga",
            "animation.pag",
            "animation.lottie",
            "scene.vrm",
            "grade.cube",
            "light.ies",
            "readme.md",
            "pack.zip",
        ] {
            assert!(has_supported_extension(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn canonical_blob_extensions_are_stable() {
        assert_eq!(extension_for_mime("audio/mpeg"), Some("mp3"));
        assert_eq!(extension_for_mime("model/gltf-binary"), Some("glb"));
    }
}
