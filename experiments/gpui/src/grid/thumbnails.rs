//! Persistent, bounded thumbnail cache. GPUI's frame-scoped asset path is not a
//! scrolling-gallery cache: retain nearby decoded thumbnails across visibility changes.
use gpui::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub struct Thumbnails {
    entries: HashMap<Resource, ImageCacheItem>,
    recent: VecDeque<Resource>,
    /// Images a view outside the grid is showing, which stay decoded. Such a
    /// view does not repaint as the grid scrolls, so if its image were
    /// evicted it would go on drawing whatever took the freed texture.
    pins: HashMap<&'static str, Resource>,
    pub hits: u64,
    pub loads: u64,
}
const CAPACITY: usize = 192;
/// `thumbnailDecodeWorker.ts` runs six loads at once.
const CONCURRENCY: usize = 6;
impl Thumbnails {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|cache, cx| {
            for (_, item) in std::mem::take(&mut cache.entries) {
                if let Some(Ok(image)) = item.get() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        Self {
            entries: HashMap::new(),
            recent: VecDeque::new(),
            pins: HashMap::new(),
            hits: 0,
            loads: 0,
        }
    }
}
impl Thumbnails {
    /// Keeps the image a view shows from being evicted, replacing the one
    /// it kept before. `None` releases it.
    pub fn pin(&mut self, holder: &'static str, resource: Option<Resource>) {
        match resource {
            Some(resource) => self.pins.insert(holder, resource),
            None => self.pins.remove(holder),
        };
    }

    /// Whether this thumbnail is decoded and ready to paint.
    pub fn is_loaded(&self, resource: &Resource) -> bool {
        self.entries
            .get(resource)
            .is_some_and(|item| matches!(item.get(), Some(Ok(_))))
    }

    /// Whether this thumbnail was tried and could not be read or decoded.
    pub fn is_failed(&self, resource: &Resource) -> bool {
        self.entries
            .get(resource)
            .is_some_and(|item| matches!(item.get(), Some(Err(_))))
    }
}
impl ImageCache for Thumbnails {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        if let Some(item) = self.entries.get(resource) {
            self.hits += 1;
            self.recent.retain(|r| r != resource);
            self.recent.push_back(resource.clone());
            return item.use_image(window);
        }
        // A pending entry notifies this view when it finishes, which paints
        // again and gives the tiles turned away here their next chance.
        let pending = self
            .entries
            .values()
            .filter(|item| item.get().is_none())
            .count();
        if pending >= CONCURRENCY {
            return None;
        }
        // Evict the least recently used images that no view has pinned.
        let mut kept = Vec::new();
        while self.entries.len() >= CAPACITY {
            let Some(oldest) = self.recent.pop_front() else {
                break;
            };
            if self.pins.values().any(|pinned| *pinned == oldest) {
                kept.push(oldest);
                continue;
            }
            if let Some(item) = self.entries.remove(&oldest)
                && let Some(Ok(image)) = item.get()
            {
                cx.drop_image(image, Some(window));
                crate::stats::thumbnail_evicted();
            }
        }
        for pinned in kept.into_iter().rev() {
            self.recent.push_front(pinned);
        }
        crate::stats::thumbnail_read();
        let item = ImageCacheItem::new(resource, cx);
        let result = item.use_image(window);
        self.loads += 1;
        self.entries.insert(resource.clone(), item);
        self.recent.push_back(resource.clone());
        result
    }
}
