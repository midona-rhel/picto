//! Originals for tiles too large for their thumbnail (`thumbnailPipeline.ts`,
//! `thumbnailPlan.ts`). A thumbnail's long edge is 512 pixels; a tile that
//! needs more than that, and has stayed in the viewport for a quarter of a
//! second, gets its original decoded to exactly the size the tile shows.
use gpui::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The long edge thumbnails are stored at.
pub const THUMBNAIL_LONG_EDGE: f32 = 512.;
/// `FULL_QUALITY_VIEWPORT_DWELL_MS`: how long a tile stays in view first.
pub const DWELL: Duration = Duration::from_millis(250);
/// Originals decoded at once. They are large, and the grid stays responsive
/// only if they do not crowd out everything else.
const CONCURRENCY: usize = 2;

/// The pixels to decode an original to for a tile (`fullQualityDecodeSize`):
/// what the tile shows at this display scale, never more than the source has.
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

/// Whether a tile should show its original (`shouldLoadFullQualityOriginal`):
/// a still image, not a GIF or JPEG XL, that the tile draws with more pixels
/// than its thumbnail holds.
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

/// One tile in the viewport that should show its original.
pub struct Want {
    pub hash: String,
    pub path: PathBuf,
    pub size: (u32, u32),
}

enum Entry {
    /// Being decoded at this size. After a zoom, the decode at the previous
    /// size stays on screen until this one lands.
    Loading {
        size: (u32, u32),
        showing: Option<Arc<RenderImage>>,
        _decode: Task<()>,
    },
    Ready((u32, u32), Arc<RenderImage>),
    /// The original could not be read or decoded; the thumbnail stays.
    Failed,
}

impl Entry {
    /// The image this entry has on screen, if any.
    fn image(self) -> Option<Arc<RenderImage>> {
        match self {
            Entry::Ready(_, image) => Some(image),
            Entry::Loading { showing, .. } => showing,
            Entry::Failed => None,
        }
    }
}

pub struct Originals {
    /// Since when each wanted tile has been in the viewport without leaving.
    seen: HashMap<String, Instant>,
    entries: HashMap<String, Entry>,
    /// Paints again when the next tile's dwell is over.
    wake: Option<Task<()>>,
}

impl Originals {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|originals, cx| {
            for (_, entry) in std::mem::take(&mut originals.entries) {
                if let Some(image) = entry.image() {
                    cx.drop_image(image, None);
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

    /// The decoded original for a tile, once it is ready.
    pub fn get(&self, hash: &str) -> Option<Arc<RenderImage>> {
        match self.entries.get(hash)? {
            Entry::Ready(_, image) => Some(image.clone()),
            Entry::Loading { showing, .. } => showing.clone(),
            Entry::Failed => None,
        }
    }

    /// The tiles in the viewport that should show their original. One that
    /// has left is let go and its dwell starts over when it returns; one
    /// that has stayed long enough is decoded, a couple at a time.
    pub fn plan(&mut self, wants: Vec<Want>, window: &mut Window, cx: &mut Context<Self>) {
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
            if let Some(image) = self.entries.remove(&hash).and_then(Entry::image) {
                cx.drop_image(image, Some(window));
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
            // A zoom changes the size wanted; the old decode shows meanwhile.
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
                // A JPEG decodes at the tile's size, not its own.
                let image = crate::viewer::scaled::decode(&path, size, false).ok()?;
                let mut pixels = image
                    .resize_exact(size.0, size.1, image::imageops::FilterType::CatmullRom)
                    .into_rgba8();
                // GPUI's images are blue-green-red.
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
                    // Only if the tile is still waiting for exactly this.
                    let waiting = matches!(
                        this.entries.get(&key),
                        Some(Entry::Loading { size: wanted, .. }) if *wanted == size
                    );
                    if !waiting {
                        return;
                    }
                    let previous = this.entries.remove(&key).and_then(Entry::image);
                    let entry = match (decoded, previous.clone()) {
                        (Some(image), _) => Entry::Ready(size, image),
                        // A failed re-decode keeps what was showing.
                        (None, Some(image)) => Entry::Ready(size, image),
                        (None, None) => Entry::Failed,
                    };
                    if let (Entry::Ready(_, image), Some(previous)) = (&entry, previous)
                        && !Arc::ptr_eq(image, &previous)
                    {
                        cx.drop_image(previous, None);
                    }
                    this.entries.insert(key, entry);
                    cx.notify();
                });
            });
            let showing = self.entries.remove(&hash).and_then(Entry::image);
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
        // A 4000×3000 source in a 400×300 tile on a 2× display.
        assert_eq!(
            decode_size((400., 300.), Some((4000, 3000)), false, 2.),
            (800, 600)
        );
        // Never more than the source has.
        assert_eq!(
            decode_size((400., 300.), Some((600, 450)), false, 2.),
            (600, 450)
        );
        // Cover fills the tile, contain fits inside it.
        assert_eq!(
            decode_size((300., 300.), Some((4000, 2000)), false, 1.),
            (600, 300)
        );
        assert_eq!(
            decode_size((300., 300.), Some((4000, 2000)), true, 1.),
            (300, 150)
        );
        // Unknown source: the tile's own pixels.
        assert_eq!(decode_size((200.5, 100.), None, false, 2.), (401, 200));
    }

    #[test]
    fn only_a_tile_that_outgrows_its_thumbnail_gets_the_original() {
        let source = Some((4000, 3000));
        // 512px thumbnail of 4000×3000 is 512×384. A 250×188 tile at 2× needs
        // 500×376: the thumbnail is enough.
        assert!(!wants_original(
            "image/jpeg",
            (250., 187.5),
            source,
            false,
            2.
        ));
        // A 300×225 tile at 2× needs 600×450: it is not.
        assert!(wants_original(
            "image/jpeg",
            (300., 225.),
            source,
            false,
            2.
        ));
        // A source no larger than its thumbnail never upgrades.
        assert!(!wants_original(
            "image/png",
            (600., 600.),
            Some((400, 400)),
            false,
            2.
        ));
        // Animated and JPEG XL originals are never decoded here, nor video.
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
