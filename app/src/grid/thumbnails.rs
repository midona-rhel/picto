//! The cache of decoded thumbnails that the grid and other views draw from. It
//! keeps the thumbnails from one frame to the next.
//!
//! On every frame the grid gives [`Thumbnails::plan`] the list of thumbnails
//! its tiles need, with the most important first. [`load_order`] gives that
//! order. The cache starts reading and decoding thumbnails from the front of
//! the list, at most six at a time (`CONCURRENCY`). It cancels a load that has
//! not finished when the grid no longer needs that thumbnail, which happens
//! when the user scrolls past it. A decoded thumbnail that the grid no longer
//! needs stays in the cache, so that scrolling back to it needs no new load.
//! When the decoded thumbnails take more memory than the budget allows
//! ([`budget`]), the ones used longest ago are removed.
use crate::i18n::{en, t};
use gpui::*;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct Thumbnails {
    /// Every thumbnail the cache holds, whether it is still loading, has
    /// loaded or has failed to load, keyed by its file. Each entry records
    /// when it was last used, as a value of `clock`, so that the least
    /// recently used entries can be removed first when the cache is over its
    /// budget. A use only writes that number into the entry. The cache keeps
    /// no list ordered by use, because moving an entry within such a list
    /// would mean searching for it by comparing paths, and comparing two paths
    /// compares them one component at a time.
    entries: HashMap<Resource, Entry>,
    clock: u64,
    /// The thumbnails that views outside the grid are showing, each keyed by a
    /// name for the view that shows it (for example `"inspector-preview"`). A
    /// pinned thumbnail is never removed from the cache. A view outside the
    /// grid is not drawn again when the grid scrolls. If its thumbnail were
    /// removed and the thumbnail's place in the sprite atlas were given to
    /// another picture, the view would go on drawing that other picture.
    pins: HashMap<&'static str, Resource>,
    /// The thumbnails given to the most recent call to [`Thumbnails::plan`]:
    /// the ones the calling view needs for the tiles in and near its viewport.
    wanted: HashSet<Resource>,
    /// The view that made the most recent call to [`Thumbnails::plan`]. That
    /// is the grid, or the viewer's collection editor, which uses the cache in
    /// the same way. When this view asks for a thumbnail that is in `wanted`
    /// and has not started loading, the load is not started at once. `plan`
    /// starts it when its turn comes.
    planner: Option<EntityId>,
    /// The state of the work that makes missing thumbnails
    /// ([`Thumbnails::make_missing`]).
    renders: Renders,
}

/// How far the loading of a tile's thumbnail has got.
#[derive(Clone, Copy, PartialEq)]
pub enum Thumb {
    Pending,
    Loaded,
    /// The thumbnail could not be read or decoded. The tile shows the picture
    /// of a torn page instead.
    Failed,
}

/// A file whose tile is on screen and whose thumbnail could not be read or
/// decoded. It is given to [`Thumbnails::make_missing`] to have the thumbnail
/// made. `hash` is the file's content hash and `mime` is its type.
pub struct Missing {
    pub hash: String,
    pub mime: String,
    /// The path of the `.jpg` thumbnail the tile tries to read.
    pub jpeg: PathBuf,
}

/// The work of making missing thumbnails. At most two are made at the same
/// time (`RENDERS`), and only for tiles that are still on screen.
#[derive(Default)]
struct Renders {
    /// The files whose tiles are on screen, whose thumbnail is missing, and
    /// whose thumbnail has not started being made, in the order they will be
    /// started.
    waiting: Vec<Missing>,
    /// The thumbnails being made now: the task that makes each one, keyed by
    /// the file's content hash.
    running: HashMap<String, Task<()>>,
    /// The content hashes of the files whose thumbnail has already started
    /// being made. Each file is tried only once. A file whose thumbnail could
    /// not be made keeps showing the picture of a torn page and is not tried
    /// again.
    tried: HashSet<String>,
}

struct Entry {
    item: ImageCacheItem,
    /// The value `Thumbnails::clock` had when this thumbnail was last used.
    used: u64,
    /// Whether the decoded picture was given to a view to draw at least once.
    /// The statistics use it to count the thumbnails that were removed from
    /// the cache without ever having been drawn (`stats::thumbnail_evicted`).
    drawn: bool,
}

