//! Reads EPUB books. It finds the package document that
//! `META-INF/container.xml` names, lists the XHTML chapters of the package's
//! spine in reading order, and parses one chapter at a time together with
//! the pictures that the chapter refers to. The reader shows each chapter as
//! one page.
use super::pages::{ENTRY_LIMIT, read_entry};
use super::rich::{self, Block, Picture};
use anyhow::{Context as _, Result, anyhow, bail};
use std::collections::HashMap;
use std::path::Path;

/// Returns the chapters of the book at `path` in reading order, each as the
/// path of its file inside the archive. It fails when the file is not a zip
/// archive, when the container file or the package document is missing or
/// damaged, and when the book has no chapters.
pub fn chapters(path: &Path) -> Result<Vec<String>> {
    let mut archive = open(path)?;
    let container = read_text(&mut archive, "META-INF/container.xml")
        .map_err(|_| anyhow!("This EPUB has no container manifest."))?;
    let rootfile = {
        let document = roxmltree::Document::parse(&container)
            .map_err(|_| anyhow!("This EPUB has no package document."))?;
        document
            .descendants()
            .find(|node| node.tag_name().name() == "rootfile")
            .and_then(|node| node.attribute("full-path"))
            .ok_or_else(|| anyhow!("This EPUB has no package document."))?
            .to_owned()
    };
    let package = read_text(&mut archive, &rootfile)
        .map_err(|_| anyhow!("The EPUB package document is missing."))?;
    let chapters = spine(&package, &rootfile)?;
    if chapters.is_empty() {
        bail!("This EPUB has no readable pages.");
    }
    Ok(chapters)
}

/// Returns the chapters that the spine of `package`, the text of the
/// package document, lists, in the spine's order. An entry of the spine is
/// a chapter when the media type of its manifest item contains "html". Each
/// chapter is returned as a path inside the archive: the address in its
/// manifest item, resolved against the folder of `rootfile`, the path of
/// the package document.
pub fn spine(package: &str, rootfile: &str) -> Result<Vec<String>> {
    let document =
        roxmltree::Document::parse(package).map_err(|_| anyhow!("The EPUB package is damaged."))?;
    let base = dirname(rootfile);
    let manifest: std::collections::HashMap<&str, (&str, &str)> = document
        .descendants()
        .filter(|node| node.tag_name().name() == "item")
        .map(|item| {
            (
                item.attribute("id").unwrap_or(""),
                (
                    item.attribute("href").unwrap_or(""),
                    item.attribute("media-type").unwrap_or(""),
                ),
            )
        })
        .collect();
    Ok(document
        .descendants()
        .filter(|node| node.tag_name().name() == "itemref")
        .filter_map(|item| manifest.get(item.attribute("idref").unwrap_or("")))
        .filter(|(_, media_type)| media_type.to_ascii_lowercase().contains("html"))
        .map(|(href, _)| resolve(base, href))
        .collect())
}

/// The most memory, 256 MB, that the decoded pictures of one chapter may
/// take together. A picture that would go over it is left out.
const PICTURE_BUDGET: u64 = 256 << 20;

/// Reads the chapter at `chapter`, a path inside the archive at `path`, and
/// returns its content as blocks with its pictures decoded. A picture that
/// the chapter uses more than once is decoded once. A picture that is
/// missing or cannot be decoded is left out.
pub fn chapter(path: &Path, chapter: &str) -> Result<Vec<Block>> {
    let mut archive = open(path)?;
    let markup = read_text(&mut archive, chapter)?;
    let base = dirname(chapter).to_owned();
    let mut decoded: HashMap<String, Option<Picture>> = HashMap::new();
    let mut budget = PICTURE_BUDGET;
    let mut picture = |source: &str| -> Option<Picture> {
        let entry = resolve(&base, source);
        if let Some(picture) = decoded.get(&entry) {
            return picture.clone();
        }
        let picture = read_entry(&mut archive, &entry, ENTRY_LIMIT)
            .and_then(|bytes| super::pages::decode_picture(&entry, &bytes))
            .ok()
            .filter(|image| {
                let bytes = image.as_raw().len() as u64;
                let fits = bytes <= budget;
                if fits {
                    budget -= bytes;
                }
                fits
            })
            .map(|image| Picture {
                size: (image.width() as f32, image.height() as f32),
                image: super::render_image(image),
            });
        decoded.insert(entry, picture.clone());
        picture
    };
    Ok(rich::xhtml(&markup, &mut picture))
}

