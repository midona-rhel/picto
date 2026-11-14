//! The decoded thumbnails, kept across frames. The grid says which it wants,
//! most wanted first, as `thumbnailPlan.ts` orders them; this loads those a
//! few at a time, drops loads the grid has scrolled away from, and keeps the
//! rest for scrolling back until they take more memory than the budget.
use crate::i18n::t;
use gpui::*;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct Thumbnails {
    /// Each thumbnail, and when it was last used (`clock`), for letting go
    /// of the least recently used first. A use only stamps its entry: a
    /// resource is a path, and comparing paths walks their components, so
    /// nothing is searched for in a list on the way.
    entries: HashMap<Resource, Entry>,
    clock: u64,
    /// Images a view outside the grid is showing, which stay decoded. Such a
    /// view does not repaint as the grid scrolls, so if its image were
    /// evicted it would go on drawing whatever took the freed texture.
    pins: HashMap<&'static str, Resource>,
    /// What the grid's visible band wants.
    wanted: HashSet<Resource>,
    /// The view that planned `wanted` (the grid), whose thumbnails wait
    /// their turn in the plan.
    planner: Option<EntityId>,
    /// Missing thumbnails being made.
    renders: Renders,
}

/// Where a tile's thumbnail is.
#[derive(Clone, Copy, PartialEq)]
pub enum Thumb {
    Pending,
    Loaded,
    /// It could not be read or decoded: the tile shows the broken page.
    Failed,
}

/// A file a tile shows whose thumbnail could not be read.
pub struct Missing {
    pub hash: String,
    pub mime: String,
    /// The `.jpg` the tile reads.
    pub jpeg: PathBuf,
}

/// The missing thumbnails being made, a couple at a time, of the tiles that
/// still show them.
#[derive(Default)]
struct Renders {
    /// The tiles on screen whose thumbnail is missing, waiting their turn.
    waiting: Vec<Missing>,
    /// Being made now, each by its file.
    running: HashMap<String, Task<()>>,
    /// Each file is tried once: one that could not be made stays broken.
    tried: HashSet<String>,
}

struct Entry {
    item: ImageCacheItem,
    /// When it was last used, by `Thumbnails::clock`.
    used: u64,
    /// It was painted at least once.
    drawn: bool,
}

impl Renders {
    /// The tiles on screen whose thumbnail is missing now: those not tried
    /// yet wait their turn in this order, any that left the screen are
    /// dropped, and as many as there is room for start, which are returned.
    fn admit(&mut self, missing: Vec<Missing>) -> Vec<Missing> {
        self.waiting = missing
            .into_iter()
            .filter(|file| {
                !self.tried.contains(&file.hash) && !self.running.contains_key(&file.hash)
            })
            .collect();
        let room = RENDERS
            .saturating_sub(self.running.len())
            .min(self.waiting.len());
        let starting = self.waiting.drain(..room).collect::<Vec<_>>();
        for file in &starting {
            self.tried.insert(file.hash.clone());
        }
        starting
    }
}

/// What a decoded thumbnail usually takes: 512 × 384 pixels (a 4:3 picture
/// at the long edge thumbnails are stored at), four bytes each.
const TYPICAL_BYTES: usize = (crate::grid::originals::THUMBNAIL_LONG_EDGE
    * crate::grid::originals::THUMBNAIL_LONG_EDGE
    * 0.75) as usize
    * 4;
/// The least an entry is counted at, so thumbnails that could not be read
/// are let go too.
const ENTRY_BYTES: usize = 4096;

/// How many bytes of decoded thumbnails are kept, wanted ones included: a
/// window full of the smallest tiles, and a window's worth either side.
pub fn budget(window: (f32, f32)) -> usize {
    let across = (window.0 / crate::grid::nav::SMALLEST_TILE).ceil().max(1.);
    let down = (window.1 / crate::grid::nav::SMALLEST_TILE).ceil().max(1.);
    3 * (across * down) as usize * TYPICAL_BYTES
}

/// `thumbnailDecodeWorker.ts` runs six loads at once.
const CONCURRENCY: usize = 6;
/// Missing thumbnails made at once: a Flash movie plays for seconds to give
/// its picture, and a paged document is opened and drawn.
const RENDERS: usize = 2;

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

/// `find_thumbnail_path`, the media protocol's rule: a thumbnail is the
/// `.jpg` the library writes, else the `.png` written for audio waveforms,
/// captured frames and by older libraries. This is the `.png` to try.
pub fn png_for(jpeg: &Path) -> PathBuf {
    jpeg.with_extension("png")
}

