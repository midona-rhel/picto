//! The rules that decide where an import goes and what it asks the
//! application for. The functions here do not read or change any state.
use picto_library::FolderId;
use picto_library::query::ItemScope;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The destination of an import that the user starts. The items go to the
/// Inbox when the Inbox is open, otherwise to the library, and into the
/// open folder when a folder is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Destination {
    pub inbox: bool,
    pub folder: Option<FolderId>,
}

impl Destination {
    /// Returns the destination of an import made while `scope` is open.
    pub fn for_scope(scope: &ItemScope) -> Self {
        Self {
            inbox: matches!(scope, ItemScope::Inbox),
            folder: match scope {
                ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id } => {
                    Some(*folder_id)
                }
                _ => None,
            },
        }
    }

    /// Returns the destination for a folder chosen from the sidebar ("Import
    /// Folder Here..."), or for the library's top level when `folder` is
    /// `None`.
    pub fn folder(folder: Option<FolderId>) -> Self {
        Self {
            inbox: false,
            folder,
        }
    }

    fn lifecycle(self) -> &'static str {
        if self.inbox { "inbox" } else { "active" }
    }
}

/// What the command `imports.enqueue` is told besides the paths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params {
    pub destination: Destination,
    pub tags: Vec<String>,
    pub source_urls: Vec<String>,
    pub preserve_structure: bool,
    pub include_subfolders: Option<bool>,
    pub include_folders_without_media: bool,
    pub watch_source_folder: bool,
    pub delete_after_ingest: bool,
    /// Whether several files become one collection. It is `None` until the
    /// multi-file import setting or the user has decided it.
    pub group_files: Option<bool>,
}

impl Params {
    pub fn to(destination: Destination) -> Self {
        Self {
            destination,
            ..Default::default()
        }
    }

    /// Returns the arguments of `imports.enqueue` for `paths`.
    /// `include_subfolders` is left out when it is not set, so that the
    /// application's default applies.
    pub fn enqueue(&self, paths: &[PathBuf]) -> Value {
        let mut args = json!({
            "paths": paths.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
            "lifecycle": self.destination.lifecycle(),
            "parent_folder_id": self.destination.folder.map(|folder| folder.0),
            "tags": self.tags,
            "source_urls": self.source_urls,
            "preserve_structure": self.preserve_structure,
            "include_folders_without_media": self.include_folders_without_media,
            "watch_source_folder": self.watch_source_folder,
            "delete_after_ingest": self.delete_after_ingest,
            "group_files": self.group_files.unwrap_or(false),
        });
        if let Some(include) = self.include_subfolders {
            args["include_subfolders"] = json!(include);
        }
        args
    }
}

/// The extensions by which a dropped path is taken to be media when the
/// path cannot be inspected. One path without one of them is then taken
/// for a folder.
const MEDIA_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "svg", "mp4", "mkv", "webm", "avi",
    "mov", "wmv", "flv", "m4v", "avif", "jxl", "ico", "pdf",
];

fn has_media_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            MEDIA_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// What a drop or a paste of some paths is to do.
#[derive(Debug, PartialEq)]
pub enum Dropped {
    /// One Picto Pack, which is imported through the pack dialog.
    Pack(PathBuf),
    /// Picto Packs together with other files, or several packs.
    PackMixed,
    /// One folder, which opens the folder dialog.
    Folder(PathBuf),
    /// Files that are imported as they are.
    Files(Vec<PathBuf>),
    Nothing,
}

/// Returns whether the path is a Picto Pack, which its extension
/// `.picto-pack` says, in any letter case.
pub fn is_pack(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("picto-pack"))
}

/// Decides what a drop of `paths` does. A Picto Pack on its own is a pack,
/// and a pack with anything else is refused. One path is a folder when
/// `is_dir` says so. When `is_dir` cannot say, the path is a folder if it
/// has no known media extension. Everything else is imported as files.
pub fn classify(paths: Vec<PathBuf>, is_dir: impl Fn(&Path) -> Option<bool>) -> Dropped {
    let packs = paths.iter().filter(|path| is_pack(path)).count();
    if packs > 0 {
        return if paths.len() == 1 {
            Dropped::Pack(paths.into_iter().next().expect("one path"))
        } else {
            Dropped::PackMixed
        };
    }
    match paths.as_slice() {
        [] => Dropped::Nothing,
        [one] if is_dir(one).unwrap_or_else(|| !has_media_extension(one)) => {
            Dropped::Folder(one.clone())
        }
        _ => Dropped::Files(paths),
    }
}

/// Whether files are imported at once or the user is asked first. One
/// file, or files whose grouping is already decided, are imported at once.
/// Otherwise the multi-file import setting decides: "ask" asks the user,
/// "group" imports the files as one collection, and any other value
/// imports them as separate items.
#[derive(Debug, PartialEq)]
pub enum Batch {
    Now(Params),
    Ask,
}

pub fn batch(count: usize, params: Params, behavior: &str) -> Batch {
    if count <= 1 || params.group_files.is_some() {
        return Batch::Now(params);
    }
    match behavior {
        "ask" => Batch::Ask,
        behavior => Batch::Now(Params {
            group_files: Some(behavior == "group"),
            ..params
        }),
    }
}

