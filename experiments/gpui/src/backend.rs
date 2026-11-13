use picto_library::predicate::{FilterClause, FilterExpr, TextField};
use picto_library::query::{ItemScope, PageRequest, RootPage, RootQuery};
use picto_library::{Library, RootDetails, RootId, TagRecord};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

#[derive(Clone)]
pub struct Navigation {
    pub label: String,
    /// Rows without a count (Random, Subscriptions) show none.
    pub count: Option<u64>,
    pub scope: ItemScope,
    /// A destination that is its own screen, not a grid of this scope.
    pub screen: Option<Screen>,
    /// The All scope in a shuffled order.
    pub random: bool,
    /// The row's glyph: a system row's own, or the one the user chose for
    /// a folder (a name from the icon picker's registry, such as `IconPhoto`).
    pub icon: Option<String>,
    /// The colour the user gave a folder, which tints its glyph.
    pub tint: Option<u32>,
    /// A smart folder with no rules of its own. It only holds others: it
    /// has no count and cannot be opened.
    pub group: bool,
    pub depth: usize,
    pub parent: Option<usize>,
}
impl Navigation {
    /// The id the Electron sidebar gives this destination (`system:inbox`,
    /// `folder:5`, `smart:3`). It stays the same when the list is rebuilt,
    /// where a position does not.
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

/// The destinations that are screens of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Tags,
    Subscriptions,
    Duplicates,
}

/// Work for the library's thread: a screen's own reads and edits.
pub type Job = Box<dyn FnOnce(&Library) + Send>;

pub enum Request {
    Job(Job),
    /// Generation, scope, filter, cursor, and whether to shuffle.
    Query(u64, ItemScope, FilterExpr, Option<String>, bool),
    Details(RootId),
    Rate(RootId, picto_library::Rating),
    Name(RootId, String),
    Notes(RootId, String),
    Lifecycle(Vec<RootId>, picto_library::Lifecycle),
    Undo(RootId),
    Redo(RootId),
}
pub enum Reply {
    Ready(Vec<Navigation>, Vec<TagRecord>),
    Page(u64, bool, RootPage, crate::grid::prefs::ViewPrefs),
    Details(RootId, RootDetails),
    /// The results on screen may no longer be the scope's.
    Changed,
    Error(String),
}
/// Whether a smart folder's filter has no rules, which makes it a group.
fn is_empty_filter(filter: &FilterExpr) -> bool {
    matches!(filter, FilterExpr::All(rules) if rules.is_empty())
}