impl Renders {
    /// Takes `missing`, the files whose tiles are on screen now and whose
    /// thumbnail could not be read, and returns the files whose thumbnails are
    /// to start being made now.
    ///
    /// The waiting list is replaced by the files of `missing` that have not
    /// been tried and are not being made, in the order given. A file whose
    /// tile left the screen before its turn is therefore no longer waiting.
    /// Files are then taken from the front of the waiting list until two are
    /// being made (`RENDERS`). They are marked as tried and returned. The
    /// caller starts them and adds them to `running`.
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

/// The number of bytes one decoded thumbnail is assumed to take when the
/// budget is computed: 512 × 384 pixels at four bytes a pixel, which is
/// 786,432 bytes. That is a 4:3 picture whose long edge is the 512 pixels
/// thumbnails are stored at (`THUMBNAIL_LONG_EDGE`).
const TYPICAL_BYTES: usize = (crate::grid::originals::THUMBNAIL_LONG_EDGE
    * crate::grid::originals::THUMBNAIL_LONG_EDGE
    * 0.75) as usize
    * 4;
/// The smallest number of bytes an entry is counted as. An entry whose
/// thumbnail failed to load, or is still loading, holds no decoded picture.
/// Counting it as 4096 bytes makes such entries add to the total too, so that
/// they are also removed when the cache is over its budget and do not
/// accumulate without limit.
const ENTRY_BYTES: usize = 4096;

/// Returns the number of bytes of decoded thumbnails the cache may hold,
/// counting the thumbnails the grid needs now. `window` is the window's width
/// and height in pixels. The budget is the memory of three windows full of
/// tiles at the smallest tile size (150px, `SMALLEST_TILE`), with each
/// thumbnail counted as `TYPICAL_BYTES`: one windowful for the tiles on
/// screen, one for the tiles above them and one for the tiles below them.
pub fn budget(window: (f32, f32)) -> usize {
    let across = (window.0 / crate::grid::layout::SMALLEST_TILE)
        .ceil()
        .max(1.);
    let down = (window.1 / crate::grid::layout::SMALLEST_TILE)
        .ceil()
        .max(1.);
    3 * (across * down) as usize * TYPICAL_BYTES
}

/// The number of thumbnails that are read and decoded at the same time.
const CONCURRENCY: usize = 6;
/// The number of missing thumbnails that are made at the same time. It is only
/// two because making one is slow: a Flash movie is played for five seconds
/// before its picture is taken, and a paged document has to be opened and its
/// first page drawn.
const RENDERS: usize = 2;

/// Returns the order in which to load the thumbnails of `tiles`. Tiles that
/// are at least partly inside the viewport come before tiles that are outside
/// it, in the margin above or below. Within each of the two groups, the tile
/// whose centre is nearest to the vertical middle of the viewport comes first.
/// A tile that only touches the edge of the viewport counts as outside it.
///
/// Each entry of `tiles` is a tile's top and its height. `view_top` is the top
/// of the viewport in the same coordinates, and `view_height` is its height.
/// The result is a list of indexes into `tiles`.
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

/// Returns the `.png` path to try when the `.jpg` thumbnail at `jpeg` cannot
/// be read: the same path with the extension `.png`. The library looks for a
/// thumbnail in the same order (`find_thumbnail_path` in the `core` crate). A
/// thumbnail is normally the `.jpg` the library writes. When there is no
/// `.jpg`, it is the `.png` that is written for audio waveforms, for captured
/// frames, and by older libraries.
pub fn png_for(jpeg: &Path) -> PathBuf {
    jpeg.with_extension("png")
}

/// Returns the thumbnail file that exists on disk for the `.jpg` path `jpeg`:
/// the `.jpg` if it exists, otherwise the `.png` if that exists, otherwise
/// `jpeg` unchanged. It checks the file system, so it is meant for a view that
/// shows one item at a time. The grid does not call it for its tiles. The
/// cache tries the `.png` itself when the `.jpg` cannot be read.
pub fn existing(jpeg: PathBuf) -> PathBuf {
    present(&jpeg).unwrap_or(jpeg)
}

/// Returns the thumbnail file that exists on disk for the `.jpg` path `jpeg`:
/// the `.jpg` if it exists, otherwise the `.png` if that exists, otherwise
/// `None`.
pub fn present(jpeg: &Path) -> Option<PathBuf> {
    [jpeg.to_path_buf(), png_for(jpeg)]
        .into_iter()
        .find(|path| path.exists())
}

/// Returns the path a thumbnail file is renamed to while the thumbnail is
/// being made again. It is the thumbnail's own path with
/// `.<process>.<stamp>.regenerate-backup` added to the end, so the backup is
/// in the same folder. The caller passes this process's id as `process` and
/// the current time in milliseconds as `stamp`, which keeps the name
/// different from the name of any other backup.
pub fn backup_for(thumbnail: &Path, process: u32, stamp: u128) -> PathBuf {
    let mut name = thumbnail.as_os_str().to_owned();
    name.push(format!(".{process}.{stamp}.regenerate-backup"));
    PathBuf::from(name)
}

/// Makes the thumbnail of one file. Returns whether a thumbnail file exists
/// afterwards.
///
/// First it sends the application the command `media.render_thumbnail_now` for
/// the file and waits for the answer. If there is still no thumbnail file, and
/// the file is of a type the application cannot make a thumbnail of but this
/// app draws itself (Flash, PDF and the other paged documents, see
/// `rendered::draws`), it draws the thumbnail to the `.png` beside `jpeg`. The
/// drawing runs on a background thread.
///
/// The thumbnail is drawn only when the session's library is the one open in
/// the window (`libraries::writable`), and only to a path inside that
/// library's folder (`libraries::confine`). Every file write in this module is
/// limited in the same two ways.
async fn make(
    session: &gpui::Entity<crate::session::Session>,
    hash: &str,
    mime: &str,
    jpeg: &Path,
    cx: &mut AsyncApp,
) -> bool {
    let (answer, original, writable) = session.update(cx, |session, cx| {
        (
            session.dispatch(
                "media.render_thumbnail_now",
                serde_json::json!({ "file_hash": hash }),
            ),
            session.original(hash, mime),
            session.writable_root(cx),
        )
    });
    let _ = answer.await;
    let (hash, mime, jpeg) = (hash.to_owned(), mime.to_owned(), jpeg.to_path_buf());
    cx.background_executor()
        .spawn(async move {
            if present(&jpeg).is_none()
                && let Some(root) = writable
                && crate::grid::rendered::draws(&mime)
            {
                let drawn = rendered_target(&root, &jpeg)
                    .map_err(anyhow::Error::msg)
                    .and_then(|png| crate::grid::rendered::render(&mime, &original, &png));
                if let Err(error) = drawn {
                    eprintln!("picto: could not draw the thumbnail of {hash}: {error:#}");
                }
            }
            present(&jpeg).is_some()
        })
        .await
}

/// Returns the path this app writes a thumbnail to when it draws the thumbnail
/// itself: the `.png` with the same name and in the same folder as the
/// library's `.jpg` thumbnail path `jpeg`. Returns an error message when that
/// path is not inside the library at `root`.
pub fn rendered_target(root: &Path, jpeg: &Path) -> Result<PathBuf, String> {
    crate::libraries::confine(root, &png_for(jpeg))
}

/// Renames the thumbnail file `previous` to `backup`, which moves it out of
/// the way while the thumbnail is made again. `regenerate_one` also calls it
/// with the two paths swapped, to put the old thumbnail back. Both paths must
/// be inside the library at `root`. Returns an error message when either is
/// not, or when the rename fails.
pub fn set_aside(root: &Path, previous: &Path, backup: &Path) -> Result<(), String> {
    let previous = crate::libraries::confine(root, previous)?;
    let backup = crate::libraries::confine(root, backup)?;
    std::fs::rename(previous, backup).map_err(|error| error.to_string())
}

/// Makes the thumbnails of `files` again, one file after another. The menu
/// command "Regenerate Thumbnail" calls it. Each entry of `files` is a file's
/// content hash and its type.
///
/// For each file, the existing thumbnail is renamed to a backup name. A new
/// thumbnail is then made by the application, or drawn by this app for Flash
/// and paged documents ([`make`]), and the backup is deleted. If no new
/// thumbnail could be made, the backup is renamed back. After each file the
/// session emits `Event::Thumbnail` for it, so every view that shows the
/// thumbnail reads it again.
///
/// The first file that fails stops the files after it. Its error is shown in
/// an error notification titled "Menu action failed". Files are written only
/// when the session's library is the one open in the window, and only inside
/// that library's folder. When that library is not the one open, nothing is
/// done and the notification says "No library is open."
pub fn regenerate(
    session: &gpui::Entity<crate::session::Session>,
    files: Vec<(String, String)>,
    cx: &mut App,
) {
    if files.is_empty() {
        return;
    }
    let Some(root) = session.read(cx).writable_root(cx) else {
        crate::ui::notify::error(t("Menu action failed"), en("No library is open."), cx);
        return;
    };
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
            let made = regenerate_one(&session, &root, &hash, &mime, &jpeg, cx).await;
            session.update(cx, |session, cx| session.thumbnail_written(hash, cx));
            if let Err(error) = made {
                cx.update(|cx| crate::ui::notify::error(t("Menu action failed"), error, cx));
                break;
            }
        }
    })
    .detach();
}

