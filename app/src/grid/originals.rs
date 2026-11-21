//! Originals for tiles that are too large for their thumbnail. A thumbnail's
//! long edge is 512 pixels. A still image whose tile draws more pixels than
//! its thumbnail holds, and that has been in the viewport for a fifth of a
//! second, gets its original decoded to exactly the size the tile shows.
//! This happens whether the grid is scrolling or not.
use gpui::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The length in pixels of the long edge that thumbnails are stored with.
pub const THUMBNAIL_LONG_EDGE: f32 = 512.;
/// How long a tile must be in the viewport before its original is read. The
/// viewers wait the same time before they read an item's original
/// ([`crate::motion::ORIGINAL_DELAY`]).
pub const DWELL: Duration = crate::motion::ORIGINAL_DELAY;
/// The number of originals decoded at the same time. Originals are large,
/// and decoding more of them at once would slow down the rest of the app.
const CONCURRENCY: usize = 2;

/// Returns the width and the height in pixels to decode an original to for
/// a tile: the size at which the tile shows the picture at the display scale
/// `scale`, and never more than the source has. With `contain` the picture
/// fits inside the tile, and without it the picture covers the tile. When
/// the size of the source is unknown, the tile's own size in pixels is
/// returned.
pub fn decode_size(
    tile: (f32, f32),
    source: Option<(u32, u32)>,
    contain: bool,
    scale: f32,
) -> (u32, u32) {
    let scale = scale.max(1.);
    match source {
        Some((width, height)) if width > 0 && height > 0 => {
            let (across, down) = (tile.0 / width as f32, tile.1 / height as f32);
            let fit = if contain {
                across.min(down)
            } else {
                across.max(down)
            };
            let side = |pixels: u32| ((pixels as f32 * fit * scale).ceil() as u32).clamp(1, pixels);
            (side(width), side(height))
        }
        _ => (
            ((tile.0 * scale).ceil() as u32).max(1),
            ((tile.1 * scale).ceil() as u32).max(1),
        ),
    }
}

/// Whether a tile should show its original. It should when the item is a
/// still image other than a GIF or a JPEG XL, and the tile draws it with
/// more pixels than its thumbnail holds.
pub fn wants_original(
    mime: &str,
    tile: (f32, f32),
    source: Option<(u32, u32)>,
    contain: bool,
    scale: f32,
) -> bool {
    if !mime.starts_with("image/") || mime == "image/gif" || mime == "image/jxl" {
        return false;
    }
    let (width, height) = decode_size(tile, source, contain, scale);
    match source {
        Some((source_width, source_height)) if source_width > 0 && source_height > 0 => {
            let shrink = (THUMBNAIL_LONG_EDGE / source_width.max(source_height) as f32).min(1.);
            let stored = |pixels: u32| ((pixels as f32 * shrink).floor() as u32).max(1);
            width > stored(source_width) || height > stored(source_height)
        }
        _ => width as f32 > THUMBNAIL_LONG_EDGE || height as f32 > THUMBNAIL_LONG_EDGE,
    }
}

/// A tile in the viewport that should show its original: the item's content
/// hash, the path of the original, and the size to decode it to.
pub struct Want {
    pub hash: String,
    pub path: PathBuf,
    pub size: (u32, u32),
}

enum Entry {
    /// The original is being decoded at `size`. `showing` is the picture
    /// decoded at an earlier size. It stays on screen until the new one is
    /// ready, which happens after the user changes the size of the tiles.
    Loading {
        size: (u32, u32),
        showing: Option<Arc<RenderImage>>,
        _decode: Task<()>,
    },
    Ready((u32, u32), Arc<RenderImage>),
    /// The original could not be read or decoded. The tile keeps its
    /// thumbnail.
    Failed,
}

impl Entry {
    /// Returns the picture that this entry has on screen, if it has one.
    fn image(&self) -> Option<Arc<RenderImage>> {
        match self {
            Entry::Ready(_, image) => Some(image.clone()),
            Entry::Loading { showing, .. } => showing.clone(),
            Entry::Failed => None,
        }
    }
}

pub struct Originals {
    /// For each wanted tile, by content hash, the time since which it has
    /// been in the viewport without leaving it.
    seen: HashMap<String, Instant>,
    entries: HashMap<String, Entry>,
    /// A timer that draws the grid again when the next tile has been in the
    /// viewport for `DWELL`.
    wake: Option<Task<()>>,
}