/// `#rrggbb` as a number.
fn hex_color(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    (digits.len() == 6)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
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
/// The sidebar's destinations in the Electron order: the system rows, then
/// the folder tree, then the smart-folder tree; and the tags. Built when the
/// library opens, and again after the sidebar edits a folder.
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
        depth: 0,
        parent: None,
    };
    let screen = |screen, label: &str, icon, count| Navigation {
        screen: Some(screen),
        ..system(label, icon, count, ItemScope::All)
    };
    // The order of the Electron sidebar's system rows.
    let mut nav = vec![
        system("All", "photo", Some(counts.all), ItemScope::All),
        system("Inbox", "inbox", Some(counts.inbox), ItemScope::Inbox),
        system(
            "Recently Viewed",
            "clock",
            Some(counts.recently_viewed),
            ItemScope::RecentlyViewed,
        ),
        system(
            "Uncategorized",
            "folder-question",
            Some(counts.uncategorized),
            ItemScope::Uncategorized,
        ),
        system(
            "Untagged",
            "bookmark-question",
            Some(counts.untagged),
            ItemScope::Untagged,
        ),
        screen(Screen::Tags, "Tags", "bookmark", Some(tags.len() as u64)),
        Navigation {
            random: true,
            ..system("Random", "arrows-shuffle", None, ItemScope::All)
        },
        screen(Screen::Subscriptions, "Subscriptions", "download", None),
        screen(
            Screen::Duplicates,
            "Duplicates",
            "copy",
            Some(counts.duplicates),
        ),
        system("Trash", "trash", Some(counts.trash), ItemScope::Trash),
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
                    tint: f.color.as_deref().and_then(hex_color),
                    group: false,
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
                    tint: f.color.as_deref().and_then(hex_color),
                    group,
                    depth: 0,
                    parent: None,
                },
            )
        })
        .collect::<Vec<_>>();
    append_tree(&smart, None, None, &mut nav);
    Ok((nav, tags))
}
pub fn start(database: PathBuf) -> (Sender<Request>, Receiver<Reply>) {
    let (tx, requests) = mpsc::channel();
    let (replies, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let work = || -> Result<(), Box<dyn std::error::Error>> {
            let library = Library::open(database)?;
            let (nav, tags) = navigation(&library)?;
            replies.send(Reply::Ready(nav, tags))?;
            // One seed per session keeps a random sort stable while paging.
            let seed = now_ms().to_string();
            for request in requests {
                let request = match request {
                    Request::Job(job) => {
                        job(&library);
                        continue;
                    }
                    other => other,
                };
                let result = match request {
                    Request::Job(_) => unreachable!("jobs are run above"),
                    Request::Name(id, name) => library
                        .rename_root(id, &name, now_ms())
                        .and_then(|_| library.details(id))
                        .map(|details| Reply::Details(id, details)),
                    Request::Notes(id, notes) => library
                        .set_notes(
                            &picto_library::selection::SelectionTarget::Explicit {
                                root_ids: vec![id],
                            },
                            Some(notes),
                            now_ms(),
                        )
                        .and_then(|_| library.details(id))
                        .map(|details| Reply::Details(id, details)),
                    Request::Query(generation, scope, filter, cursor, shuffle) => {
                        let append = cursor.is_some();
                        // The scope's saved view decides the sort, exactly as it
                        // does for the Electron grid reading the same rows.
                        let saved = |key: &str| {
                            library.read_auxiliary_json("view_pref", key).map(|row| {
                                row.and_then(|json| serde_json::from_str(&json).ok())
                                    .unwrap_or_else(|| serde_json::json!({}))
                            })
                        };
                        saved("grid:defaults")
                            .map(|mut defaults| {
                                // A view without a spacing of its own takes
                                // the `gridSpacing` setting (`gridSpacingAtom`).
                                if defaults.get("spacing").is_none_or(|v| v.is_null())
                                    && let Ok(Some(app)) =
                                        library.read_auxiliary_json("setting", "application")
                                    && let Ok(app) = serde_json::from_str::<serde_json::Value>(&app)
                                    && let Some(spacing) = app.get("gridSpacing").cloned()
                                    && let Some(defaults) = defaults.as_object_mut()
                                {
                                    defaults.insert("spacing".into(), spacing);
                                }
                                defaults
                            })
                            .and_then(|defaults| {
                                saved(&crate::grid::prefs::scope_key(&scope))
                                    .map(|overrides| (defaults, overrides))
                            })
                            .and_then(|(defaults, overrides)| {
                                let mut prefs = crate::grid::prefs::ViewPrefs::resolve(
                                    &scope, &defaults, &overrides, &seed,
                                );
                                if shuffle {
                                    prefs.sort = picto_library::predicate::ItemSort {
                                        field: picto_library::predicate::SortField::Random,
                                        direction: prefs.sort.direction,
                                        random_seed: Some(seed.clone()),
                                    };
                                }
                                // A folder without its subfolders' items
                                // (`show_subfolders` off) is the folder alone.
                                let scope = match scope {
                                    ItemScope::FolderTree { folder_id }
                                        if !prefs.show_subfolders =>
                                    {
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
                                library
                                    .query(&query, &PageRequest { limit: 500, cursor })
                                    .map(|page| Reply::Page(generation, append, page, prefs))
                            })
                    }
                    Request::Rate(id, rating) => library
                        .set_rating(
                            &picto_library::selection::SelectionTarget::Explicit {
                                root_ids: vec![id],
                            },
                            rating,
                        )
                        .and_then(|_| library.details(id))
                        .map(|details| Reply::Details(id, details)),
                    Request::Lifecycle(ids, lifecycle) => library
                        .set_lifecycle(
                            &picto_library::selection::SelectionTarget::Explicit { root_ids: ids },
                            lifecycle,
                        )
                        .map(|_| Reply::Changed),
                    Request::Undo(id) => library
                        .undo()
                        .and_then(|_| library.details(id))
                        .map(|details| Reply::Details(id, details)),
                    Request::Redo(id) => library
                        .redo()
                        .and_then(|_| library.details(id))
                        .map(|details| Reply::Details(id, details)),
                    Request::Details(id) => library
                        .details(id)
                        .map(|details| Reply::Details(id, details)),
                };
                replies.send(result.unwrap_or_else(|error| Reply::Error(error.to_string())))?;
            }
            Ok(())
        };
        if let Err(error) = work() {
            let _ = replies.send(Reply::Error(error.to_string()));
        }
    });
    (tx, rx)
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colours_and_groups_are_read_as_the_sidebar_needs_them() {
        assert_eq!(hex_color("#3297ff"), Some(0x3297ff));
        assert_eq!(hex_color("868e96"), Some(0x868e96));
        assert_eq!(hex_color("#fff"), None);
        assert_eq!(hex_color("#zzzzzz"), None);
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
    #[test]
    #[ignore = "Requires PICTO_GPUI_TEST_DB pointing to a marked disposable snapshot"]
    fn real_snapshot_query_details_rating_undo_redo() {
        let path = PathBuf::from(std::env::var("PICTO_GPUI_TEST_DB").unwrap());
        assert!(path.parent().unwrap().join(".gpui-snapshot").is_file());
        let library = Library::open(path).unwrap();
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
