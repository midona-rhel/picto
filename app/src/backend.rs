use crate::i18n::t;
use picto_library::RootId;
use picto_library::predicate::{FilterClause, FilterExpr, TextField};
use picto_library::query::{ItemScope, RootQuery, RootWindow, WindowRequest};
use picto_library::{Library, TagRecord};
use std::sync::{
    Arc,
    mpsc::{self, Receiver, Sender},
};

#[derive(Clone)]
pub struct Navigation {
    pub label: String,
    /// The number of items shown on the row. It is `None` for the rows that
    /// show no number (Random, Subscriptions and smart folders without
    /// rules).
    pub count: Option<u64>,
    pub scope: ItemScope,
    /// The screen this destination opens, when it opens a screen of its
    /// own instead of a grid of the items in `scope`.
    pub screen: Option<Screen>,
    /// Whether this is the Random row, which shows the items of the All
    /// scope in a random order.
    pub random: bool,
    /// The name of the row's icon. A system row has a fixed icon. A folder
    /// or smart folder has the icon the user chose for it in the icon
    /// picker (a name such as `IconPhoto`), or none.
    pub icon: Option<String>,
    /// The colour the user gave a folder or smart folder. Its icon is
    /// drawn in this colour.
    pub tint: Option<u32>,
    /// Whether this is a smart folder without rules. Such a smart folder
    /// only contains other smart folders: it shows no count and cannot be
    /// opened.
    pub group: bool,
    /// Whether this is a folder that watches a directory on disk for new
    /// files.
    pub watched: bool,
    pub depth: usize,
    pub parent: Option<usize>,
}
impl Navigation {
    /// Returns an id for this destination, such as `system:inbox`,
    /// `folder:5` or `smart:3`. The id stays the same when the list of
    /// destinations is rebuilt, which the destination's position in the list
    /// does not. Picto 0.6.19 and earlier named the destinations by the
    /// same ids, and ids are kept on disk (the sidebar's folded rows).
    pub fn id(&self) -> String {
        match (&self.scope, self.screen, self.random) {
            (_, Some(Screen::Tags), _) => "system:tag_manager".into(),
            (_, Some(Screen::Subscriptions), _) => "system:subscriptions".into(),
            (_, Some(Screen::Duplicates), _) => "system:duplicates".into(),
            (_, None, true) => "system:random".into(),
            (ItemScope::All, ..) => "system:active".into(),
            (ItemScope::Inbox, ..) => "system:inbox".into(),
            (ItemScope::RecentlyViewed, ..) => "system:recent_viewed".into(),
            (ItemScope::Uncategorized, ..) => "system:uncategorized".into(),
            (ItemScope::Untagged, ..) => "system:untagged".into(),
            (ItemScope::Trash, ..) => "system:trash".into(),
            (ItemScope::MediaMatches { item_id }, ..) => format!("media:{item_id}"),
            (ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id }, ..) => {
                format!("folder:{}", folder_id.0)
            }
            (ItemScope::SmartFolder { smart_folder_id }, ..) => {
                format!("smart:{}", smart_folder_id.0)
            }
        }
    }
}

/// The destinations that open a screen of their own instead of a grid of
/// items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Tags,
    Subscriptions,
    Duplicates,
}

/// A piece of work that runs on the library's thread. The screens use it
/// for their own reads and edits of the library.
pub type Job = Box<dyn FnOnce(&Library) + Send>;

pub enum Request {
    Job(Job),
    /// Asks for the results of a destination that is being opened. The
    /// fields are the query's generation, its scope, its filter, whether to
    /// sort the results randomly, and which window of the results to read.
    /// The sort comes from the view settings saved for the scope.
    Query(u64, ItemScope, FilterExpr, bool, WindowRequest),
    /// Asks for another window of results that are already shown, read
    /// with the same query as before. The fields are the query's
    /// generation, the query, the window to read, and the reason for the
    /// read.
    Window(u64, Box<RootQuery>, WindowRequest, Fetch),
    /// Says that the application finished opening the library (its
    /// recovery passes ran and its workers started), or that the open
    /// failed.
    Opened(Result<(), String>),
}

/// What the thread that opens the library reports to the library's thread,
/// which waits for it.
enum Opening {
    /// The database is open and can be read.
    Readable(Arc<Library>),
    /// The open failed before the database could be read.
    Failed(String),
}