fn open(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path).context("read the EPUB")?;
    zip::ZipArchive::new(file).map_err(|_| anyhow!("Could not open this EPUB."))
}

fn read_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String> {
    let bytes = read_entry(archive, name, ENTRY_LIMIT)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Returns the folder part of a path inside the archive, with the slash at
/// its end. A path without a slash gives the empty string.
fn dirname(path: &str) -> &str {
    path.rfind('/').map_or("", |slash| &path[..=slash])
}

/// Returns the path inside the archive that the reference `relative` names
/// when a file in the folder `base` uses it. The reference's fragment and
/// query are removed, `.` and `..` are resolved, and percent escapes are
/// decoded. A reference that starts with a slash is counted from the
/// archive's root. No reference gives a path outside the archive's root.
pub fn resolve(base: &str, relative: &str) -> String {
    let relative = relative.split(['#', '?']).next().unwrap_or("");
    let joined = if relative.starts_with('/') {
        relative.trim_start_matches('/').to_owned()
    } else {
        format!("{base}{relative}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    percent_decode(&parts.join("/"))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(code) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(code);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{chapter, resolve, spine};
    use crate::viewer::document::rich::Block;

    #[test]
    fn references_resolve_inside_the_archive() {
        assert_eq!(
            resolve("OEBPS/text/", "../images/a%20b.png#x"),
            "OEBPS/images/a b.png"
        );
        assert_eq!(resolve("OEBPS/", "ch1.xhtml"), "OEBPS/ch1.xhtml");
        assert_eq!(resolve("", "./ch1.xhtml"), "ch1.xhtml");
        // A reference with more `..` than there are folders, and one that
        // starts with a slash, still name a path inside the archive's root.
        assert_eq!(resolve("OEBPS/", "../../../etc/passwd"), "etc/passwd");
        assert_eq!(resolve("OEBPS/", "/etc/passwd"), "etc/passwd");
    }

    #[test]
    fn a_picture_used_many_times_is_decoded_once() {
        use std::io::Write;
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("book.epub");
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        let mut png = Vec::new();
        image::RgbaImage::new(2, 1)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        archive.start_file("a.png", options).unwrap();
        archive.write_all(&png).unwrap();
        archive.start_file("one.xhtml", options).unwrap();
        let body = r#"<img src="a.png"/><img src="missing.png"/>"#.repeat(500);
        write!(archive, "<html><body>{body}</body></html>").unwrap();
        archive.finish().unwrap();
        let blocks = chapter(&path, "one.xhtml").unwrap();
        let pictures: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Picture(picture) => Some(picture),
                _ => None,
            })
            .collect();
        assert_eq!(pictures.len(), 500);
        assert!(pictures.iter().all(|picture| picture == &pictures[0]));
    }

    #[test]
    fn the_spine_lists_html_chapters_in_order() {
        let package = r#"<package xmlns="http://www.idpf.org/2007/opf"><manifest>
            <item id="c2" href="text/two.xhtml" media-type="application/xhtml+xml"/>
            <item id="c1" href="text/one.xhtml" media-type="application/xhtml+xml"/>
            <item id="css" href="style.css" media-type="text/css"/>
          </manifest><spine><itemref idref="c1"/><itemref idref="css"/><itemref idref="c2"/></spine></package>"#;
        assert_eq!(
            spine(package, "OEBPS/content.opf").unwrap(),
            ["OEBPS/text/one.xhtml", "OEBPS/text/two.xhtml"]
        );
    }
}
