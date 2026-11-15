//! What the grid's frame reads before it draws: the session's scroll and
//! restore, the layout of the window of results, the visible tiles copied
//! from the session, and the thumbnails and originals they load.
use super::layout::Layout;
use super::prefs::ViewPrefs;
use super::thumbnails::{self, Thumb};
use super::tile::{TileData, TileLook, aspect, media_rect};
use super::{GridView, Surface, empty, layout, nav, originals, pointer, reveal, window};
use gpui::*;
use picto_library::query::ItemScope;

/// The visible tiles and what the frame draws them with.
pub(super) struct Frame {
    pub fade: f32,
    pub animating: bool,
    pub prefs: ViewPrefs,
    pub surface: Surface,
    /// The destination holds more than the window of results loaded.
    pub windowed: bool,
    pub tiles: Vec<TileData>,
}

impl GridView {
    /// The frame's start: the rubber band's scrolling at the edges, the
    /// session told where the grid is, and the scroll a restored grid
    /// opens at; the restored window's top, if any.
    pub(super) fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<f32> {
        // A rubber band near the top or bottom edge scrolls the grid each
        // frame; the rectangle follows below.
        if pointer::autoscroll(self.marquee.as_ref(), &self.scroll, self.raw_scroll()) {
            window.request_animation_frame();
        }
        // The session moves its scope change along, hears how far this grid
        // is scrolled and where its window lies, and says where a restored
        // grid should open.
        let scrolled = self.raw_scroll();
        let epoch = self.epoch;
        let window_top = self.placement.top;
        // A press, rubber band or drag in progress holds back a refresh of
        // the results, which would move what it is on.
        let busy = self.marquee.is_some() || self.dragging || self.press.is_some();
        let restore = self.session.update(cx, |session, cx| {
            session.advance_transition(cx);
            session.pointer_busy = busy;
            if session.results_epoch == epoch {
                session.grid_scroll = scrolled;
                session.window_top = window_top;
                None
            } else {
                Some((
                    session.restore_scroll.take().unwrap_or(0.),
                    session.restore_window_top.take(),
                ))
            }
        });
        let mut restored_top = None;
        if let Some((offset, top)) = restore {
            self.scroll.set_offset(point(px(0.), px(-offset)));
            restored_top = top;
        }
        restored_top
    }

