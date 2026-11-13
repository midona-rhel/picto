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
    pub icon: Option<&'static str>,
    pub depth: usize,
    pub parent: Option<usize>,
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
    /// Takes the named tag (`namespace:subtag`) off the item.
    Untag(RootId, String),
    LeaveFolder(RootId, picto_library::FolderId),
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
pub fn start(database: PathBuf) -> (Sender<Request>, Receiver<Reply>) {
    let (tx, requests) = mpsc::channel();
    let (replies, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let work = || -> Result<(), Box<dyn std::error::Error>> {
            let library = Library::open(database)?;
            let counts = library.sidebar_counts()?;
            let tags = library.tags()?;
            let system = |label: &str, icon, count, scope| Navigation {
                label: label.into(),
                count,
                scope,
                screen: None,
                random: false,
                icon: Some(icon),
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
                            icon: None,
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
                    (
                        f.smart_folder_id.0,
                        f.parent_id.map(|p| p.0),
                        Navigation {
                            label: f.name,
                            count: Some(f.count),
                            scope: ItemScope::SmartFolder {
                                smart_folder_id: f.smart_folder_id,
                            },
                            screen: None,
                            random: false,
                            icon: None,
                            depth: 0,
                            parent: None,
                        },
                    )
                })
                .collect::<Vec<_>>();
            append_tree(&smart, None, None, &mut nav);
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
                    Request::Untag(id, name) => library
                        .apply_tags(
                            &picto_library::selection::SelectionTarget::Explicit {
                                root_ids: vec![id],
                            },
                            &[name],
                            false,
                        )
                        .and_then(|_| library.details(id))
                        .map(|details| Reply::Details(id, details)),
                    Request::LeaveFolder(id, folder) => library
                        .remove_from_folder(
                            &picto_library::selection::SelectionTarget::Explicit {
                                root_ids: vec![id],
                            },
                            folder,
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