/// The reason a window of results is read.
#[derive(Clone, Debug, PartialEq)]
pub enum Fetch {
    /// It is the first window of a destination that is being opened.
    Open,
    /// The grid scrolled to items outside the window it holds. The number
    /// counts these requests, so that only the result of the latest one is
    /// shown.
    Move(u64),
    /// The library changed, so the window that is shown is read again at
    /// the same position. The read also returns the current positions of
    /// the given items, which are the selected ones.
    Refresh(Vec<RootId>),
}

/// A window of results that the library's thread has read.
pub struct Arrived {
    pub generation: u64,
    pub fetch: Fetch,
    pub window: RootWindow,
    /// The view settings the results were read with. It is set only for
    /// the first window of a destination that is being opened.
    pub prefs: Option<crate::grid::prefs::ViewPrefs>,
    pub query: RootQuery,
    /// For a `Fetch::Refresh`, the current position of each of its items
    /// in the results, or `None` for an item that is no longer in them.
    pub places: Vec<Option<u64>>,
}

pub enum Reply {
    Ready(Vec<Navigation>, Vec<TagRecord>),
    Arrived(Box<Arrived>),
    /// A read of results failed. The fields are the query's generation,
    /// the reason for the read, and the error message.
    Failed(u64, Fetch, String),
    Error(String),
}
/// Returns whether a smart folder's filter has no rules. A smart folder
/// without rules only contains other smart folders (`Navigation::group`).
fn is_empty_filter(filter: &FilterExpr) -> bool {
    matches!(filter, FilterExpr::All(rules) if rules.is_empty())
}

fn append_tree(
    rows: &[(u32, Option<u32>, Navigation)],
    parent_key: Option<u32>,
    parent: Option<usize>,
    output: &mut Vec<Navigation>,
) {
    for (key, _, node) in rows.iter().filter(|(_, p, _)| *p == parent_key) {
        let mut node = node.clone();
        node.parent = parent;
        node.depth = parent.map(|i| output[i].depth + 1).unwrap_or(0);
        let index = output.len();
        output.push(node);
        append_tree(rows, Some(*key), Some(index), output);
    }
}
pub fn text_filter(text: &str) -> FilterExpr {
    if text.trim().chars().count() < 3 {
        FilterExpr::default()
    } else {
        FilterExpr::Clause(FilterClause::Text {
            field: TextField::Global,
            query: text.trim().to_owned(),
        })
    }
}
/// Returns the sidebar's destinations and the library's tags. The
/// destinations are in the order the sidebar shows them: the system rows,
/// then the tree of folders, then the tree of smart folders. This is read
/// when the library opens, and again whenever the library reports that
/// they changed.
pub fn navigation(library: &Library) -> picto_library::Result<(Vec<Navigation>, Vec<TagRecord>)> {
    let counts = library.sidebar_counts()?;
    let tags = library.tags()?;
    let system = |label: &str, icon, count, scope| Navigation {
        label: label.into(),
        count,
        scope,
        screen: None,
        random: false,
        icon: Some(String::from(icon)),
        tint: None,
        group: false,
        watched: false,
        depth: 0,
        parent: None,
    };
    let screen = |screen, label: &str, icon, count| Navigation {
        screen: Some(screen),
        ..system(label, icon, count, ItemScope::All)
    };
    // The system rows, in the order the sidebar shows them.
    let mut nav = vec![
        system(t("All"), "IconPhoto", Some(counts.all), ItemScope::All),
        system(t("Inbox"), "inbox", Some(counts.inbox), ItemScope::Inbox),
        system(
            t("Recently Viewed"),
            "clock",
            Some(counts.recently_viewed),
            ItemScope::RecentlyViewed,
        ),
        system(
            t("Uncategorized"),
            "folder-question",
            Some(counts.uncategorized),
            ItemScope::Uncategorized,
        ),
        system(
            t("Untagged"),
            "bookmark-question",
            Some(counts.untagged),
            ItemScope::Untagged,
        ),
        screen(
            Screen::Tags,
            t("Tags"),
            "IconBookmark",
            Some(tags.len() as u64),
        ),
        Navigation {
            random: true,
            ..system(t("Random"), "arrows-shuffle", None, ItemScope::All)
        },
        // i18n: an icon's name
        screen(Screen::Subscriptions, t("Subscriptions"), "download", None),
        screen(
            Screen::Duplicates,
            t("Duplicates"),
            "copy",
            Some(counts.duplicates),
        ),
        system(t("Trash"), "trash", Some(counts.trash), ItemScope::Trash),
    ];
    let folders = library
        .folders()?
        .into_iter()
        .map(|f| {
            (
                f.folder_id.0,
                f.parent_id.map(|p| p.0),
                Navigation {
                    label: f.name,
                    count: Some(f.count),
                    scope: ItemScope::FolderTree {
                        folder_id: f.folder_id,
                    },
                    screen: None,
                    random: false,
                    icon: f.icon,
                    tint: f.color.as_deref().and_then(crate::theme::parse_hex),
                    group: false,
                    watched: f.watch_enabled,
                    depth: 0,
                    parent: None,
                },
            )
        })
        .collect::<Vec<_>>();
    append_tree(&folders, None, None, &mut nav);
    let smart = library
        .smart_folders()?
        .into_iter()
        .map(|f| {
            let group = is_empty_filter(&f.view.filter);
            (
                f.smart_folder_id.0,
                f.parent_id.map(|p| p.0),
                Navigation {
                    label: f.name,
                    count: (!group).then_some(f.count),
                    scope: ItemScope::SmartFolder {
                        smart_folder_id: f.smart_folder_id,
                    },
                    screen: None,
                    random: false,
                    icon: f.icon,
                    tint: f.color.as_deref().and_then(crate::theme::parse_hex),
                    group,
                    watched: false,
                    depth: 0,
                    parent: None,
                },
            )
        })
        .collect::<Vec<_>>();
    append_tree(&smart, None, None, &mut nav);
    Ok((nav, tags))
}
/// Runs a job and reports to `stats.rs` how long it took.
fn run(job: Job, library: &Library) {
    let started = std::time::Instant::now();
    job(library);
    crate::stats::job_ran(started.elapsed());
}