impl Originals {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|originals, cx| {
            for (_, entry) in std::mem::take(&mut originals.entries) {
                if let Some(image) = entry.image() {
                    crate::atlas::release(image, cx);
                }
            }
        })
        .detach();
        Self {
            seen: HashMap::new(),
            entries: HashMap::new(),
            wake: None,
        }
    }

    /// Returns the decoded original of the item with the content hash
    /// `hash`, once it is ready.
    pub fn get(&self, hash: &str) -> Option<Arc<RenderImage>> {
        self.entries.get(hash)?.image()
    }

    /// Takes the tiles in the viewport that should show their original. The
    /// original of a tile that has left the viewport is released, and the
    /// tile waits for `DWELL` again when it comes back. A tile that has been
    /// in the viewport for `DWELL` has its original decoded, and at most
    /// `CONCURRENCY` originals are decoded at the same time.
    pub fn plan(&mut self, wants: Vec<Want>, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.seen
            .retain(|hash, _| wants.iter().any(|want| want.hash == *hash));
        let gone = self
            .entries
            .keys()
            .filter(|hash| !wants.iter().any(|want| want.hash == **hash))
            .cloned()
            .collect::<Vec<_>>();
        for hash in gone {
            if let Some(image) = self.entries.remove(&hash).and_then(|entry| entry.image()) {
                crate::atlas::release(image, cx);
            }
        }
        let mut loading = self
            .entries
            .values()
            .filter(|entry| matches!(entry, Entry::Loading { .. }))
            .count();
        let mut next_dwell: Option<Duration> = None;
        for want in wants {
            let since = *self.seen.entry(want.hash.clone()).or_insert(now);
            let current = match self.entries.get(&want.hash) {
                Some(Entry::Loading { size, .. } | Entry::Ready(size, _)) => Some(*size),
                Some(Entry::Failed) => continue,
                None => None,
            };
            // The size wanted changes when the user changes the size of the
            // tiles. The picture decoded at the old size is shown until the
            // new one is ready.
            if current == Some(want.size) {
                continue;
            }
            let waited = now.duration_since(since);
            if waited < DWELL {
                let left = DWELL - waited;
                next_dwell = Some(next_dwell.map_or(left, |soonest| soonest.min(left)));
                continue;
            }
            if loading >= CONCURRENCY {
                continue;
            }
            loading += 1;
            let (hash, path, size) = (want.hash, want.path, want.size);
            let decode = cx.background_executor().spawn(async move {
                // A JPEG is decoded at a reduced size that still covers the
                // tile's size, which takes less time and memory than decoding
                // it at its full size.
                let image = crate::viewer::scaled::decode(&path, size, false).ok()?;
                let mut pixels = image
                    .resize_exact(size.0, size.1, image::imageops::FilterType::CatmullRom)
                    .into_rgba8();
                // GPUI expects each pixel as blue, green, red and alpha.
                for pixel in pixels.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                Some(Arc::new(RenderImage::new(smallvec::smallvec![
                    image::Frame::new(pixels)
                ])))
            });
            let key = hash.clone();
            let task = cx.spawn(async move |this, cx| {
                let decoded = decode.await;
                let _ = this.update(cx, |this: &mut Self, cx| {
                    // The result is used only if the tile still waits for a
                    // decode at this size.
                    let waiting = matches!(
                        this.entries.get(&key),
                        Some(Entry::Loading { size: wanted, .. }) if *wanted == size
                    );
                    if !waiting {
                        return;
                    }
                    let previous = this.entries.remove(&key).and_then(|entry| entry.image());
                    let entry = match (decoded, previous.clone()) {
                        (Some(image), _) => Entry::Ready(size, image),
                        // When the decode at a new size fails, the picture that
                        // was showing is kept.
                        (None, Some(image)) => Entry::Ready(size, image),
                        (None, None) => Entry::Failed,
                    };
                    if let (Entry::Ready(_, image), Some(previous)) = (&entry, previous)
                        && !Arc::ptr_eq(image, &previous)
                    {
                        crate::atlas::release(previous, cx);
                    }
                    this.entries.insert(key, entry);
                    cx.notify();
                });
            });
            let showing = self.entries.remove(&hash).and_then(|entry| entry.image());
            self.entries.insert(
                hash,
                Entry::Loading {
                    size,
                    showing,
                    _decode: task,
                },
            );
        }
        self.wake = next_dwell.map(|left| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(left).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_size, wants_original};

    #[test]
    fn an_original_is_decoded_to_what_the_tile_shows() {
        // A 4000×3000 source in a 400×300 tile on a 2× display is decoded to
        // 800×600.
        assert_eq!(
            decode_size((400., 300.), Some((4000, 3000)), false, 2.),
            (800, 600)
        );
        // The size is never more than the source has.
        assert_eq!(
            decode_size((400., 300.), Some((600, 450)), false, 2.),
            (600, 450)
        );
        // Without `contain` the picture covers the tile, and with it the
        // picture fits inside the tile.
        assert_eq!(
            decode_size((300., 300.), Some((4000, 2000)), false, 1.),
            (600, 300)
        );
        assert_eq!(
            decode_size((300., 300.), Some((4000, 2000)), true, 1.),
            (300, 150)
        );
        // When the size of the source is unknown, the tile's own size in
        // pixels is used.
        assert_eq!(decode_size((200.5, 100.), None, false, 2.), (401, 200));
    }

    #[test]
    fn only_a_tile_that_outgrows_its_thumbnail_gets_the_original() {
        let source = Some((4000, 3000));
        // The 512 px thumbnail of a 4000×3000 source is 512×384. A 250×188
        // tile at 2× needs 500×376, so the thumbnail is enough.
        assert!(!wants_original(
            "image/jpeg",
            (250., 187.5),
            source,
            false,
            2.
        ));
        // A 300×225 tile at 2× needs 600×450, so the thumbnail is not enough.
        assert!(wants_original(
            "image/jpeg",
            (300., 225.),
            source,
            false,
            2.
        ));
        // A source that is no larger than its thumbnail never has its original
        // decoded.
        assert!(!wants_original(
            "image/png",
            (600., 600.),
            Some((400, 400)),
            false,
            2.
        ));
        // The originals of GIFs, of JPEG XL pictures and of videos are never
        // decoded here.
        assert!(!wants_original(
            "image/gif",
            (600., 600.),
            source,
            false,
            2.
        ));
        assert!(!wants_original(
            "image/jxl",
            (600., 600.),
            source,
            false,
            2.
        ));
        assert!(!wants_original(
            "video/mp4",
            (600., 600.),
            source,
            false,
            2.
        ));
    }
}
