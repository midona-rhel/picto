//! Create an isolated, renderable million-item library for manual grid testing.
//! Never opens or modifies an existing library. Not an import/codec benchmark.
//! cargo run --release -p picto_core --example million_grid_demo -- <new.library>
#[allow(dead_code)]
#[path = "../../picto-library/examples/grid_query_scale_probe.rs"]
mod fixture;

use image::{codecs::jpeg::JpegEncoder, Rgb, RgbImage};
use picto_library::{Library, SmartFolderInput};
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("provide a NEW .library directory")?,
    );
    if root.exists() {
        return Err("refusing to overwrite an existing path".into());
    }
    std::fs::create_dir(&root)?;
    let mut assets = Vec::new();
    // 256 procedurally drawn patterns, 4096 distinct valid JPEG assets. The
    // library repeats these assets across independent media items/metadata.
    for pattern in 0u32..256 {
        let width = 640 + pattern % 4 * 160;
        let height = 480 + pattern % 5 * 120;
        let mut image = RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let tile = ((x / 64 + y / 64 + pattern) % 2) * 35;
            *pixel = Rgb([
                ((pattern * 37 + x * 110 / width + tile) % 256) as u8,
                ((pattern * 73 + y * 140 / height + tile) % 256) as u8,
                ((pattern * 109 + (x + y) * 70 / (width + height)) % 256) as u8,
            ]);
        }
        let mut original = Vec::new();
        JpegEncoder::new_with_quality(&mut original, 85).encode_image(&image)?;
        let thumb = image::imageops::thumbnail(&image, 512, 512);
        let mut thumbnail = Vec::new();
        JpegEncoder::new_with_quality(&mut thumbnail, 80).encode_image(&thumb)?;
        for variant in 0..16 {
            let comment = format!("Picto synthetic sample {pattern}/{variant}").into_bytes();
            let mut bytes = original[..original.len() - 2].to_vec();
            bytes.extend([0xff, 0xfe]);
            bytes.extend(((comment.len() + 2) as u16).to_be_bytes());
            bytes.extend(comment);
            bytes.extend([0xff, 0xd9]);
            let hash = format!("{:x}", Sha256::digest(&bytes));
            let directory = root.join("blobs/f").join(&hash[..2]).join(&hash[2..4]);
            let thumbs = root.join("blobs/t").join(&hash[..2]).join(&hash[2..4]);
            std::fs::create_dir_all(&directory)?;
            std::fs::create_dir_all(&thumbs)?;
            let path = directory.join(format!("{hash}.jpg"));
            std::fs::write(&path, &bytes)?;
            std::fs::write(thumbs.join(format!("{hash}.jpg")), &thumbnail)?;
            assets.push(fixture::SampleAsset {
                hash,
                path: path.to_string_lossy().into_owned(),
                width,
                height,
                size: bytes.len() as u64,
            });
        }
        if pattern % 32 == 0 {
            eprintln!("ASSETS {}/4096", assets.len());
        }
    }
    fixture::seed_with_assets(&root.join("library.sqlite"), 1_000_000, &assets)?;
    let library = Library::open(root.join("library.sqlite"))?;
    for (label, query) in fixture::queries()
        .into_iter()
        .filter(|(label, _)| label.starts_with("tag_") || label.starts_with("text_"))
    {
        library.create_smart_folder(SmartFolderInput {
            name: label.replace('_', " "),
            parent_id: None,
            icon: None,
            color: None,
            notes: Some("Synthetic million-item grid test".into()),
            view: query.view,
        })?;
    }
    assert_eq!(library.counts()?.all, 1_000_000);
    library.write_projection_checkpoint()?;
    println!(
        "READY {} — 1,000,000 active media items, 4,096 reusable JPEG assets, five smart folders",
        root.display()
    );
    Ok(())
}