/// Returns the position of the first item a window request asks for, or 0
/// for a request that gives a fraction of the results instead.
fn first(window: &WindowRequest) -> usize {
    match window.position {
        picto_library::query::WindowPosition::Ordinal(start) => start,
        picto_library::query::WindowPosition::Fraction(_) => 0,
    }
}

/// Reads the first window of a destination that is being opened. The sort
/// comes from the view settings saved in the library for the scope, which
/// may have been written by Picto 0.6.19 or earlier. `seed` is the seed of a
/// random sort; using the same seed for every window keeps the order the
/// same from one window to the next.
fn read_open(
    library: &Library,
    seed: &str,
    scope: ItemScope,
    filter: FilterExpr,
    shuffle: bool,
    window: &WindowRequest,
) -> picto_library::Result<(RootWindow, crate::grid::prefs::ViewPrefs, RootQuery)> {
    let saved = |key: &str| {
        library.read_auxiliary_json("view_pref", key).map(|row| {
            row.and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_else(|| serde_json::json!({}))
        })
    };
    let mut defaults = saved("grid:defaults")?;
    // When the saved defaults give no spacing, the spacing is the
    // application setting `gridSpacing`.
    if defaults.get("spacing").is_none_or(|v| v.is_null())
        && let Ok(Some(app)) = library.read_auxiliary_json("setting", "application")
        && let Ok(app) = serde_json::from_str::<serde_json::Value>(&app)
        && let Some(spacing) = app.get("gridSpacing").cloned()
        && let Some(defaults) = defaults.as_object_mut()
    {
        defaults.insert("spacing".into(), spacing);
    }
    let overrides = saved(&crate::grid::prefs::scope_key(&scope))?;
    let mut prefs = crate::grid::prefs::ViewPrefs::resolve(&scope, &defaults, &overrides, seed);
    if shuffle {
        prefs.sort = picto_library::predicate::ItemSort {
            field: picto_library::predicate::SortField::Random,
            direction: prefs.sort.direction,
            random_seed: Some(seed.to_owned()),
        };
    }
    // When `show_subfolders` is off, a folder shows only the items that
    // are directly in it, not the items of its subfolders.
    let scope = match scope {
        ItemScope::FolderTree { folder_id } if !prefs.show_subfolders => {
            ItemScope::Folder { folder_id }
        }
        scope => scope,
    };
    let query = RootQuery {
        scope,
        view: picto_library::predicate::ViewQuerySpec {
            filter,
            sort: prefs.sort.clone(),
        },
    };
    let started = std::time::Instant::now();
    let read = library.query_window(&query, window);
    crate::stats::window_read("open", first(window), started.elapsed());
    Ok((read?, prefs, query))
}