    /// The items scrolled to `top`: whether they moved since the last
    /// frame. Any scroll ends the hover and closes the preview at once.
    pub(super) fn follow_scroll(&mut self, top: f32, cx: &mut Context<Self>) -> bool {
        // Any scroll ends the hover and closes the preview at once; the
        // hover comes back 80 ms after the last one, where the pointer is.
        let moved = top != self.last_top;
        if moved {
            self.last_top = top;
            self.hovered = None;
            // The anchor was for where the scroll was when it asked.
            self.window_anchor = None;
            self.preview.close(cx);
            self.scrolled = std::time::Instant::now();
            self.settle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::motion::MAGNIFIER_RESUME)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(pointer) = this.pointer {
                        this.pointer_moved(pointer, cx);
                    }
                    cx.notify();
                });
            }));
        }
        moved
    }

    /// The results laid out for `width` and the tiles of a `viewport` tall
    /// view scrolled to `top`, which keeping the topmost tile in place or
    /// revealing an item may move: the scroll it ends at, and the frame.
    pub(super) fn snapshot(
        &mut self,
        width: f32,
        viewport: f32,
        mut top: f32,
        restored_top: Option<f32>,
        cx: &mut Context<Self>,
    ) -> (f32, Frame) {
        // Everything the frame needs from the session, copied for the visible
        // tiles only, so the listeners below can borrow the context.
        let mut measured = None;
        let mut revealed = None;
        let (fade, animating, prefs, surface, windowed, tiles) = {
            let session = self.session.read(cx);
            let fresh = session.results_epoch != self.epoch;
            if fresh {
                self.epoch = session.results_epoch;
                self.layout_key = None;
                self.window_anchor = None;
                // Positions now mean other items.
                self.marquee = None;
                self.hovered = None;
            }
            let prefs = session.prefs.clone();
            let text_height = prefs.text_height();
            self.text_height = text_height;
            let geometry = (
                width as u32,
                session.tile as u32,
                prefs.gap as u32,
                prefs.mode,
                text_height as u32,
            );
            let key = Some((
                geometry.0,
                geometry.1,
                geometry.2,
                geometry.3,
                geometry.4,
                session.window_epoch,
            ));
            if key != self.layout_key {
                let laying_out = crate::stats::Started::now();
                let window_changed = self.layout_key.map(|key| key.5) != key.map(|key| key.5);
                if window_changed {
                    // Tiles of the window before mean other items now.
                    self.marquee = None;
                    self.hovered = None;
                    self.reorder = None;
                }
                let aspects = session
                    .items
                    .iter()
                    // `gridLayoutModel.ts`: an unknown shape lays out as 3:2.
                    .map(|item| aspect(item).unwrap_or(1.5))
                    .collect::<Vec<_>>();
                let compute = |aspects: &[f64]| {
                    Layout::compute(
                        aspects,
                        width,
                        session.tile,
                        prefs.gap,
                        prefs.mode,
                        text_height,
                    )
                };
                let next = compute(&aspects);
                // The height per item, measured on the first items of the
                // window the destination opened with (or of the one held
                // when the geometry changed), as `densityRef` keeps it.
                let density = match session.density {
                    Some((measured_for, density)) if measured_for == geometry => density,
                    _ => {
                        let count = aspects.len().min(window::SAMPLE);
                        let height = if count == aspects.len() {
                            next.total_height
                        } else {
                            compute(&aspects[..count]).total_height
                        };
                        let density = window::density(height, count).unwrap_or(1.);
                        if count > 0 {
                            measured = Some((geometry, density));
                        }
                        density
                    }
                };
                let anchor = self.window_anchor.filter(|_| window_changed);
                let mut placement = window::place(
                    next.total_height,
                    session.start,
                    session.items.len(),
                    session.total as usize,
                    density,
                    anchor,
                    |offset| next.tiles.get(offset).map(|tile| tile.y),
                );
                // A restored window lies where it lay when it was left.
                if let Some(restored) = restored_top {
                    let span = (placement.height - next.total_height).max(0.);
                    placement.top = restored.clamp(0., span);
                    placement.bottom = placement.top + next.total_height;
                }
                let previous = std::mem::replace(&mut self.layout, next);
                let previous_top = std::mem::replace(&mut self.placement, placement).top;
                // The same results laid out anew (zoom, spacing, layout mode,
                // labels, width), another window of them, or the window read
                // again in place: the topmost visible tile, followed by its
                // item, stays where it was on screen, as `CanvasGrid` keeps it.
                let ids = session
                    .items
                    .iter()
                    .map(|item| item.root_id.0)
                    .collect::<Vec<_>>();
                if self.layout_key.is_some() && !fresh {
                    let now = ids
                        .iter()
                        .enumerate()
                        .map(|(index, id)| (*id, index))
                        .collect::<std::collections::HashMap<_, _>>();
                    let shift = |tiles: &[layout::Tile], by: f32| {
                        tiles
                            .iter()
                            .map(|tile| layout::Tile {
                                y: tile.y + by,
                                ..*tile
                            })
                            .collect::<Vec<_>>()
                    };
                    // In the content's own scroll, below the subfolder
                    // header, so a header in view stays where it is.
                    let header = self.subfolders.height();
                    let before = shift(&previous.tiles, previous_top + header);
                    let after = shift(&self.layout.tiles, self.placement.top + header);
                    let selected = self
                        .layout_ids
                        .iter()
                        .enumerate()
                        .filter(|(index, id)| {
                            session
                                .selection
                                .contains(picto_library::RootId(**id), self.layout_start + index)
                        })
                        .map(|(index, _)| index)
                        .collect();
                    let moved = |index: usize| {
                        self.layout_ids
                            .get(index)
                            .and_then(|id| now.get(id).copied())
                    };
                    let raw = top + header;
                    if let Some(anchored) =
                        layout::scroll_anchor(&before, &after, &selected, raw, viewport, moved)
                        && anchored != raw
                    {
                        top = anchored - header;
                        self.last_top = top;
                        self.set_scroll_top(top);
                    }
                }
                self.layout_ids = ids;
                self.layout_start = session.start;
                self.layout_key = key;
                crate::stats::grid_part(crate::stats::GridPart::Layout, laying_out);
            }
            // An item moved to (a key, the viewer) comes into view once its
            // window is laid out.
            if let Some(ordinal) = session.reveal
                && let Some(tile) = ordinal
                    .checked_sub(session.start)
                    .and_then(|index| self.placed(index))
            {
                if let Some(offset) = nav::scroll_into_view(&tile, top, viewport, prefs.gap) {
                    top = offset.max(-self.subfolders.height());
                    self.last_top = top;
                    self.set_scroll_top(top);
                }
                revealed = Some(ordinal);
            }
            if let Some(marquee) = &mut self.marquee {
                marquee.track(top - self.placement.top, &self.layout, text_height);
            }
            // `GridScreen.tsx`: an error first, then the tiles, and only a
            // scope known to be empty says so; a loading one shows nothing.
            let surface = if let Some(error) = &session.error {
                Surface::Error(error.clone())
            } else if !session.items.is_empty()
                || (!self.subfolders.children.is_empty() && !session.loading)
            {
                Surface::Tiles
            } else if session.loading && !session.refreshing {
                Surface::Loading
            } else {
                let scope = session
                    .nav
                    .get(session.displayed)
                    .map_or(ItemScope::All, |entry| entry.scope.clone());
                Surface::Empty(empty::copy(
                    &scope,
                    session.searching(),
                    session.filter.is_some(),
                ))
            };
            // Only the window's tiles are laid out; the rest of the height
            // is the destination's other items, drawn once their window
            // arrives.
            let local_top = top - self.placement.top;
            let marquee_base = self.marquee.as_ref().map(|_| {
                (0..self.layout.tiles.len())
                    .filter(|index| session.is_selected(session.start + index))
                    .collect::<std::collections::BTreeSet<_>>()
            });
            let tiles = self
                .layout
                .visible(
                    local_top - reveal::MARGIN,
                    local_top + viewport + reveal::MARGIN,
                )
                .into_iter()
                .filter_map(|index| {
                    let item = session.items.get(index)?;
                    let ordinal = session.start + index;
                    let selected = match (&self.marquee, &marquee_base) {
                        (Some(marquee), Some(base)) => marquee.shows_selected(index, base),
                        _ => session.selection.contains(item.root_id, ordinal),
                    };
                    Some(TileData {
                        index,
                        ordinal,
                        look: TileLook {
                            tile: self.placed(index)?,
                            item: item.clone(),
                            thumbnail: session.thumbnail(&item.content_hash),
                            selected,
                            thumb: Thumb::Pending,
                            original: None,
                            // Set as the tile is drawn.
                            alphas: (0., 0.),
                            magnifier: false,
                            grayscale: false,
                        },
                    })
                })
                .collect::<Vec<_>>();
            (
                session.transition.opacity(),
                session.transition.animating(),
                prefs,
                surface,
                (session.total as usize) > session.items.len() && !session.loading,
                tiles,
            )
        };
        if measured.is_some() || revealed.is_some() {
            self.session.update(cx, |session, _| {
                if measured.is_some() {
                    session.density = measured;
                }
                if revealed.is_some() && session.reveal == revealed {
                    session.reveal = None;
                }
            });
        }
        (
            top,
            Frame {
                fade,
                animating,
                prefs,
                surface,
                windowed,
                tiles,
            },
        )
    }

    /// The thumbnails and originals the visible tiles need, from the
    /// middle of the viewport outward, and each tile's picture as it
    /// stands; a frame is asked for while one fades in.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn load_pictures(
        &mut self,
        tiles: &mut [TileData],
        prefs: &ViewPrefs,
        top: f32,
        viewport: f32,
        now: f64,
        animating: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Thumbnails load from the middle of the viewport outward.
        let spans = tiles
            .iter()
            .map(|data| (data.look.tile.y, data.look.tile.h))
            .collect::<Vec<_>>();
        // The subfolder covers first while the header is in view.
        let header_wanted = if self.raw_scroll() < self.subfolders.height() {
            self.subfolders.wanted()
        } else {
            Vec::new()
        };
        let wanted = header_wanted
            .into_iter()
            .chain(
                thumbnails::load_order(&spans, top, viewport)
                    .into_iter()
                    .map(|at| Resource::Path(tiles[at].look.thumbnail.clone().into())),
            )
            .collect();
        self.thumbnails
            .update(cx, |thumbnails, cx| thumbnails.plan(wanted, window, cx));
        let mut missing = Vec::new();
        let decoded = {
            let thumbnails = self.thumbnails.read(cx);
            tiles
                .iter_mut()
                .map(|data| {
                    let resource = Resource::Path(data.look.thumbnail.clone().into());
                    data.look.thumb = thumbnails.thumb(&resource, &data.look.item.mime);
                    if data.look.thumb == Thumb::Failed {
                        missing.push(thumbnails::Missing {
                            hash: data.look.item.content_hash.clone(),
                            mime: data.look.item.mime.clone(),
                            jpeg: data.look.thumbnail.clone(),
                        });
                    }
                    (data.look.item.root_id.0, data.look.thumb == Thumb::Loaded)
                })
                .collect::<Vec<_>>()
        };
        let session = self.session.clone();
        self.thumbnails.update(cx, |thumbnails, cx| {
            thumbnails.make_missing(&session, missing, cx)
        });
        // Tiles in the viewport that draw more pixels than their thumbnail
        // holds ask for their original, which arrives once the grid has
        // rested a moment.
        let scale = window.scale_factor();
        let wants = {
            let session = self.session.read(cx);
            tiles
                .iter()
                .filter(|data| {
                    data.look.thumb == Thumb::Loaded
                        && data.look.tile.y + data.look.tile.h > top
                        && data.look.tile.y < top + viewport
                })
                .filter_map(|data| {
                    let (contain, (_, _, w, h)) =
                        media_rect(&data.look.tile, &data.look.item, &prefs);
                    let source = data.look.item.width.zip(data.look.item.height);
                    originals::wants_original(&data.look.item.mime, (w, h), source, contain, scale)
                        .then(|| originals::Want {
                            hash: data.look.item.content_hash.clone(),
                            path: session
                                .original(&data.look.item.content_hash, &data.look.item.mime),
                            size: originals::decode_size((w, h), source, contain, scale),
                        })
                })
                .collect::<Vec<_>>()
        };
        self.originals
            .update(cx, |originals, cx| originals.plan(wants, top, cx));
        {
            let originals = self.originals.read(cx);
            for data in tiles.iter_mut() {
                data.look.original = originals.get(&data.look.item.content_hash);
            }
        }
        self.reveal.update(&decoded, now);
        // Keep painting while a fade or a thumbnail reveal is in motion.
        if animating || self.reveal.active(now) {
            window.request_animation_frame();
        }
    }
}