/// Makes the thumbnail of one file again, for [`regenerate`]. It renames the
/// existing thumbnail file to a backup name, makes a new thumbnail with
/// [`make`], and deletes the backup. If no thumbnail could be made, it renames
/// the backup back and returns the error message
/// "No thumbnail renderer supports this file."
async fn regenerate_one(
    session: &gpui::Entity<crate::session::Session>,
    root: &Path,
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
        set_aside(root, previous, backup)?;
    }
    if make(session, hash, mime, jpeg, cx).await {
        if let Some(backup) = backup {
            let _ = std::fs::remove_file(backup);
        }
        Ok(())
    } else {
        if let (Some(previous), Some(backup)) = (previous, backup) {
            let _ = set_aside(root, &backup, &previous);
        }
        Err("No thumbnail renderer supports this file.".into())
    }
}

/// Returns the number of bytes a decoded picture takes in memory: its width
/// times its height times four bytes, added up over all of its frames. An
/// animated picture has more than one frame.
fn decoded_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .map(|frame| {
            let size = image.size(frame);
            size.width.0.max(0) as usize * size.height.0.max(0) as usize * 4
        })
        .sum()
}

/// Returns the `.png` resource to load when the `.jpg` thumbnail `resource`
/// cannot be read or decoded. Returns `None` when `resource` is not a path
/// that ends in `.jpg`.
fn fallback(resource: &Resource) -> Option<Resource> {
    match resource {
        Resource::Path(path) if path.extension().is_some_and(|ext| ext == "jpg") => {
            Some(Resource::Path(png_for(path).into()))
        }
        _ => None,
    }
}