/// Reads another window of results that are already shown, with the query
/// they were first read with. For a `Fetch::Refresh` it also reads the
/// current positions of the given items in the results, in the same call on
/// the library's thread, so that the positions belong to the same state of
/// the library as the window.
fn read_window(
    library: &Library,
    query: &RootQuery,
    window: &WindowRequest,
    fetch: &Fetch,
) -> picto_library::Result<(RootWindow, Vec<Option<u64>>)> {
    let started = std::time::Instant::now();
    let read = library.query_window(query, window).and_then(|read| {
        let places = match fetch {
            Fetch::Refresh(ids) if !ids.is_empty() => library.root_positions(query, ids)?,
            _ => Vec::new(),
        };
        Ok((read, places))
    });
    crate::stats::window_read(
        match fetch {
            Fetch::Open => "open",
            Fetch::Move(_) => "move",
            Fetch::Refresh(_) => "refresh",
        },
        first(window),
        started.elapsed(),
    );
    read
}

/// Starts the library's thread. The thread has the application open the
/// library and then answers requests with the application's `Library`.
///
/// The destinations and their results are read as soon as the database is
/// open, while the application still runs its recovery passes. The screens'
/// own reads and edits (`Request::Job`) are held until the whole open has
/// finished, as the application's commands are.
pub fn start(
    application: Arc<crate::application::Application>,
) -> (Sender<Request>, Receiver<Reply>, Wake) {
    let (tx, requests) = mpsc::channel();
    let (sender, rx) = mpsc::channel();
    let (wake, woken) = futures::channel::mpsc::unbounded();
    let replies = Replies { sender, wake };
    let finished = tx.clone();
    let holder = application.clone();
    std::thread::spawn(move || {
        let work = || -> Result<(), Box<dyn std::error::Error>> {
            let (readable, opening) = mpsc::channel();
            let failed = readable.clone();
            std::thread::Builder::new()
                .name("picto-open".into())
                .spawn(move || {
                    let opened = application.open(move |library| {
                        crate::stats::opening("readable");
                        let _ = readable.send(Opening::Readable(library));
                    });
                    crate::stats::opening("finished");
                    if let Err(error) = &opened {
                        let _ = failed.send(Opening::Failed(error.clone()));
                    }
                    let _ = finished.send(Request::Opened(opened.map(|_| ())));
                })?;
            let library = match opening.recv()? {
                Opening::Readable(library) => library,
                Opening::Failed(error) => return Err(error.as_str().into()),
            };
            let library: &Library = &library;
            let (nav, tags) = navigation(library)?;
            crate::stats::opening("navigation-read");
            replies.send(Reply::Ready(nav, tags))?;
            // The same seed is used for every random sort until the app
            // quits, so that the order stays the same from one window of
            // results to the next.
            let seed = crate::now_ms().to_string();
            let mut held: Option<Vec<Job>> = Some(Vec::new());
            for request in requests {
                let result = match request {
                    Request::Opened(Ok(())) => {
                        for job in held.take().into_iter().flatten() {
                            run(job, library);
                        }
                        continue;
                    }
                    Request::Opened(Err(error)) => return Err(error.as_str().into()),
                    Request::Job(job) => {
                        match &mut held {
                            Some(held) => held.push(job),
                            None => run(job, library),
                        }
                        continue;
                    }
                    Request::Query(generation, scope, filter, shuffle, window) => {
                        read_open(library, &seed, scope, filter, shuffle, &window)
                            .map(|(window, prefs, query)| {
                                Reply::Arrived(Box::new(Arrived {
                                    generation,
                                    fetch: Fetch::Open,
                                    window,
                                    prefs: Some(prefs),
                                    query,
                                    places: Vec::new(),
                                }))
                            })
                            .map_err(|error| (generation, Fetch::Open, error))
                    }
                    Request::Window(generation, query, window, fetch) => {
                        match read_window(library, &query, &window, &fetch) {
                            Ok((window, places)) => Ok(Reply::Arrived(Box::new(Arrived {
                                generation,
                                fetch,
                                window,
                                prefs: None,
                                query: *query,
                                places,
                            }))),
                            Err(error) => Err((generation, fetch, error)),
                        }
                    }
                };
                replies.send(result.unwrap_or_else(|(generation, fetch, error)| {
                    Reply::Failed(generation, fetch, error.to_string())
                }))?;
            }
            Ok(())
        };
        if let Err(error) = work() {
            let _ = replies.send(Reply::Error(error.to_string()));
        }
        // `work` has returned and dropped the library, so the app no longer
        // holds any connection to its database.
        holder.release();
    });
    (tx, rx, woken)
}