/// Returns what the folder dialog says when the folder is deeper than
/// Picto keeps. Returns `None` when no level of the folder is merged into
/// another.
pub fn consolidation_message(analysis: &Value) -> Option<String> {
    let levels = analysis["consolidated_levels"].as_u64()?;
    let retained = analysis["retained_depth"].as_u64()?;
    if levels == 0 {
        return None;
    }
    if retained == 0 {
        return Some(
            crate::i18n::t("This Picto folder is already at level 8. Files from the selected folder's subfolders will be added directly here, so no media is skipped because of folder depth.")
                .into(),
        );
    }
    let label = if retained == 1 {
        crate::i18n::t("level")
    } else {
        crate::i18n::t("levels")
    };
    Some(crate::tf!(
        "This folder goes past Picto's 8-level folder limit. Picto can keep the first {value0} {value1} here. Files from deeper folders will be placed in the nearest kept folder, so no media is skipped because of folder depth.",
        retained,
        label
    ))
}

#[cfg(test)]
mod tests {
    use super::{Batch, Destination, Dropped, Params, batch, classify, consolidation_message};
    use picto_library::FolderId;
    use picto_library::query::ItemScope;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn imports_go_to_the_inbox_only_from_the_inbox_and_into_the_open_folder() {
        assert_eq!(
            Destination::for_scope(&ItemScope::Inbox),
            Destination {
                inbox: true,
                folder: None
            }
        );
        assert_eq!(
            Destination::for_scope(&ItemScope::Trash),
            Destination::default()
        );
        let folder = Destination::for_scope(&ItemScope::FolderTree {
            folder_id: FolderId(4),
        });
        assert_eq!(folder.folder, Some(FolderId(4)));
        assert!(!folder.inbox);
    }

    #[test]
    fn enqueue_arguments_name_the_destination() {
        let params = Params::to(Destination {
            inbox: true,
            folder: Some(FolderId(9)),
        });
        let args = params.enqueue(&[PathBuf::from("/a/b.png")]);
        assert_eq!(args["paths"], json!(["/a/b.png"]));
        assert_eq!(args["lifecycle"], "inbox");
        assert_eq!(args["parent_folder_id"], 9);
        assert_eq!(args["group_files"], false);
        assert!(args.get("include_subfolders").is_none());
        let library = Params::to(Destination::default()).enqueue(&[]);
        assert_eq!(library["lifecycle"], "active");
        assert!(library["parent_folder_id"].is_null());
    }

    #[test]
    fn a_drop_is_a_pack_a_folder_or_files() {
        let path = PathBuf::from;
        // `is_dir` cannot say what the path is, so the extension decides.
        let unknown = |_: &std::path::Path| None;
        assert_eq!(
            classify(vec![path("/x/Set.picto-pack")], unknown),
            Dropped::Pack(path("/x/Set.picto-pack"))
        );
        assert_eq!(
            classify(vec![path("/x/Set.picto-pack"), path("/x/a.png")], unknown),
            Dropped::PackMixed
        );
        assert_eq!(
            classify(vec![path("/x/Holiday")], unknown),
            Dropped::Folder(path("/x/Holiday"))
        );
        assert_eq!(
            classify(vec![path("/x/movie.swf")], unknown),
            Dropped::Folder(path("/x/movie.swf"))
        );
        assert_eq!(
            classify(vec![path("/x/A.PNG")], unknown),
            Dropped::Files(vec![path("/x/A.PNG")])
        );
        assert_eq!(
            classify(vec![path("/x/a.swf"), path("/x/b")], unknown),
            Dropped::Files(vec![path("/x/a.swf"), path("/x/b")])
        );
        assert_eq!(classify(Vec::new(), unknown), Dropped::Nothing);
    }

    #[test]
    fn one_file_of_any_type_imports_and_one_folder_asks() {
        let path = PathBuf::from;
        let file = |_: &std::path::Path| Some(false);
        let folder = |_: &std::path::Path| Some(true);
        assert_eq!(
            classify(vec![path("/x/movie.swf")], file),
            Dropped::Files(vec![path("/x/movie.swf")])
        );
        assert_eq!(
            classify(vec![path("/x/Shots.png")], folder),
            Dropped::Folder(path("/x/Shots.png"))
        );
    }

    #[test]
    fn several_files_follow_the_multi_file_setting() {
        let params = Params::default();
        assert_eq!(batch(1, params.clone(), "ask"), Batch::Now(params.clone()));
        assert_eq!(batch(3, params.clone(), "ask"), Batch::Ask);
        let Batch::Now(grouped) = batch(3, params.clone(), "group") else {
            panic!("group goes at once");
        };
        assert_eq!(grouped.group_files, Some(true));
        let Batch::Now(separate) = batch(3, params, "separate") else {
            panic!("separate goes at once");
        };
        assert_eq!(separate.group_files, Some(false));
    }

    #[test]
    fn deep_folders_are_explained() {
        assert_eq!(
            consolidation_message(&json!({"consolidated_levels": 0, "retained_depth": 3})),
            None
        );
        assert!(
            consolidation_message(&json!({"consolidated_levels": 2, "retained_depth": 1}))
                .unwrap()
                .contains("the first 1 level here")
        );
        assert!(
            consolidation_message(&json!({"consolidated_levels": 2, "retained_depth": 0}))
                .unwrap()
                .starts_with("This Picto folder is already at level 8")
        );
    }
}