/// The thumbnail file that exists, checked on the file system, for a view
/// that resolves one item at a time. The grid does not check: its cache
/// tries the `.png` when the `.jpg` cannot be read.
pub fn existing(jpeg: PathBuf) -> PathBuf {
    present(&jpeg).unwrap_or(jpeg)
}

/// The thumbnail file there is for this `.jpg` path, if any: the `.jpg`,
/// else the `.png`.
pub fn present(jpeg: &Path) -> Option<PathBuf> {
    [jpeg.to_path_buf(), png_for(jpeg)]
        .into_iter()
        .find(|path| path.exists())
}

/// Where `regenerateThumbnail` keeps a thumbnail aside while it is made
/// again: beside it, named for this process and the moment.
pub fn backup_for(thumbnail: &Path, process: u32, stamp: u128) -> PathBuf {
    let mut name = thumbnail.as_os_str().to_owned();
    name.push(format!(".{process}.{stamp}.regenerate-backup"));
    PathBuf::from(name)
}

/// `scheduleThumbnailAfterMiss`: asks the application to render a file's
/// thumbnail (`media.render_thumbnail_now`), as the media protocol does on a
/// miss, and draws one it cannot make (Flash, PDF and the other paged
/// documents) as the Electron main process does
/// (`renderExternalThumbnail`), off the main thread and only into a marked
/// disposable copy, as every write here. Resolves to whether there is a
/// thumbnail now.
async fn make(
    session: &gpui::Entity<crate::session::Session>,
    hash: &str,
    mime: &str,
    jpeg: &Path,
    cx: &mut AsyncApp,
) -> bool {
    let (answer, original, disposable) = session.update(cx, |session, _| {
        (
            session.dispatch(
                "media.render_thumbnail_now",
                serde_json::json!({ "file_hash": hash }),
            ),
            session.original(hash, mime),
            session.disposable(),
        )
    });
    let _ = answer.await;
    let (hash, mime, jpeg) = (hash.to_owned(), mime.to_owned(), jpeg.to_path_buf());
    cx.background_executor()
        .spawn(async move {
            if present(&jpeg).is_none() && disposable && crate::grid::rendered::draws(&mime) {
                let png = png_for(&jpeg);
                if let Err(error) = crate::grid::rendered::render(&mime, &original, &png) {
                    eprintln!("picto: could not draw the thumbnail of {hash}: {error:#}");
                }
            }
            present(&jpeg).is_some()
        })
        .await
}

/// `media.regenerate_thumbnails` as the Electron main process performs it
/// (`regenerateThumbnail`), one file after another: the thumbnail is kept
/// aside, made again by the application (or drawn here for Flash and paged
/// documents), and the old one restored if no new one could be made. Every
/// view showing it reads it again. The first failure stops the rest and is
/// shown as production's menu shows it. Only a marked disposable copy is
/// written to. `files` are content hashes and types.
pub fn regenerate(
    session: &gpui::Entity<crate::session::Session>,
    files: Vec<(String, String)>,
    cx: &mut App,
) {
    if files.is_empty() || !session.read(cx).disposable() {
        return;
    }
    let files: Vec<_> = files
        .into_iter()
        .map(|(hash, mime)| {
            let jpeg = session.read(cx).thumbnail(&hash);
            (hash, mime, jpeg)
        })
        .collect();
    let session = session.clone();
    cx.spawn(async move |cx| {
        for (hash, mime, jpeg) in files {
            let made = regenerate_one(&session, &hash, &mime, &jpeg, cx).await;
            session.update(cx, |session, cx| session.thumbnail_written(hash, cx));
            if let Err(error) = made {
                cx.update(|cx| crate::ui::notify::error(t("Menu action failed"), error, cx));
                break;
            }
        }
    })
    .detach();
}

/// One file of [`regenerate`].
async fn regenerate_one(
    session: &gpui::Entity<crate::session::Session>,
    hash: &str,
    mime: &str,
    jpeg: &Path,
    cx: &mut gpui::AsyncApp,
) -> Result<(), String> {
    let previous = present(jpeg);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let backup = previous
        .as_ref()
        .map(|previous| backup_for(previous, std::process::id(), stamp));
    if let (Some(previous), Some(backup)) = (&previous, &backup) {
        std::fs::rename(previous, backup).map_err(|error| error.to_string())?;
    }
    if make(session, hash, mime, jpeg, cx).await {
        if let Some(backup) = backup {
            let _ = std::fs::remove_file(backup);
        }
        Ok(())
    } else {
        if let (Some(previous), Some(backup)) = (previous, backup) {
            let _ = std::fs::rename(backup, previous);
        }
        Err("No thumbnail renderer supports this file.".into())
    }
}

