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
    pub count: u64,
    pub scope: ItemScope,
    pub depth: usize,
    pub parent: Option<usize>,
}
pub enum Request {
    Query(u64, ItemScope, FilterExpr, Option<String>),
    Details(RootId),
    Rate(RootId, picto_library::Rating),
    Name(RootId, String),
    Notes(RootId, String),
    Undo(RootId),
    Redo(RootId),
}
pub enum Reply {
    Ready(Vec<Navigation>, Vec<TagRecord>),
    Page(u64, bool, RootPage),
    Details(RootId, RootDetails),
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
            let mut nav = vec![
                Navigation {
                    label: "All".into(),
                    count: counts.all,
                    scope: ItemScope::All,
                    depth: 0,
                    parent: None,
                },
                Navigation {
                    label: "Inbox".into(),
                    count: counts.inbox,
                    scope: ItemScope::Inbox,
                    depth: 0,
                    parent: None,
                },
                Navigation {
                    label: "Recently Viewed".into(),
                    count: counts.recently_viewed,
                    scope: ItemScope::RecentlyViewed,
                    depth: 0,
                    parent: None,
                },
                Navigation {
                    label: "Uncategorized".into(),
                    count: counts.uncategorized,
                    scope: ItemScope::Uncategorized,
                    depth: 0,
                    parent: None,
                },
                Navigation {
                    label: "Untagged".into(),
                    count: counts.untagged,
                    scope: ItemScope::Untagged,
                    depth: 0,
                    parent: None,
                },
                Navigation {
                    label: "Trash".into(),
                    count: counts.trash,
                    scope: ItemScope::Trash,
                    depth: 0,
                    parent: None,
                },
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
                            count: f.count,
                            scope: ItemScope::FolderTree {
                                folder_id: f.folder_id,
                            },
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
                            count: f.count,
                            scope: ItemScope::SmartFolder {
                                smart_folder_id: f.smart_folder_id,
                            },
                            depth: 0,
                            parent: None,
                        },
                    )
                })
                .collect::<Vec<_>>();
            append_tree(&smart, None, None, &mut nav);
            replies.send(Reply::Ready(nav, library.tags()?))?;
            for request in requests {
                let result = match request {
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
                    Request::Query(generation, scope, filter, cursor) => {
                        let append = cursor.is_some();
                        let query = RootQuery {
                            scope,
                            view: picto_library::predicate::ViewQuerySpec {
                                filter,
                                ..Default::default()
                            },
                        };
                        library
                            .query(&query, &PageRequest { limit: 500, cursor })
                            .map(|page| Reply::Page(generation, append, page))
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
            count: 0,
            scope: ItemScope::All,
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
