//! EPUB books as `ArchiveDocumentViewer.tsx` reads them: the package
//! document named by `META-INF/container.xml`, its spine of XHTML chapters,
//! one chapter to a page, and the pictures each chapter refers to.
use super::rich::{self, Block, Picture};
use anyhow::{Context as _, Result, anyhow, bail};
use gpui::RenderImage;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// The book's chapters, as paths inside the archive, in reading order.
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

/// The spine's XHTML chapters, resolved against the package document.
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

/// One chapter's blocks, with its pictures decoded.
pub fn chapter(path: &Path, chapter: &str) -> Result<Vec<Block>> {
    let mut archive = open(path)?;
    let markup = read_text(&mut archive, chapter)?;
    let base = dirname(chapter).to_owned();
    let mut picture = |source: &str| -> Option<Picture> {
        let entry = resolve(&base, source);
        let mut bytes = Vec::new();
        archive.by_name(&entry).ok()?.read_to_end(&mut bytes).ok()?;
        let image = super::pages::decode_picture(&entry, &bytes).ok()?;
        let size = (image.width() as f32, image.height() as f32);
        Some(Picture {
            image: render_image(image),
            size,
        })
    };
    Ok(rich::xhtml(&markup, &mut picture))
}

/// A decoded picture in the BGRA order GPUI draws.
pub fn render_image(mut image: image::RgbaImage) -> Arc<RenderImage> {
    for pixel in image.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
        image
    )]))
}

fn open(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path).context("read the EPUB")?;
    zip::ZipArchive::new(file).map_err(|_| anyhow!("Could not open this EPUB."))
}

fn read_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|_| anyhow!("{name} is missing"))?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `dirname`: the folder part of an archive path, with its slash.
fn dirname(path: &str) -> &str {
    path.rfind('/').map_or("", |slash| &path[..=slash])
}

/// `resolveArchivePath`: a relative reference resolved against a folder in
/// the archive, without its fragment or query, percent-decoded.
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
    use super::{resolve, spine};

    #[test]
    fn references_resolve_inside_the_archive() {
        assert_eq!(
            resolve("OEBPS/text/", "../images/a%20b.png#x"),
            "OEBPS/images/a b.png"
        );
        assert_eq!(resolve("OEBPS/", "ch1.xhtml"), "OEBPS/ch1.xhtml");
        assert_eq!(resolve("", "./ch1.xhtml"), "ch1.xhtml");
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