/// The bytes a decoded picture takes, all its frames.
fn decoded_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .map(|frame| {
            let size = image.size(frame);
            size.width.0.max(0) as usize * size.height.0.max(0) as usize * 4
        })
        .sum()
}

/// The `.png` a `.jpg` thumbnail resource falls back to.
fn fallback(resource: &Resource) -> Option<Resource> {
    match resource {
        Resource::Path(path) if path.extension().is_some_and(|ext| ext == "jpg") => {
            Some(Resource::Path(png_for(path).into()))
        }
        _ => None,
    }
}

/// The `.jpg` a `.png` thumbnail stands in for.
fn primary(resource: &Resource) -> Option<Resource> {
    match resource {
        Resource::Path(path) if path.extension().is_some_and(|ext| ext == "png") => {
            Some(Resource::Path(path.with_extension("jpg").into()))
        }
        _ => None,
    }
}

impl Thumbnails {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|cache, cx| {
            for (_, entry) in std::mem::take(&mut cache.entries) {
                if let Some(Ok(image)) = entry.item.get() {
                    crate::atlas::release(image, cx);
                }
            }
        })
        .detach();
        Self {
            entries: HashMap::new(),
            clock: 0,
            pins: HashMap::new(),
            planner: None,
            wanted: HashSet::new(),
            renders: Renders::default(),
        }
    }

    /// Where a tile's thumbnail is: decoded (it, or the `.png` it falls
    /// back to), failed (both), or on its way. A font draws its card and
    /// has nothing to load.
    pub fn thumb(&self, resource: &Resource, mime: &str) -> Thumb {
        if mime.starts_with("font/") {
            return Thumb::Loaded;
        }
        let png = fallback(resource).map(|png| self.state(&png));
        match (self.state(resource), png) {
            (Some(true), _) | (Some(false), Some(Some(true))) => Thumb::Loaded,
            (Some(false), None | Some(Some(false))) => Thumb::Failed,
            _ => Thumb::Pending,
        }
    }

    /// Has the thumbnails of the tiles on screen that could not be read
    /// made (`scheduleThumbnailAfterMiss`), a couple at a time, each file
    /// once; a tile that left the screen before its turn is not made. A
    /// thumbnail that was made is read again, and the views that observe
    /// this cache are told. One whose file is there but did not decode
    /// stays broken.
    pub fn make_missing(
        &mut self,
        session: &gpui::Entity<crate::session::Session>,
        missing: Vec<Missing>,
        cx: &mut Context<Self>,
    ) {
        for Missing { hash, mime, jpeg } in self.renders.admit(missing) {
            let session = session.clone();
            let key = hash.clone();
            let task = cx.spawn(async move |this, cx| {
                let exists = {
                    let jpeg = jpeg.clone();
                    cx.background_executor()
                        .spawn(async move { present(&jpeg).is_some() })
                        .await
                };
                let made = !exists && make(&session, &hash, &mime, &jpeg, cx).await;
                let _ = this.update(cx, |this, cx| {
                    this.renders.running.remove(&hash);
                    if made {
                        this.forget(&jpeg, cx);
                    }
                    cx.notify();
                });
            });
            self.renders.running.insert(key, task);
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

    fn state(&self, resource: &Resource) -> Option<bool> {
        match self.entries.get(resource)?.item.get()? {
            Ok(_) => Some(true),
            Err(_) => Some(false),
        }
    }

    /// Whether this thumbnail was tried and could not be read or decoded,
    /// nor its `.png`.
    pub fn is_failed(&self, resource: &Resource) -> bool {
        self.thumb(resource, "") == Thumb::Failed
    }

    fn held(&self, resource: &Resource) -> bool {
        let is = |wanted: &Resource| {
            self.wanted.contains(wanted) || self.pins.values().any(|pinned| pinned == wanted)
        };
        is(resource) || primary(resource).is_some_and(|jpg| is(&jpg))
    }

    /// The decoded thumbnail, once it is ready.
    pub fn image(&self, resource: &Resource) -> Option<Arc<RenderImage>> {
        self.entries.get(resource)?.item.get()?.ok()
    }

    /// The decoded size of a thumbnail, in its pixels, once it is ready.
    pub fn size(&self, resource: &Resource) -> Option<(f32, f32)> {
        match self.entries.get(resource)?.item.get()? {
            Ok(image) => {
                let size = image.size(0);
                Some((size.width.0 as f32, size.height.0 as f32))
            }
            Err(_) => None,
        }
    }

    /// Lets go of a thumbnail (its `.jpg` and its `.png`) that was written
    /// again or for the first time (`picto:thumbnail-changed`), so the next
    /// frame that wants it reads the new file, as the Electron grid's
    /// pipeline invalidates it.
    pub fn forget(&mut self, jpeg: &Path, cx: &mut App) {
        for resource in [
            Resource::Path(jpeg.into()),
            Resource::Path(png_for(jpeg).into()),
        ] {
            if let Some(entry) = self.entries.remove(&resource)
                && let Some(Ok(image)) = entry.item.get()
            {
                crate::atlas::release(image, cx);
            }
        }
    }

    /// A tick of the clock that orders uses.
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn start(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) {
        crate::stats::thumbnail_read();
        let item = ImageCacheItem::new(resource, cx);
        // The view that asked is told when it is ready.
        item.use_image(window);
        let used = self.tick();
        self.entries.insert(
            resource.clone(),
            Entry {
                item,
                used,
                drawn: false,
            },
        );
    }

    /// The grid's thumbnails for this frame, most wanted first. A load that
    /// is no longer wanted is abandoned, the most wanted that are missing
    /// start, and decoded thumbnails beyond the window's [`budget`] are let
    /// go, oldest first, never one that is wanted or pinned.
    pub fn plan(&mut self, wanted: Vec<Resource>, window: &mut Window, cx: &mut App) {
        self.wanted = wanted.iter().cloned().collect();
        self.planner = Some(window.current_view());
        // Dropping an unfinished entry cancels its read and decode.
        let abandoned = self
            .entries
            .iter()
            .filter(|(resource, entry)| entry.item.get().is_none() && !self.held(resource))
            .map(|(resource, _)| resource.clone())
            .collect::<Vec<_>>();
        for resource in &abandoned {
            self.entries.remove(resource);
            crate::stats::thumbnail_abandoned();
        }
        let mut pending = self
            .entries
            .values()
            .filter(|entry| entry.item.get().is_none())
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
        let cost = |entry: &Entry| {
            entry
                .item
                .get()
                .and_then(Result::ok)
                .map_or(0, |image| decoded_bytes(&image))
                .max(ENTRY_BYTES)
        };
        let mut held_bytes = self.entries.values().map(cost).sum::<usize>();
        let viewport = window.viewport_size();
        let budget = budget((f32::from(viewport.width), f32::from(viewport.height)));
        if held_bytes > budget {
            let mut oldest_first = self
                .entries
                .iter()
                .map(|(resource, entry)| (entry.used, resource.clone()))
                .collect::<Vec<_>>();
            oldest_first.sort_unstable_by_key(|(used, _)| *used);
            for (_, resource) in oldest_first {
                if held_bytes <= budget {
                    break;
                }
                if self.held(&resource) {
                    continue;
                }
                if let Some(entry) = self.entries.remove(&resource) {
                    held_bytes -= cost(&entry);
                    if let Some(Ok(image)) = entry.item.get() {
                        crate::atlas::release(image, cx);
                        crate::stats::thumbnail_evicted(entry.drawn);
                    }
                }
            }
        }
        crate::stats::thumbnail_bytes(held_bytes);
    }
}

impl ImageCache for Thumbnails {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let used = self.tick();
        if let Some(entry) = self.entries.get_mut(resource) {
            entry.used = used;
            let image = entry.item.use_image(window);
            entry.drawn |= matches!(image, Some(Ok(_)));
            if let Some(Err(_)) = &image
                && let Some(png) = fallback(resource)
            {
                return self.load(&png, window, cx);
            }
            return image;
        }
        // A grid thumbnail waits for its turn in the plan. Anything else,
        // such as the inspector's preview, loads as soon as it is asked for,
        // even when the grid wants it too: only the view that starts a load
        // is told when it is ready, so another view left waiting on the
        // grid's turn would show its placeholder until something else
        // repainted it.
        if !self.wanted.contains(resource) || self.planner != Some(window.current_view()) {
            self.start(resource, window, cx);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Thumbnails, budget, fallback, load_order, primary};
    use gpui::prelude::FluentBuilder as _;
    use gpui::{
        AnyWindowHandle, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
        RenderImage, Resource, Styled as _, TestAppContext, Window, div, img, px, size,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    /// A band of the grid: its thumbnails, planned and drawn as the grid
    /// does.
    struct Band {
        thumbnails: Entity<Thumbnails>,
        shown: Vec<PathBuf>,
    }

    impl Render for Band {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let wanted = self
                .shown
                .iter()
                .map(|path| Resource::Path(path.clone().into()))
                .collect();
            self.thumbnails
                .update(cx, |thumbnails, cx| thumbnails.plan(wanted, window, cx));
            div().size_full().children(self.shown.iter().map(|path| {
                img(path.clone())
                    .image_cache(&self.thumbnails)
                    .w(px(20.))
                    .h(px(15.))
            }))
        }
    }

    /// Scrolling through several thousand tiles, a band at a time: every
    /// thumbnail is decoded and drawn, and the sprite atlas ends holding no
    /// more than the window's budget (and the band on screen), not every
    /// thumbnail ever shown.
    #[test]
    fn scrolling_through_thousands_of_tiles_keeps_the_atlas_bounded() {
        const TILES: usize = 3000;
        const BAND: usize = 40;
        let folder = tempfile::tempdir().unwrap();
        let first = folder.path().join("0.jpg");
        image::RgbImage::from_pixel(96, 72, image::Rgb([200, 90, 20]))
            .save(&first)
            .unwrap();
        let paths = (0..TILES)
            .map(|index| {
                let path = folder.path().join(format!("{index}.jpg"));
                if index > 0 {
                    std::fs::copy(&first, &path).unwrap();
                }
                path
            })
            .collect::<Vec<_>>();
        let mut cx = TestAppContext::single();
        let window = cx.open_window(size(px(300.), px(300.)), |_, cx| Band {
            thumbnails: cx.new(Thumbnails::new),
            shown: Vec::new(),
        });
        let handle: AnyWindowHandle = window.into();
        let draw = |cx: &mut TestAppContext| {
            for _ in 0..2 {
                cx.update_window(handle, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                })
                .unwrap();
                cx.run_until_parked();
            }
        };
        let mut decoded: Vec<Arc<RenderImage>> = Vec::new();
        for band in paths.chunks(BAND) {
            window
                .update(&mut cx, |view, _, _| view.shown = band.to_vec())
                .unwrap();
            // Six load at a time: the band is drawn until all have come.
            for _ in 0..BAND {
                draw(&mut cx);
                let loaded = window
                    .read_with(&cx, |view, cx| {
                        let thumbnails = view.thumbnails.read(cx);
                        band.iter()
                            .map(|path| thumbnails.image(&Resource::Path(path.clone().into())))
                            .collect::<Option<Vec<_>>>()
                    })
                    .unwrap();
                if let Some(images) = loaded {
                    decoded.extend(images);
                    break;
                }
            }
        }
        assert_eq!(decoded.len(), TILES);
        let held = cx
            .update_window(handle, |_, window, _| {
                decoded
                    .iter()
                    .filter(|image| window.has_image_atlas_entry(image))
                    .count()
            })
            .unwrap();
        let each = 96 * 72 * 4;
        assert!(
            held > BAND && held * each <= budget((300., 300.)) + BAND * each,
            "{held} in the atlas"
        );
    }

    /// The grid and, in a view of its own, the first of its covers that
    /// waits for its turn in the grid's plan (as the inspector's collage
    /// shows the first covers of a destination just opened).
    struct Cover {
        thumbnails: Entity<Thumbnails>,
        shown: PathBuf,
        renders: usize,
    }

    impl Render for Cover {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            img(self.shown.clone())
                .image_cache(&self.thumbnails)
                .w(px(20.))
                .h(px(15.))
        }
    }

    struct Shell {
        band: Entity<Band>,
        cover: Entity<Cover>,
        /// The cover is shown once the grid has planned.
        cover_shown: bool,
    }

    impl Render for Shell {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            // Cached, as the shell holds its grid and inspector: a view
            // draws again only when it is told to.
            let fill = || gpui::StyleRefinement::default().size_full();
            div()
                .size_full()
                .child(self.band.clone().cached(fill()))
                .when(self.cover_shown, |shell| {
                    shell.child(self.cover.clone().cached(fill()))
                })
        }
    }

    /// A view outside the grid asking for a thumbnail the grid wants but
    /// has not started yet has it loaded and is told when it is ready: a
    /// real window draws only when told, so otherwise it would show its
    /// placeholder until something else repainted it.
    #[test]
    fn another_view_is_not_left_waiting_on_the_grids_turn() {
        let folder = tempfile::tempdir().unwrap();
        let paths = (0..super::CONCURRENCY + 1)
            .map(|index| {
                let path = folder.path().join(format!("{index}.jpg"));
                image::RgbImage::from_pixel(8, 6, image::Rgb([200, 90, 20]))
                    .save(&path)
                    .unwrap();
                path
            })
            .collect::<Vec<_>>();
        let last = paths.last().unwrap().clone();
        let mut cx = TestAppContext::single();
        let window = cx.open_window(size(px(300.), px(300.)), |_, cx| {
            let thumbnails = cx.new(Thumbnails::new);
            let band = cx.new(|_| Band {
                thumbnails: thumbnails.clone(),
                shown: paths.clone(),
            });
            let cover = cx.new(|_| Cover {
                thumbnails,
                shown: last.clone(),
                renders: 0,
            });
            Shell {
                band,
                cover,
                cover_shown: false,
            }
        });
        let handle: AnyWindowHandle = window.into();
        let draw_if_told = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| {
                if window.is_dirty() {
                    window.draw(cx).clear(cx);
                }
            })
            .unwrap();
        };
        // The grid draws and plans first; the cover then asks for the
        // thumbnail the grid has not started.
        window
            .update(&mut cx, |shell, _, cx| {
                shell.cover_shown = true;
                cx.notify();
            })
            .unwrap();
        draw_if_told(&mut cx);
        // Every load finishes and the window draws each frame it is told to,
        // as a real one does.
        for _ in 0..4 {
            cx.run_until_parked();
            draw_if_told(&mut cx);
        }
        let (loaded, renders) = window
            .read_with(&cx, |shell, cx| {
                let cover = shell.cover.read(cx);
                let loaded = cover
                    .thumbnails
                    .read(cx)
                    .image(&Resource::Path(last.clone().into()))
                    .is_some();
                (loaded, cover.renders)
            })
            .unwrap();
        assert!(loaded, "the cover was not loaded");
        assert!(renders > 1, "the cover was never drawn again once loaded");
    }

    #[test]
    fn missing_thumbnails_are_made_two_at_a_time_while_their_tiles_show() {
        let file = |hash: &str| super::Missing {
            hash: hash.into(),
            mime: "application/pdf".into(),
            jpeg: PathBuf::from(format!("/l/blobs/t/{hash}.jpg")),
        };
        let hashes = |files: &[super::Missing]| {
            files
                .iter()
                .map(|file| file.hash.clone())
                .collect::<Vec<_>>()
        };
        let mut renders = super::Renders::default();
        let started = renders.admit(["a", "b", "c", "d"].map(file).into());
        assert_eq!(hashes(&started), ["a", "b"]);
        for file in started {
            renders.running.insert(file.hash, gpui::Task::ready(()));
        }
        // Scrolled on: c left the screen before its turn; e came.
        assert!(renders.admit(["b", "d", "e"].map(file).into()).is_empty());
        assert_eq!(hashes(&renders.waiting), ["d", "e"]);
        // One finished: the next in view starts; a finished file is not
        // made again.
        renders.running.remove("a");
        let started = renders.admit(["a", "d", "e"].map(file).into());
        assert_eq!(hashes(&started), ["d"]);
        assert_eq!(hashes(&renders.waiting), ["e"]);
    }

    #[test]
    fn the_budget_keeps_three_windows_of_the_smallest_tiles() {
        // 1213 × 865: 9 × 6 tiles of 150 a window, 3 windows of 786 kB.
        assert_eq!(budget((1213., 865.)), 3 * 54 * 786_432);
        // Never less than one tile a window.
        assert_eq!(budget((0., 0.)), 3 * 786_432);
    }

    #[test]
    fn a_thumbnail_is_kept_aside_beside_itself() {
        assert_eq!(
            super::backup_for(Path::new("/l/blobs/t/ab/cd/abcd.jpg"), 42, 1700),
            Path::new("/l/blobs/t/ab/cd/abcd.jpg.42.1700.regenerate-backup")
        );
    }

    #[test]
    fn a_jpg_thumbnail_falls_back_to_png_and_back() {
        let jpg = Resource::Path(std::path::Path::new("/l/blobs/t/ab/cd/abcd.jpg").into());
        let png = Resource::Path(std::path::Path::new("/l/blobs/t/ab/cd/abcd.png").into());
        assert_eq!(fallback(&jpg), Some(png.clone()));
        assert_eq!(fallback(&png), None);
        assert_eq!(primary(&png), Some(jpg));
    }

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
