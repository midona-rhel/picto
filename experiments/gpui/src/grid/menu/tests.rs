//! The cases of `gridContextMenu.test.tsx`, and the full row order for the
//! selections the grid, the viewer and the preview open the menu on.
use super::{
    Action, Description, Engine, Handlers, ItemRow, Kind, OpenWith, Panel, Playback, Row,
    ScopeKind, Status, Surface, collection_menu, empty_menu, member_handlers, preview_menu,
    tile_menu, viewer_menu, with_remove_from_group,
};

/// The row that shows the file in the system's file manager.
const REVEAL: &str = crate::app_menu::Platform::CURRENT.reveal_title();
use crate::settings::registry::{self, display};
use picto_library::ContentSortField;

/// `shortcutHint` with the default bindings: none for a deferred shortcut.
fn shortcut(id: &str) -> Option<String> {
    let def = registry::SHORTCUT_DEFS.iter().find(|def| def.id == id)?;
    (!def.deferred).then(|| registry::display(def.keys))
}

fn tile(d: &Description) -> Vec<Row> {
    tile_menu(d, &shortcut)
}

fn empty(d: &Description) -> Vec<Row> {
    empty_menu(d, &shortcut)
}

/// `cleanSeparators`, as the menu draws the rows.
fn clean(rows: &[Row]) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for row in rows {
        match row {
            Row::Separator => {
                if out.last().is_some_and(|last| *last != Row::Separator) {
                    out.push(Row::Separator);
                }
            }
            other => out.push(other.clone()),
        }
    }
    if out.last() == Some(&Row::Separator) {
        out.pop();
    }
    out
}

/// The rows' labels, `-` for a separator, as the menu shows them.
fn labels(rows: &[Row]) -> Vec<String> {
    clean(rows)
        .iter()
        .map(|row| match row {
            Row::Item(item) => item.label.clone(),
            Row::Separator => "-".to_owned(),
            Row::Submenu { label, .. } => format!("{label} >"),
            Row::Panel(Panel::Layout) => "[view panel]".to_owned(),
            Row::Panel(Panel::Display) => "[display panel]".to_owned(),
        })
        .collect()
}

fn find<'a>(rows: &'a [Row], label: &str) -> Option<&'a ItemRow> {
    rows.iter().find_map(|row| match row {
        Row::Item(item) if item.label == label => Some(item),
        _ => None,
    })
}

