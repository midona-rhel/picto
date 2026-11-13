//! The decoded thumbnails, kept across frames. The grid says which it wants,
//! most wanted first, as `thumbnailPlan.ts` orders them; this loads those a
//! few at a time, drops loads the grid has scrolled away from, and keeps the
//! rest for scrolling back until there are too many.
use gpui::*;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

pub struct Thumbnails {
    entries: HashMap<Resource, ImageCacheItem>,
    recent: VecDeque<Resource>,
    /// Images a view outside the grid is showing, which stay decoded. Such a
    /// view does not repaint as the grid scrolls, so if its image were
    /// evicted it would go on drawing whatever took the freed texture.
    pins: HashMap<&'static str, Resource>,
    /// What the grid's visible band wants.
    wanted: HashSet<Resource>,
    pub hits: u64,
    pub loads: u64,
}

/// Decoded thumbnails kept beyond the ones wanted now.
const CAPACITY: usize = 192;
/// `thumbnailDecodeWorker.ts` runs six loads at once.
const CONCURRENCY: usize = 6;

/// The order to load a band of tiles in: those in the viewport before those
/// in the margin around it, and nearest the viewport's middle first. Each
/// tile is its top and height; the answer is positions in `tiles`.
pub fn load_order(tiles: &[(f32, f32)], view_top: f32, view_height: f32) -> Vec<usize> {
    let middle = view_top + view_height / 2.;
    let mut order = (0..tiles.len()).collect::<Vec<_>>();
    let rank = |index: usize| {
        let (top, height) = tiles[index];
        let outside = top + height <= view_top || top >= view_top + view_height;
        (outside, (top + height / 2. - middle).abs())
    };
    order.sort_by(|a, b| {
        let (a, b) = (rank(*a), rank(*b));
        a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
    });
    order
}

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
            wanted: HashSet::new(),
            hits: 0,
            loads: 0,
        }
    }

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

    fn held(&self, resource: &Resource) -> bool {
        self.wanted.contains(resource) || self.pins.values().any(|pinned| pinned == resource)
    }

    fn start(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) {
        crate::stats::thumbnail_read();
        let item = ImageCacheItem::new(resource, cx);
        // The view that asked is told when it is ready.
        item.use_image(window);
        self.loads += 1;
        self.entries.insert(resource.clone(), item);
        self.recent.push_back(resource.clone());
    }

    /// The grid's thumbnails for this frame, most wanted first. A load that
    /// is no longer wanted is abandoned, the most wanted that are missing
    /// start, and decoded thumbnails beyond the capacity are let go, oldest
    /// first, never one that is wanted or pinned.
    pub fn plan(&mut self, wanted: Vec<Resource>, window: &mut Window, cx: &mut App) {
        self.wanted = wanted.iter().cloned().collect();
        // Dropping an unfinished entry cancels its read and decode.
        let abandoned = self
            .entries
            .iter()
            .filter(|(resource, item)| item.get().is_none() && !self.held(resource))
            .map(|(resource, _)| resource.clone())
            .collect::<Vec<_>>();
        for resource in &abandoned {
            self.entries.remove(resource);
        }
        if !abandoned.is_empty() {
            self.recent.retain(|resource| !abandoned.contains(resource));
        }
        let mut pending = self
            .entries
            .values()
            .filter(|item| item.get().is_none())
            .count();
        for resource in &wanted {
            if pending >= CONCURRENCY {
                break;
            }
            if !self.entries.contains_key(resource) {
                self.start(resource, window, cx);
                pending += 1;
            }
        }
        let capacity = CAPACITY.max(self.wanted.len() + self.pins.len());
        let mut kept = Vec::new();
        while self.entries.len() > capacity {
            let Some(oldest) = self.recent.pop_front() else {
                break;
            };
            if self.held(&oldest) {
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
        for resource in kept.into_iter().rev() {
            self.recent.push_front(resource);
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
        // A grid thumbnail waits for its turn in the plan. Anything else,
        // such as the inspector's preview, loads as soon as it is asked for.
        if !self.wanted.contains(resource) {
            self.start(resource, window, cx);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::load_order;

    #[test]
    fn tiles_load_from_the_middle_of_the_viewport_outward() {
        // A viewport from 1000 to 1600; tiles of height 200.
        let tiles = [
            (500., 200.),  // in the margin above
            (1000., 200.), // top of the viewport
            (1200., 200.), // its middle
            (1400., 200.), // bottom of the viewport
            (1700., 200.), // in the margin below
        ];
        assert_eq!(load_order(&tiles, 1000., 600.), vec![2, 1, 3, 4, 0]);
    }

    #[test]
    fn a_tile_that_only_touches_the_viewport_edge_is_outside_it() {
        let tiles = [(800., 200.), (1600., 100.), (1500., 100.)];
        assert_eq!(load_order(&tiles, 1000., 600.), vec![2, 1, 0]);
    }
}
