//! Persistent, bounded thumbnail cache. GPUI's frame-scoped asset path is not a
//! scrolling-gallery cache: retain nearby decoded thumbnails across visibility changes.
use futures::FutureExt;
use gpui::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub struct Thumbnails {
    entries: HashMap<Resource, ImageCacheItem>,
    recent: VecDeque<Resource>,
    in_flight: Arc<AtomicUsize>,
    pub hits: u64,
    pub loads: u64,
}
const CAPACITY: usize = 192;
const CONCURRENCY: usize = 4;
impl Thumbnails {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|cache, cx| {
            for (_, mut item) in std::mem::take(&mut cache.entries) {
                if let Some(Ok(image)) = item.get() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        Self {
            entries: HashMap::new(),
            recent: VecDeque::new(),
            in_flight: Arc::new(AtomicUsize::new(0)),
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
        if let Some(item) = self.entries.get_mut(resource) {
            self.hits += 1;
            self.recent.retain(|r| r != resource);
            self.recent.push_back(resource.clone());
            return item.get();
        }
        if self.in_flight.load(Ordering::Relaxed) >= CONCURRENCY {
            return None;
        }
        while self.entries.len() >= CAPACITY {
            let oldest = self.recent.pop_front()?;
            if let Some(mut item) = self.entries.remove(&oldest) {
                if let Some(Ok(image)) = item.get() {
                    cx.drop_image(image, Some(window));
                }
            }
        }
        let future = ImageAssetLoader::load(resource.clone(), cx);
        let active = self.in_flight.clone();
        active.fetch_add(1, Ordering::Relaxed);
        self.loads += 1;
        let task = cx
            .background_executor()
            .spawn(async move {
                let result = future.await;
                active.fetch_sub(1, Ordering::Relaxed);
                result
            })
            .shared();
        self.entries
            .insert(resource.clone(), ImageCacheItem::Loading(task.clone()));
        self.recent.push_back(resource.clone());
        let view = window.current_view();
        window
            .spawn(cx, async move |cx| {
                let _ = task.await;
                cx.on_next_frame(move |_, cx| cx.notify(view));
            })
            .detach();
        None
    }
}