fn children<'a>(rows: &'a [Row], name: &str) -> &'a [Row] {
    rows.iter()
        .find_map(|row| match row {
            Row::Submenu {
                label, children, ..
            } if label == name => Some(children.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {name} submenu"))
}

fn has_submenu(rows: &[Row], name: &str) -> bool {
    rows.iter()
        .any(|row| matches!(row, Row::Submenu { label, .. } if label == name))
}

fn single(hash: &str) -> Description {
    Description {
        selection_count: 1,
        single: true,
        single_hash: Some(hash.into()),
        scope: Some(ScopeKind::System),
        ..Default::default()
    }
}

fn several(count: usize) -> Description {
    Description {
        selection_count: count,
        scope: Some(ScopeKind::System),
        ..Default::default()
    }
}

#[test]
fn folder_removal_takes_the_delete_shortcut_from_trash() {
    let mut d = single("hash");
    d.handlers.remove_from_folder = true;
    d.handlers.move_to_trash = true;
    d.scope = Some(ScopeKind::Folder);
    let rows = tile(&d);
    let remove = find(&rows, "Remove from Folder").unwrap();
    assert_eq!(remove.shortcut, shortcut("file.removeFromFolder"));
    assert_eq!(remove.shortcut, Some(display("Mod+Backspace").into()));
    assert_eq!(remove.action, Action::RemoveFromFolder);
    assert_eq!(find(&rows, "Move to Trash").unwrap().shortcut, None);
    for scope in [ScopeKind::System, ScopeKind::SmartFolder] {
        d.scope = Some(scope);
        let rows = tile(&d);
        assert!(find(&rows, "Remove from Folder").is_none());
        assert_eq!(
            find(&rows, "Move to Trash").unwrap().shortcut,
            shortcut("file.delete")
        );
    }
}

#[test]
fn finds_items_with_the_exact_media_for_any_type() {
    for mime in ["image/png", "video/mp4", "application/pdf"] {
        let mut d = single("exact-hash");
        d.single_item_id = Some(42);
        d.single_kind = Some(Kind::Media);
        d.single_mime = Some(mime.into());
        d.handlers.find_media_matches = true;
        let rows = tile(&d);
        let entry = find(children(&rows, "More"), "Find Items with This Media").unwrap();
        assert_eq!(entry.action, Action::FindMediaMatches);
        assert!(!entry.disabled);
    }
}

#[test]
fn offers_every_reverse_image_provider_for_images_only() {
    let mut d = single("image-hash");
    d.single_kind = Some(Kind::Media);
    d.single_mime = Some("image/png".into());
    d.handlers.search_by_image = true;
    let rows = tile(&d);
    let search = children(&rows, "Search by Image");
    assert_eq!(
        labels(search),
        [
            "TinEye",
            "SauceNAO",
            "Yandex Images",
            "Sogou",
            "Bing Visual Search"
        ]
    );
    assert_eq!(
        find(search, "Sogou").unwrap().action,
        Action::SearchByImage(Engine::Sogou)
    );
    // Drawn as production draws them, disabled until the search is built.
    assert!(
        labels(search)
            .iter()
            .all(|label| find(search, label).unwrap().disabled)
    );
    d.single_mime = Some("video/mp4".into());
    assert!(!has_submenu(&tile(&d), "Search by Image"));
}

#[test]
fn export_separates_originals_from_converted() {
    let mut d = single("hash");
    d.handlers.export = true;
    d.handlers.export_originals = true;
    let rows = tile(&d);
    let export = children(&rows, "Export");
    assert_eq!(labels(export), ["Export Originals...", "Export As..."]);
    assert!(
        export
            .iter()
            .all(|row| matches!(row, Row::Item(item) if !item.disabled))
    );
    assert!(rows.iter().any(|row| matches!(
        row,
        Row::Submenu { label, icon: Some("menu-file-export"), .. } if label == "Export"
    )));
}

#[test]
fn grayscale_is_a_checked_row_that_keeps_the_menu_open() {
    let mut d = single("hash");
    d.single_kind = Some(Kind::Media);
    d.grayscale = true;
    d.handlers.toggle_grayscale = true;
    let rows = tile(&d);
    let entry = find(&rows, "Grayscale").unwrap();
    assert_eq!(entry.checked, Some(true));
    assert!(entry.keep_open);
    assert!(!entry.disabled);
    assert_eq!(entry.icon, Some("menu-contrast"));
}

fn empty_space(scope: Option<ScopeKind>) -> Description {
    Description {
        scope,
        ..Default::default()
    }
}

#[test]
fn empty_space_offers_creation_and_import() {
    let mut d = empty_space(None);
    d.handlers.new_folder = true;
    d.handlers.new_smart_folder = true;
    d.handlers.import_files = true;
    d.handlers.import_folder = true;
    d.handlers.paste_import = true;
    let rows = empty(&d);
    for (label, action) in [
        ("New Folder", Action::NewFolder),
        ("New Smart Folder", Action::NewSmartFolder),
        ("Import Files...", Action::ImportFiles),
        ("Import Folder...", Action::ImportFolder),
        ("Paste Import", Action::PasteImport),
    ] {
        let entry = find(&rows, label).unwrap_or_else(|| panic!("missing {label}"));
        assert_eq!(entry.action, action);
        assert!(!entry.disabled);
    }
    assert_eq!(
        labels(&rows),
        [
            "New Folder",
            "New Smart Folder",
            "-",
            "Import Files...",
            "Import Folder...",
            "Paste Import",
            "-",
            "[view panel]",
            "-",
            "Display >",
        ]
    );
    // `buildContextMenuViewEntries`: the display switches are a panel of
    // their own inside "Display".
    assert_eq!(labels(children(&rows, "Display")), ["[display panel]"]);
    // Another import running blocks the three import rows.
    d.import_blocked = true;
    let rows = empty(&d);
    for label in ["Import Files...", "Import Folder...", "Paste Import"] {
        assert!(find(&rows, label).unwrap().disabled);
    }
}

#[test]
fn no_smart_folder_creation_inside_a_folder() {
    let mut d = empty_space(Some(ScopeKind::Folder));
    d.handlers.new_smart_folder = true;
    assert!(find(&empty(&d), "New Smart Folder").is_none());
}

#[test]
fn no_creation_in_smart_folder_results() {
    let mut d = empty_space(Some(ScopeKind::SmartFolder));
    d.handlers.new_folder = true;
    d.handlers.new_smart_folder = true;
    let rows = empty(&d);
    assert!(find(&rows, "New Folder").is_none());
    assert!(find(&rows, "New Smart Folder").is_none());
}

#[test]
fn empty_space_in_a_folder_sorts_its_contents() {
    let mut d = empty_space(Some(ScopeKind::Folder));
    d.handlers.sort_contents = true;
    let rows = empty(&d);
    let size = find(children(&rows, "Sort by"), "Size").unwrap();
    assert_eq!(size.action, Action::SortContents(ContentSortField::Size));
    assert_eq!(
        labels(children(&rows, "Sort by")),
        [
            "Name",
            "Import Date",
            "Date Created",
            "Date Modified",
            "Size",
            "Notes"
        ]
    );
    // The folder's own sort, then the view panel (which offers no sort of
    // the view in a folder).
    assert_eq!(
        labels(&rows),
        ["Sort by >", "-", "[view panel]", "-", "Display >"]
    );
}

#[test]
fn a_selection_in_a_folder_can_sort_its_contents() {
    let mut d = single("hash");
    d.single_kind = Some(Kind::Media);
    d.scope = Some(ScopeKind::Folder);
    d.handlers.sort_contents = true;
    let rows = tile(&d);
    assert_eq!(
        find(children(&rows, "Sort by"), "Import Date")
            .unwrap()
            .action,
        Action::SortContents(ContentSortField::ImportedAt)
    );
}

#[test]
fn the_viewer_offers_no_sorting_or_view_options() {
    let mut d = single("hash");
    d.surface = Surface::Viewer;
    d.single_kind = Some(Kind::Media);
    d.scope = Some(ScopeKind::Folder);
    d.handlers.sort_contents = true;
    let rows = tile(&d);
    assert!(!has_submenu(&rows, "Sort by"));
    assert!(!has_submenu(&rows, "Display"));
    assert!(!rows.iter().any(|row| matches!(row, Row::Panel(_))));
    assert!(find(&rows, "Grayscale").is_none());
}

#[test]
fn paste_import_needs_an_importable_clipboard() {
    let mut d = empty_space(None);
    d.handlers.import_files = true;
    d.handlers.import_folder = true;
    assert!(find(&empty(&d), "Paste Import").is_none());
}

#[test]
fn several_items_and_a_collection_copy_through_the_selection() {
    for (count, is_single, kind) in [(2, false, None), (1, true, Some(Kind::Collection))] {
        let d = Description {
            selection_count: count,
            single: is_single,
            single_hash: is_single.then(|| "cover-hash".to_owned()),
            single_kind: kind,
            handlers: Handlers {
                copy_selection: true,
                copy_selection_paths: true,
                copy_selection_names: true,
                copy_selection_links: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let rows = tile(&d);
        let names = if count == 1 {
            "Copy Name"
        } else {
            "Copy Names"
        };
        for (label, action) in [
            ("Copy", Action::CopySelection),
            ("Copy File Paths", Action::CopySelectionPaths),
            (names, Action::CopySelectionNames),
            ("Copy as Links", Action::CopySelectionLinks),
        ] {
            assert_eq!(find(&rows, label).unwrap().action, action);
        }
    }
}

#[test]
fn open_with_other_while_applications_are_looked_up() {
    let mut d = single("hash");
    d.open_with = OpenWith::Pending;
    let rows = tile(&d);
    let pending = children(&rows, "Open With Other");
    assert_eq!(labels(pending), ["Loading applications..."]);
    assert!(find(pending, "Loading applications...").unwrap().disabled);
}

fn application(name: &str, default: bool) -> crate::desktop::Application {
    crate::desktop::Application {
        name: name.into(),
        path: format!("/Applications/{name}.app").into(),
        icon: None,
        default,
    }
}

#[test]
fn open_with_other_lists_the_applications_with_the_default_marked() {
    let mut d = single("hash");
    d.open_with = OpenWith::Applications(vec![
        application("Preview", true),
        application("Safari", false),
    ]);
    d.handlers.open_with_application = true;
    let rows = tile(&d);
    let listed = children(&rows, "Open With Other");
    assert_eq!(labels(listed), ["Preview (Default)", "Safari"]);
    let safari = find(listed, "Safari").unwrap();
    assert_eq!(safari.action, Action::OpenWithApplication);
    assert_eq!(safari.value.as_deref(), Some("/Applications/Safari.app"));
    // Without an icon of its own, a row shows the apps glyph.
    assert_eq!(safari.icon, Some("menu-apps"));
}

#[test]
fn open_with_other_is_left_out_when_nothing_opens_the_file() {
    let mut d = single("hash");
    d.open_with = OpenWith::Applications(Vec::new());
    d.handlers.open_with_application = true;
    assert!(!labels(&tile(&d)).contains(&"Open With Other".to_owned()));
    // The looked-up row that replaces the pending one is then none.
    assert!(super::open_with_rows(Vec::new()).is_empty());
    assert_eq!(
        super::open_with_rows(vec![application("Preview", true)]).len(),
        1
    );
}

#[test]
fn open_with_other_through_the_system_chooser() {
    let mut d = single("hash");
    d.open_with = OpenWith::Chooser;
    d.handlers.open_with_chooser = true;
    let rows = tile(&d);
    let entry = find(&rows, "Open With Other...").unwrap();
    assert_eq!(entry.action, Action::OpenWithOther);
    assert_eq!(entry.shortcut, None);
    // The chooser is Windows' own; elsewhere the row stays disabled.
    assert_eq!(entry.disabled, !cfg!(target_os = "windows"));
}

#[test]
fn a_mixed_selection_only_moves_to_trash_and_is_not_dangerous() {
    let mut d = several(2);
    d.mixed = true;
    d.handlers.move_to_trash = true;
    let rows = tile(&d);
    assert_eq!(labels(&rows), ["Move to Trash"]);
    assert!(!find(&rows, "Move to Trash").unwrap().danger);
}

#[test]
fn inbox_accepts_and_rejects_several_items_first() {
    let mut d = several(3);
    d.status = Some(Status::Inbox);
    d.handlers.accept = true;
    d.handlers.reject = true;
    let rows = tile(&d);
    let shown = labels(&rows);
    assert_eq!(shown[..3], ["Accept 3 Items", "Reject 3 Items", "-"]);
    let accept = find(&rows, "Accept 3 Items").unwrap();
    assert_eq!(accept.action, Action::Accept);
    assert_eq!(accept.shortcut.as_deref(), Some("Z"));
    let reject = find(&rows, "Reject 3 Items").unwrap();
    assert_eq!(reject.action, Action::Reject);
    assert_eq!(reject.icon, Some("menu-trash"));
    assert_eq!(reject.shortcut.as_deref(), Some("X"));
}

#[test]
fn library_cover_for_media_of_any_type_but_not_groups() {
    let mut d = single("image-hash");
    d.single_kind = Some(Kind::Media);
    d.single_mime = Some("image/jpeg".into());
    d.handlers.library_cover = true;
    let rows = tile(&d);
    let entry = find(children(&rows, "More"), "Set as Library Cover").unwrap();
    assert_eq!(entry.icon, Some("menu-photo"));
    assert_eq!(entry.action, Action::SetLibraryCover);
    d.single_mime = Some("video/mp4".into());
    assert!(find(children(&tile(&d), "More"), "Set as Library Cover").is_some());
    d.single_kind = Some(Kind::Collection);
    d.contains_group = true;
    d.single_mime = Some("image/jpeg".into());
    let rows = tile(&d);
    assert!(!has_submenu(&rows, "More"));
}

#[test]
fn auto_tag_is_disabled_when_the_selection_cannot_be_tagged() {
    let mut d = several(12);
    d.query_selection = true;
    d.ai_tag_enabled = Some(false);
    d.handlers.ai_tagger = true;
    let entry = find(&tile(&d), "Auto Tag 12 Images").unwrap().clone();
    assert!(entry.disabled);
}

#[test]
fn tag_rows_use_the_bookmark_glyphs() {
    let mut d = single("hash");
    d.clipboard_tags = true;
    d.handlers.tag_select = true;
    d.handlers.ai_tagger = true;
    d.handlers.copy_tags = true;
    d.handlers.paste_tags = true;
    let rows = tile(&d);
    for (label, icon) in [
        ("Add Tags", "menu-bookmark"),
        ("Auto Tag", "menu-auto-tag"),
        ("Copy Tags", "menu-bookmarks"),
        ("Paste Tags", "menu-paste-tags"),
    ] {
        let entry = find(&rows, label).unwrap();
        assert_eq!(entry.icon, Some(icon));
        assert!(!entry.disabled, "{label}");
    }
    // Copying and pasting tags have deferred shortcuts, so none is shown.
    assert_eq!(find(&rows, "Copy Tags").unwrap().shortcut, None);
    assert_eq!(find(&rows, "Paste Tags").unwrap().shortcut, None);
    assert_eq!(
        find(&rows, "Add Tags").unwrap().shortcut.as_deref(),
        Some("T")
    );
    // Nothing copied yet: Paste Tags is shown disabled.
    d.clipboard_tags = false;
    assert!(find(&tile(&d), "Paste Tags").unwrap().disabled);
}

#[test]
fn adds_to_the_last_used_folder_by_name_without_a_shortcut() {
    let mut d = several(3);
    d.last_folder_name = Some("References".into());
    d.handlers.add_to_last_folder = true;
    let rows = tile(&d);
    let entry = find(&rows, "Add to \u{201c}References\u{201d}").unwrap();
    assert_eq!(entry.shortcut, None);
    assert_eq!(entry.action, Action::AddToLastFolder);
    // Without a folder used before, the row is not there.
    d.handlers.add_to_last_folder = false;
    assert!(
        labels(&tile(&d))
            .iter()
            .all(|label| !label.starts_with("Add to \u{201c}"))
    );
}

#[test]
fn several_items_copy_shared_tags() {
    let mut d = several(2);
    d.clipboard_tags = true;
    d.handlers.copy_tags = true;
    d.handlers.paste_tags = true;
    let shown = labels(&tile(&d));
    assert!(!shown.contains(&"Copy Tags".to_owned()));
    assert!(shown.contains(&"Copy Shared Tags".to_owned()));
    assert!(shown.contains(&"Paste Tags".to_owned()));
}

#[test]
fn batch_rename_for_listed_items() {
    let mut d = several(3);
    d.handlers.batch_rename = true;
    let rows = tile(&d);
    assert_eq!(
        find(&rows, "Batch Rename 3 Items...").unwrap().action,
        Action::BatchRename
    );
    d.query_selection = true;
    assert!(find(&tile(&d), "Batch Rename 3 Items...").is_none());
}

#[test]
fn a_selected_member_becomes_the_folder_cover() {
    let mut d = single("hash");
    d.scope = Some(ScopeKind::Folder);
    d.handlers.folder_cover = true;
    let rows = tile(&d);
    assert_eq!(
        find(children(&rows, "More"), "Set as Folder Cover")
            .unwrap()
            .action,
        Action::SetFolderCover
    );
}

#[test]
fn several_items_can_be_grouped() {
    let mut d = several(3);
    d.status = Some(Status::Active);
    d.handlers.organize_group = true;
    let rows = tile(&d);
    let entry = find(&rows, "Group...").unwrap();
    assert_eq!(entry.icon, Some("menu-group-create"));
    assert_eq!(entry.action, Action::Group);
}

#[test]
fn ungroup_counts_collections_and_grouping_names_merging() {
    let labels_for = |count: Option<usize>| {
        let mut d = several(5);
        d.status = Some(Status::Active);
        d.collection_count = Some(count);
        d.handlers.organize_group = true;
        d.handlers.ungroup = true;
        labels(&tile(&d))
    };
    let has = |shown: &[String], label: &str| shown.iter().any(|shown| shown == label);
    assert!(has(&labels_for(Some(3)), "Ungroup 3 Collections..."));
    assert!(has(&labels_for(Some(3)), "Merge into One Group..."));
    assert!(has(&labels_for(Some(1)), "Ungroup..."));
    assert!(has(&labels_for(Some(0)), "Group..."));
    assert!(
        labels_for(Some(0))
            .iter()
            .all(|label| !label.starts_with("Ungroup"))
    );
    // Still being counted.
    assert!(has(&labels_for(None), "Ungroup Collections..."));
    assert!(has(&labels_for(None), "Merge into One Group..."));
    assert!(has(&labels_for(Some(1234)), "Ungroup 1,234 Collections..."));
}

#[test]
fn a_group_is_a_library_item_not_its_cover_file() {
    let mut d = single("cover-hash");
    d.single_kind = Some(Kind::Collection);
    d.contains_group = true;
    d.status = Some(Status::Active);
    d.handlers = Handlers {
        open_new_window: true,
        edit_group: true,
        ungroup: true,
        open_default: true,
        reveal: true,
        regenerate: true,
        ..Default::default()
    };
    let rows = tile(&d);
    let shown = labels(&rows);
    let has = |label: &str| shown.iter().any(|shown| shown == label);
    assert!(!has("Open"));
    assert!(has("Open in New Window"));
    assert!(has("Edit Group"));
    assert!(has("Ungroup..."));
    assert!(!has("Open with Default App"));
    assert!(!has("Regenerate Thumbnail"));
    assert!(!has("Select All"));
    assert_eq!(
        find(&rows, "Edit Group").unwrap().icon,
        Some("menu-group-edit")
    );
    assert_eq!(
        find(&rows, "Ungroup...").unwrap().icon,
        Some("menu-group-remove")
    );
}

/// What the grid hands the builder for one picture in All, as the shell
/// does: every row production builds, in production's order.
#[test]
fn one_picture_in_all_has_every_row_in_order() {
    let d = Description {
        selection_count: 1,
        single: true,
        single_hash: Some("abcd".into()),
        single_item_id: Some(7),
        single_kind: Some(Kind::Media),
        single_name: Some("cat".into()),
        single_mime: Some("image/png".into()),
        scope: Some(ScopeKind::System),
        status: Some(Status::Active),
        ai_tag_enabled: Some(true),
        open_with: OpenWith::Chooser,
        collection_count: Some(Some(0)),
        handlers: Handlers {
            open_default: true,
            open_with_chooser: true,
            reveal: true,
            open_new_window: true,
            copy_file: true,
            copy_path: true,
            copy_name: true,
            copy_link: true,
            move_to_trash: true,
            add_to_folder: true,
            tag_select: true,
            ai_tagger: true,
            rename: true,
            regenerate: true,
            library_cover: true,
            copy_tags: true,
            paste_tags: true,
            new_folder_with_selection: true,
            find_media_matches: true,
            search_by_image: true,
            set_rating: true,
            export: true,
            export_originals: true,
            export_pack: true,
            toggle_grayscale: true,
            ungroup: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let rows = tile(&d);
    assert_eq!(
        labels(&rows),
        [
            "Open with Default App",
            "Open With Other...",
            REVEAL,
            "Open in New Window",
            "-",
            "[view panel]",
            "-",
            "Display >",
            "Grayscale",
            "-",
            "Add to Folder",
            "New Folder with Selection",
            "-",
            "Rename",
            "-",
            "Copy",
            "Copy File Path",
            "Copy Name",
            "Copy as Link",
            "-",
            "Add Tags",
            "Auto Tag",
            "Copy Tags",
            "Paste Tags",
            "-",
            "Set Rating >",
            "Export >",
            "-",
            "Search by Image >",
            "-",
            "More >",
            "-",
            "Move to Trash",
        ]
    );
    assert_eq!(
        labels(children(&rows, "More")),
        [
            "Set as Library Cover",
            "Find Items with This Media",
            "Regenerate Thumbnail"
        ]
    );
    assert_eq!(
        labels(children(&rows, "Set Rating")),
        ["No Rating", "★", "★★", "★★★", "★★★★", "★★★★★"]
    );
    assert_eq!(
        labels(children(&rows, "Export")),
        [
            "Export Originals...",
            "Export As...",
            "Export as Picto Pack..."
        ]
    );
    let shortcuts: Vec<_> = [
        "Open with Default App",
        REVEAL,
        "Open in New Window",
        "Grayscale",
        "Add to Folder",
        "Rename",
        "Copy",
        "Copy File Path",
        "Auto Tag",
        "Move to Trash",
    ]
    .iter()
    .map(|label| {
        find(&rows, label)
            .unwrap()
            .shortcut
            .clone()
            .unwrap_or_default()
    })
    .collect();
    let written: Vec<String> = [
        "Shift+Enter",
        "Mod+Enter",
        "Mod+O",
        "Mod+Alt+G",
        "Mod+Shift+J",
        "Ctrl+R",
        "Mod+C",
        "Mod+Alt+C",
        "Mod+Shift+A",
        "Mod+Backspace",
    ]
    .into_iter()
    .map(display)
    .collect();
    assert_eq!(shortcuts, written);
    // Paste Tags waits for something copied; Auto Tag for a feature this
    // app does not have yet, and Open With Other for the system's chooser,
    // which only Windows has.
    assert!(find(&rows, "Paste Tags").unwrap().disabled);
    assert!(
        rows.iter()
            .filter_map(|row| match row {
                Row::Item(item) => Some(item),
                _ => None,
            })
            .filter(|item| {
                !["Paste Tags", "Auto Tag", "Open With Other..."].contains(&item.label.as_str())
            })
            .all(|item| !item.disabled)
    );
}

#[test]
fn trash_restores_and_deletes_several_items() {
    let mut d = several(3);
    d.status = Some(Status::Trash);
    d.handlers.restore = true;
    d.handlers.permanent_delete = true;
    let rows = tile(&d);
    let shown = labels(&rows);
    assert_eq!(
        shown[shown.len() - 2..],
        ["Restore 3 Items", "Delete 3 Permanently"]
    );
    let delete = find(&rows, "Delete 3 Permanently").unwrap();
    assert!(delete.danger);
    assert_eq!(delete.shortcut, Some(display("Mod+Backspace").into()));
    assert_eq!(
        find(&rows, "Restore 3 Items").unwrap().shortcut,
        Some(display("Mod+Shift+Backspace").into())
    );
    // Without the delete wired, the row stays and is disabled.
    d.handlers.permanent_delete = false;
    assert!(find(&tile(&d), "Delete 3 Permanently").unwrap().disabled);
}

#[test]
fn the_viewer_puts_flash_playback_first() {
    let mut d = single("swf");
    d.surface = Surface::Viewer;
    d.single_kind = Some(Kind::Media);
    d.handlers.open_default = true;
    let rows = viewer_menu(
        &d,
        Playback {
            flash: Some((true, false)),
            capture: true,
        },
        &shortcut,
    );
    assert_eq!(
        labels(&rows)[..6],
        [
            "Pause",
            "Stop",
            "Mute",
            "Set as Thumbnail",
            "-",
            "Open with Default App"
        ]
    );
    let plain = viewer_menu(&d, Playback::default(), &shortcut);
    assert_eq!(labels(&plain)[0], "Open with Default App");
}

#[test]
fn the_preview_opens_the_file_or_makes_it_the_cover() {
    let mut d = single("hash");
    d.handlers.open_default = true;
    d.handlers.reveal = true;
    d.handlers.open_new_window = true;
    assert_eq!(
        labels(&preview_menu(&d, &shortcut)),
        [
            "Open with Default App",
            REVEAL,
            "Open in New Window",
            "Set as Library Cover"
        ]
    );
}

#[test]
fn keys_are_unique_across_the_actions_a_menu_offers() {
    let mut d = single("hash");
    d.single_kind = Some(Kind::Media);
    d.single_mime = Some("image/png".into());
    d.scope = Some(ScopeKind::Folder);
    d.handlers = Handlers {
        set_rating: true,
        search_by_image: true,
        sort_contents: true,
        export: true,
        export_pack: true,
        ..Default::default()
    };
    let actions = super::actions(&tile(&d));
    let mut keys: Vec<_> = actions.iter().map(|action| action.key()).collect();
    let total = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), total);
}

/// A collection member's menu, as `openMemberMenu` describes one member.
fn member(editor: bool, count: usize, mime: &str) -> Description {
    Description {
        surface: if editor {
            Surface::Grid
        } else {
            Surface::Viewer
        },
        selection_count: count,
        ai_tag_enabled: Some(mime.starts_with("image/")),
        single: count == 1,
        single_hash: (count == 1).then(|| "member".into()),
        single_item_id: (count == 1).then_some(7),
        single_kind: (count == 1).then_some(Kind::Media),
        single_name: (count == 1).then(|| "Chapter".into()),
        single_mime: (count == 1).then(|| mime.into()),
        handlers: member_handlers(count == 1, editor),
        sort_fields: Some(super::COLLECTION_SORT.to_vec()),
        ..Default::default()
    }
}

#[test]
fn a_member_in_the_editor_has_production_rows_in_order() {
    let rows = with_remove_from_group(tile(&member(true, 1, "application/pdf")), 1);
    assert_eq!(
        labels(&rows),
        [
            "Open with Default App",
            REVEAL,
            "Open in New Window",
            "-",
            "Sort by >",
            "-",
            "[view panel]",
            "-",
            "Display >",
            "Grayscale",
            "-",
            "Add to Folder",
            "-",
            "Copy",
            "Copy File Path",
            "Copy Name",
            "Copy as Link",
            "-",
            "Add Tags",
            "Auto Tag",
            "Copy Tags",
            "Paste Tags",
            "-",
            "Set Rating >",
            "Export >",
            "-",
            "More >",
            "-",
            "Remove from Group",
            "Move to Trash",
        ]
    );
    // A collection's contents sort by name or size only.
    assert_eq!(labels(children(&rows, "Sort by")), ["Name", "Size"]);
    assert_eq!(
        labels(children(&rows, "More")),
        [
            "Set as Library Cover",
            "Find Items with This Media",
            "Regenerate Thumbnail"
        ]
    );
    // Without its handler, Grayscale is drawn disabled.
    assert!(find(&rows, "Grayscale").unwrap().disabled);
}

#[test]
fn a_member_in_the_reader_has_no_view_rows() {
    let rows = with_remove_from_group(tile(&member(false, 1, "image/png")), 1);
    let labels = labels(&rows);
    assert!(!labels.iter().any(|label| label == "Sort by >"));
    assert!(!labels.iter().any(|label| label == "[view panel]"));
    assert_eq!(
        &labels[labels.len() - 3..],
        ["-", "Remove from Group", "Move to Trash"]
    );
    assert_eq!(
        find(&rows, "Remove from Group").unwrap().action,
        Action::RemoveFromGroup
    );
}

#[test]
fn several_members_are_removed_by_count() {
    let rows = with_remove_from_group(tile(&member(true, 3, "image/png")), 3);
    assert!(find(&rows, "Remove 3 from Group").is_some());
    assert!(find(&rows, "Move 3 to Trash").is_some());
    // With no Trash row, it goes last.
    assert_eq!(
        labels(&with_remove_from_group(Vec::new(), 1)),
        ["Remove from Group"]
    );
}

#[test]
fn the_space_between_members_sorts_or_ungroups() {
    let rows = collection_menu();
    assert_eq!(labels(&rows), ["Sort by >", "-", "Ungroup..."]);
    assert_eq!(labels(children(&rows, "Sort by")), ["Name", "Size"]);
    assert_eq!(find(&rows, "Ungroup...").unwrap().action, Action::Ungroup);
}