/// Returns the `.jpg` resource that the `.png` thumbnail `resource` is loaded
/// in place of. Returns `None` when `resource` is not a path that ends in
/// `.png`.
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

    /// Returns how far the loading of the thumbnail `resource` has got, for a
    /// file of type `mime`. It is `Loaded` when the `.jpg` is decoded, and
    /// when the `.jpg` failed and the `.png` tried in its place is decoded. It
    /// is `Failed` when the `.jpg` failed and the `.png` failed too, or there
    /// is no `.png` to try. Otherwise it is `Pending`: a load has not started
    /// or has not finished.
    ///
    /// A font file always gives `Loaded`. A font has no thumbnail: its tile
    /// draws the picture that stands for all fonts and loads nothing.
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

    /// Makes the thumbnails that could not be read, for the tiles on screen.
    /// `missing` lists the files of those tiles, and the caller passes the
    /// current list on every frame. At most two thumbnails are made at the
    /// same time (`RENDERS`), each file is tried only once, and a file whose
    /// tile leaves the screen before its turn is not made.
    ///
    /// When a thumbnail has been made, its failed entry is removed from the
    /// cache, so that the next frame reads the new file. When the attempt for
    /// a file ends, whether a thumbnail was made or not, the views that
    /// observe this cache are notified and draw again. If a thumbnail file
    /// already exists on disk, nothing is made: the file is there but could
    /// not be decoded, and the tile keeps showing the picture of a torn page.
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

    /// Pins the thumbnail `resource` for the view named `holder`, so that the
    /// thumbnail is not removed from the cache while that view shows it. Each
    /// holder has one pinned thumbnail at a time: pinning another one replaces
    /// the earlier one, and `None` removes the pin.
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

    /// Returns whether loading this thumbnail failed: the `.jpg` could not be
    /// read or decoded, and neither could the `.png` tried in its place.
    pub fn is_failed(&self, resource: &Resource) -> bool {
        self.thumb(resource, "") == Thumb::Failed
    }

    fn held(&self, resource: &Resource) -> bool {
        let is = |wanted: &Resource| {
            self.wanted.contains(wanted) || self.pins.values().any(|pinned| pinned == wanted)
        };
        is(resource) || primary(resource).is_some_and(|jpg| is(&jpg))
    }

    /// Returns the decoded picture of this thumbnail. Returns `None` when the
    /// thumbnail is not in the cache, is still loading, or failed to load.
    pub fn image(&self, resource: &Resource) -> Option<Arc<RenderImage>> {
        self.entries.get(resource)?.item.get()?.ok()
    }

    /// Returns the width and height of the decoded thumbnail, in the
    /// thumbnail's own pixels. Returns `None` when the thumbnail is not in the
    /// cache, is still loading, or failed to load.
    pub fn size(&self, resource: &Resource) -> Option<(f32, f32)> {
        match self.entries.get(resource)?.item.get()? {
            Ok(image) => {
                let size = image.size(0);
                Some((size.width.0 as f32, size.height.0 as f32))
            }
            Err(_) => None,
        }
    }

    /// Removes a thumbnail from the cache, both its `.jpg` entry and its
    /// `.png` entry, and releases their decoded pictures from the sprite
    /// atlas. It is called when the thumbnail file has been written again or
    /// written for the first time, so that the next frame that needs the
    /// thumbnail reads the new file.
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

    /// Adds one to `clock` and returns the new value. Every use of an entry is
    /// recorded as such a value, so a larger value means a more recent use.
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn start(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) {
        crate::stats::thumbnail_read();
        let item = ImageCacheItem::new(resource, cx);
        // `use_image` subscribes the view that is being drawn now to this
        // load. GPUI notifies that view when the load finishes, and the view
        // then draws again with the thumbnail.
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

    /// Tells the cache which thumbnails the calling view needs for this frame.
    /// The caller is the grid, or the viewer's collection editor, which uses
    /// the cache in the same way. `wanted` lists the thumbnails in the order
    /// they are to load, with the most important first. The cache then does
    /// three things:
    ///
    /// 1. It removes every entry whose load has not finished and whose
    ///    thumbnail is neither in `wanted` nor pinned. Removing the entry
    ///    cancels the load.
    ///
    /// 2. It starts loading the thumbnails of `wanted` that have no entry yet,
    ///    in order, until six loads are in progress (`CONCURRENCY`).
    ///
    /// 3. If the entries together take more bytes than the window's
    ///    [`budget`], it removes entries, the least recently used first, until
    ///    they fit. It never removes a thumbnail that is in `wanted` or
    ///    pinned.
    ///
    /// A `.png` that is loaded in place of a `.jpg` counts as wanted or pinned
    /// when its `.jpg` is.
    pub fn plan(&mut self, wanted: Vec<Resource>, window: &mut Window, cx: &mut App) {
        self.wanted = wanted.iter().cloned().collect();
        self.planner = Some(window.current_view());
        // Remove the entries that are still loading and that are neither
        // wanted nor pinned. Dropping such an entry cancels its read and
        // decode.
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
        // There is no entry for this thumbnail, so no load of it has started.
        // When the view asking is the one that called `plan` (normally the
        // grid) and the thumbnail is in `wanted`, nothing is started here:
        // `plan` starts the load when its turn comes. In every other case the
        // load starts now. That includes a view outside the grid, such as the
        // inspector's preview, asking for a thumbnail that is also in
        // `wanted`. GPUI notifies a view that a load has finished only if the
        // view asked for the thumbnail when the load had already started. If
        // the other view waited here for `plan` to start the load, it would
        // not be notified, and it would show its placeholder until something
        // else made it draw again.
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

    /// A test view that stands in for the grid. Every time it is rendered it
    /// gives the thumbnails in `shown` to `Thumbnails::plan` and draws each of
    /// them as an image 20px wide and 15px high, which is how the grid uses
    /// the cache.
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

    /// Shows 3000 thumbnails, 40 at a time, as scrolling through a large grid
    /// does. Checks that all 3000 were decoded, and that at the end the
    /// window's sprite atlas holds more than the 40 on screen but no more
    /// bytes than the window's budget plus those 40. Without the budget it
    /// would hold every thumbnail that was ever shown.
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
            // Only six thumbnails load at a time, so the 40 thumbnails are
            // drawn again and again, at most 40 times, until all of them are
            // decoded.
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

    /// A test view that stands in for a view outside the grid, such as the
    /// inspector's collage, which shows the first covers of a destination that
    /// was just opened. It draws one thumbnail from the same cache as
    /// [`Band`]. In the test below, that thumbnail is one the grid has asked
    /// for but whose load `plan` has not started. `renders` counts how many
    /// times the view was rendered.
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
        /// Whether the `Cover` view is drawn. The test sets it after `Band`
        /// has been drawn once and has called `plan`.
        cover_shown: bool,
    }

    impl Render for Shell {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            // Both views are drawn through `cached`, which is how the app's
            // shell holds its grid and inspector. A cached view is rendered
            // again only when it has been notified. Otherwise GPUI reuses what
            // the view drew before.
            let fill = || gpui::StyleRefinement::default().size_full();
            div()
                .size_full()
                .child(self.band.clone().cached(fill()))
                .when(self.cover_shown, |shell| {
                    shell.child(self.cover.clone().cached(fill()))
                })
        }
    }

    /// Checks what happens when a view outside the grid asks for a thumbnail
    /// that is in the grid's `wanted` list but has not started loading. The
    /// load must start at once, and that view must be notified when the load
    /// finishes. A real window draws a view again only when the view has been
    /// notified, so without this the view would show its placeholder until
    /// something else made it draw again.
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
        // The window has been drawn once, so `Band` has called `plan`, which
        // started loading six of the seven thumbnails. Now show `Cover`, which
        // asks for the seventh.
        window
            .update(&mut cx, |shell, _, cx| {
                shell.cover_shown = true;
                cx.notify();
            })
            .unwrap();
        draw_if_told(&mut cx);
        // Let every load finish. After each round, draw the window only if a
        // view has been notified, which is what a real window does.
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
        // The grid has scrolled. The tile of file c left the screen before
        // its thumbnail was started, and the tile of file e came onto the
        // screen. Files a and b are still being made, so nothing new starts,
        // and files d and e wait.
        assert!(renders.admit(["b", "d", "e"].map(file).into()).is_empty());
        assert_eq!(hashes(&renders.waiting), ["d", "e"]);
        // File a has finished, so one more can start, and it is file d, the
        // first one waiting. The tile of file a is on screen again, but its
        // thumbnail is not made again, because each file is tried only once.
        renders.running.remove("a");
        let started = renders.admit(["a", "d", "e"].map(file).into());
        assert_eq!(hashes(&started), ["d"]);
        assert_eq!(hashes(&renders.waiting), ["e"]);
    }

    #[test]
    fn the_budget_keeps_three_windows_of_the_smallest_tiles() {
        // A window of 1213 × 865px holds 9 × 6 = 54 tiles of 150px. The budget
        // is three such windows of tiles, at 786,432 bytes a thumbnail.
        assert_eq!(budget((1213., 865.)), 3 * 54 * 786_432);
        // A window with no width and no height is counted as holding one tile.
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
        // The viewport reaches from 1000 to 1600 on the y axis, so its middle
        // is at 1300. Each tile is 200px high.
        let tiles = [
            (500., 200.),  // This tile is above the viewport.
            (1000., 200.), // This tile is at the top of the viewport.
            (1200., 200.), // This tile is in the middle of the viewport.
            (1400., 200.), // This tile is at the bottom of the viewport.
            (1700., 200.), // This tile is below the viewport.
        ];
        assert_eq!(load_order(&tiles, 1000., 600.), vec![2, 1, 3, 4, 0]);
    }

    #[test]
    fn a_tile_that_only_touches_the_viewport_edge_is_outside_it() {
        let tiles = [(800., 200.), (1600., 100.), (1500., 100.)];
        assert_eq!(load_order(&tiles, 1000., 600.), vec![2, 1, 0]);
    }
}
