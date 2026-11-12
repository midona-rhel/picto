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
    pub hits: u64,
    pub loads: u64,
}
const CAPACITY: usize = 192;
const CONCURRENCY: usize = 4;
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
            hits: 0,
            loads: 0,
        }
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
        while self.entries.len() >= CAPACITY {
            let oldest = self.recent.pop_front()?;
            if let Some(item) = self.entries.remove(&oldest)
                && let Some(Ok(image)) = item.get()
            {
                cx.drop_image(image, Some(window));
            }
        }
        let item = ImageCacheItem::new(resource, cx);
        let result = item.use_image(window);
        self.loads += 1;
        self.entries.insert(resource.clone(), item);
        self.recent.push_back(resource.clone());
        result
    }
}