/// Receives one message each time the library's thread has sent a reply,
/// so that the session reads the reply at once and does not wait until it
/// next checks for replies.
pub type Wake = futures::channel::mpsc::UnboundedReceiver<()>;

/// Sends the replies of the library's thread, and a message on `wake` after
/// each one so that the session reads it.
struct Replies {
    sender: Sender<Reply>,
    wake: futures::channel::mpsc::UnboundedSender<()>,
}

impl Replies {
    fn send(&self, reply: Reply) -> Result<(), mpsc::SendError<Reply>> {
        self.sender.send(reply)?;
        let _ = self.wake.unbounded_send(());
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colours_and_groups_are_read_as_the_sidebar_needs_them() {
        assert!(is_empty_filter(&FilterExpr::default()));
        assert!(!is_empty_filter(&text_filter("midna")));
    }
    #[test]
    fn short_search_does_not_empty_grid() {
        assert_eq!(text_filter(" ab "), FilterExpr::default());
        assert_eq!(text_filter(""), FilterExpr::default());
        assert_ne!(text_filter(" abc "), FilterExpr::default());
    }
}

#[cfg(test)]
mod acceptance {
    use super::*;
    use picto_library::query::PageRequest;
    #[test]
    fn hierarchy_keeps_children_with_parent() {
        let node = |name: &str| Navigation {
            label: name.into(),
            count: Some(0),
            scope: ItemScope::All,
            screen: None,
            random: false,
            icon: None,
            tint: None,
            group: false,
            watched: false,
            depth: 0,
            parent: None,
        };
        let mut result = vec![];
        append_tree(
            &[
                (2, Some(1), node("child")),
                (1, None, node("parent")),
                (3, None, node("sibling")),
            ],
            None,
            None,
            &mut result,
        );
        assert_eq!(
            result.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(),
            vec!["parent", "child", "sibling"]
        );
        assert_eq!(result[1].parent, Some(0));
        assert_eq!(result[1].depth, 1);
    }
    #[test]
    fn unicode_search_counts_characters_not_bytes() {
        assert_eq!(text_filter("日本"), FilterExpr::default());
        assert_ne!(text_filter("日本語"), FilterExpr::default());
    }
    /// Times every kind of read the grid makes, on a copy of a real library
    /// of any size, and prints one line for each. It fails when reading a
    /// window far into a destination takes much longer than reading the
    /// destination's first window. The grid holds one window at a time, so
    /// a read that gets slower with its position in a million items would
    /// make the grid stop for a moment at every jump and every fast scroll.
    /// It also times what the screens read on that library: each sort, each
    /// smart folder, searches, the summary of everything selected and the
    /// sidebar's counts.
    ///
    /// ```sh
    /// PICTO_GPUI_TEST_LIBRARY=<copy> cargo test --offline \
    ///     window_reads_stay_flat_across_the_library -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "Requires PICTO_GPUI_TEST_LIBRARY naming a disposable copy"]
    fn window_reads_stay_flat_across_the_library() {
        use picto_library::predicate::{ItemSort, SortDirection, SortField, ViewQuerySpec};
        use std::time::{Duration, Instant};
        let root = std::env::var("PICTO_GPUI_TEST_LIBRARY").unwrap();
        let root = crate::application::guard(std::path::Path::new(&root)).unwrap();
        let opening = Instant::now();
        let library = Library::open(root.join("library.sqlite")).unwrap();
        eprintln!("open the library: {:?}", opening.elapsed());
        let timed = |what: &str, query: &RootQuery, start: usize| {
            // The read is made three times. The first time is printed, and
            // the shortest of the three is what the test compares, so that a
            // moment in which another process kept the machine busy does not
            // fail the test.
            let mut times = Vec::new();
            let mut total = 0;
            for _ in 0..3 {
                let started = Instant::now();
                let window = library
                    .query_window(
                        query,
                        &WindowRequest::at(start, crate::grid::window::WINDOW),
                    )
                    .unwrap();
                times.push(started.elapsed());
                total = window.page.total as usize;
            }
            let best = times.iter().copied().min().unwrap();
            eprintln!(
                "{what} at {start}: first {:.1} ms, best {:.1} ms ({total} items)",
                times[0].as_secs_f64() * 1e3,
                best.as_secs_f64() * 1e3
            );
            (best, total)
        };
        let query = |scope: ItemScope, filter: FilterExpr, field, direction| RootQuery {
            scope,
            view: ViewQuerySpec {
                filter,
                sort: ItemSort {
                    field,
                    direction,
                    random_seed: Some("1".into()),
                },
            },
        };
        let fields = [
            SortField::ImportedAt,
            SortField::CapturedAt,
            SortField::Name,
            SortField::Rating,
            SortField::TotalSize,
            SortField::Random,
        ];
        let mut flat = Vec::new();
        for field in fields {
            for direction in [SortDirection::Descending, SortDirection::Ascending] {
                let all = query(ItemScope::All, FilterExpr::default(), field, direction);
                let label = format!("All by {field:?} {direction:?}");
                let (first, total) = timed(&label, &all, 0);
                let deepest = [total / 2, total.saturating_sub(crate::grid::window::WINDOW)]
                    .into_iter()
                    .map(|start| timed(&label, &all, start).0)
                    .max()
                    .unwrap();
                flat.push((label, first, deepest));
            }
        }
        let default_sort = |scope| {
            query(
                scope,
                FilterExpr::default(),
                SortField::ImportedAt,
                SortDirection::Descending,
            )
        };
        for folder in library.smart_folders().unwrap() {
            let scope = ItemScope::SmartFolder {
                smart_folder_id: folder.smart_folder_id,
            };
            let (_, total) = timed(
                &format!("smart folder {}", folder.name),
                &default_sort(scope.clone()),
                0,
            );
            timed(
                &format!("smart folder {}", folder.name),
                &default_sort(scope),
                total / 2,
            );
        }
        for scope in [
            ItemScope::Inbox,
            ItemScope::Trash,
            ItemScope::RecentlyViewed,
        ] {
            timed(&format!("{scope:?}"), &default_sort(scope.clone()), 0);
        }
        for text in ["lan", "land", "landscape", "needle", "zzzz-nothing"] {
            let search = query(
                ItemScope::All,
                text_filter(text),
                SortField::ImportedAt,
                SortDirection::Descending,
            );
            timed(&format!("search {text:?}"), &search, 0);
        }
        let started = Instant::now();
        let summary = library
            .selection_summary(&picto_library::selection::SelectionTarget::Query {
                query: default_sort(ItemScope::All),
                excluded_root_ids: Vec::new(),
            })
            .unwrap();
        eprintln!(
            "summary of everything selected: {:?} ({} items)",
            started.elapsed(),
            summary.selected_count
        );
        let started = Instant::now();
        navigation(&library).unwrap();
        eprintln!("sidebar destinations and counts: {:?}", started.elapsed());
        for (label, first, deepest) in flat {
            assert!(
                deepest <= first * 4 + Duration::from_millis(20),
                "{label}: a deep window took {deepest:?}, the first {first:?}"
            );
        }
    }

    #[test]
    #[ignore = "Requires PICTO_GPUI_TEST_LIBRARY naming a disposable copy made by run.sh"]
    fn real_snapshot_query_details_rating_undo_redo() {
        let root = std::env::var("PICTO_GPUI_TEST_LIBRARY").unwrap();
        let root = crate::application::guard(std::path::Path::new(&root)).unwrap();
        let library = Library::open(root.join("library.sqlite")).unwrap();
        let page = library
            .query(
                &RootQuery {
                    scope: ItemScope::All,
                    view: Default::default(),
                },
                &PageRequest {
                    limit: 20,
                    cursor: None,
                },
            )
            .unwrap();
        assert!(!page.items.is_empty());
        let id = page.items[0].root_id;
        let before = library.details(id).unwrap();
        assert!(!before.media.is_empty());
        let target = picto_library::selection::SelectionTarget::Explicit { root_ids: vec![id] };
        let changed = if before.rating == picto_library::Rating::Five {
            picto_library::Rating::One
        } else {
            picto_library::Rating::Five
        };
        library.set_rating(&target, changed).unwrap();
        assert_eq!(library.details(id).unwrap().rating, changed);
        library.undo().unwrap();
        assert_eq!(library.details(id).unwrap().rating, before.rating);
        library.redo().unwrap();
        assert_eq!(library.details(id).unwrap().rating, changed);
        library.undo().unwrap();
        for folder in library.smart_folders().unwrap().into_iter().take(3) {
            let page = library
                .query(
                    &RootQuery {
                        scope: ItemScope::SmartFolder {
                            smart_folder_id: folder.smart_folder_id,
                        },
                        view: Default::default(),
                    },
                    &PageRequest {
                        limit: 20,
                        cursor: None,
                    },
                )
                .unwrap();
            assert_eq!(page.total, folder.count);
        }
    }
}
