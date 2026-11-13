//! The library's undo history in the user's words, as production's
//! `historyMessages.ts` and `historyRuntime.ts` show it: "Undone — Moved 3
//! items to Trash" after an undo, and a notification with an Undo button
//! after a broad or destructive change.
use crate::grid::format::grouped;
use crate::session::Session;
use crate::ui::notify;
use gpui::{App, Entity};
use picto_library::Library;
use picto_library::history::HistoryEntrySummary;

/// `ANNOUNCED_MUTATIONS`: routine edits are visible where they happen; these
/// broad or destructive ones say what they did and offer Undo.
pub const ANNOUNCED: &[&str] = &[
    "collections.detach",
    "collections.organize",
    "collections.ungroup",
    "folders.delete",
    "items.apply_ai_tags",
    "items.delete",
    "items.reject",
    "items.rename_many",
    "smart_folders.delete",
    "tags.delete",
    "tags.delete_unused",
    "tags.group.delete",
];

/// `describeHistoryEntry`: the change in words, or the library's own label
/// for a command not worded here.
pub fn describe(entry: &HistoryEntrySummary) -> String {
    let count = entry.count;
    let n = || grouped(count as i128);
    let one = count == 1;
    let detail = entry.detail.as_deref();
    let pick = |single: &str, many: String| if one { single.to_owned() } else { many };
    match entry.command.as_str() {
        "items.set_lifecycle" => match detail {
            Some("trash") => pick(
                "Moved an item to Trash",
                format!("Moved {} items to Trash", n()),
            ),
            Some("inbox") => pick(
                "Moved an item to the Inbox",
                format!("Moved {} items to the Inbox", n()),
            ),
            _ => pick(
                "Moved an item into the library",
                format!("Moved {} items into the library", n()),
            ),
        },
        "items.set_rating" => rating(count, detail),
        "items.apply_tags" => {
            if detail == Some("remove") {
                pick(
                    "Removed tags from an item",
                    format!("Removed tags from {} items", n()),
                )
            } else {
                pick(
                    "Added tags to an item",
                    format!("Added tags to {} items", n()),
                )
            }
        }
        "items.apply_ai_tags" => "Added AI tags".into(),
        "items.delete" => pick(
            "Deleted an item permanently",
            format!("Deleted {} items permanently", n()),
        ),
        "items.reject" => pick("Rejected an item", format!("Rejected {} items", n())),
        "items.set_folder" => {
            if detail == Some("remove") {
                pick(
                    "Removed an item from a folder",
                    format!("Removed {} items from a folder", n()),
                )
            } else {
                pick(
                    "Added an item to a folder",
                    format!("Added {} items to a folder", n()),
                )
            }
        }
        "items.patch_metadata" => match detail {
            Some(detail) if detail.starts_with("rating:") => rating(count, Some(&detail[7..])),
            Some("notes") => "Changed notes".into(),
            Some("source_urls") => "Changed source links".into(),
            _ => pick(
                "Changed the details of an item",
                format!("Changed the details of {} items", n()),
            ),
        },
        "items.set_notes" => "Changed notes".into(),
        "items.set_source_urls" => "Changed source links".into(),
        "items.rename" => "Renamed an item".into(),
        "items.rename_many" => pick("Renamed an item", format!("Renamed {} items", n())),
        "items.clear_recent_views" => "Cleared recently viewed".into(),
        "collections.organize" => pick(
            "Made a collection",
            format!("Grouped {} items into a collection", n()),
        ),
        "collections.ungroup" => {
            if count > 1 {
                format!("Ungrouped {} collections", n())
            } else {
                "Ungrouped a collection".into()
            }
        }
        "collections.detach" => pick(
            "Removed an item from a collection",
            format!("Removed {} items from a collection", n()),
        ),
        "collections.reorder" => "Reordered a collection".into(),
        "collections.set_cover" => "Changed a collection’s cover".into(),
        "duplicates.resolve" => "Resolved a duplicate".into(),
        "folders.create" => "Created a folder".into(),
        "folders.duplicate" => "Duplicated a folder".into(),
        "folders.set_metadata" => "Edited a folder".into(),
        "folders.set_cover" => "Changed a folder’s cover".into(),
        "folders.set_watch" => "Changed folder watching".into(),
        "folders.clear_watch" => "Stopped watching a folder".into(),
        "folders.rename" => "Renamed a folder".into(),
        "folders.move" => "Moved a folder".into(),
        "folders.reorder" => "Reordered folders".into(),
        "folders.sort_tree" => "Sorted folders".into(),
        "folders.sort_items" => "Reordered items in a folder".into(),
        "folders.set_auto_tags" => "Changed a folder’s automatic tags".into(),
        "folders.delete" => pick("Deleted a folder", format!("Deleted {} folders", n())),
        "smart_folders.create" => "Created a smart folder".into(),
        "smart_folders.update" => "Edited a smart folder".into(),
        "smart_folders.delete" => pick(
            "Deleted a smart folder",
            format!("Deleted {} smart folders", n()),
        ),
        "smart_folders.reorder" => "Reordered smart folders".into(),
        "tags.rename_or_merge" => {
            if detail == Some("merge") {
                "Merged tags".into()
            } else {
                "Renamed a tag".into()
            }
        }
        "tags.delete" => "Deleted a tag".into(),
        "tags.delete_unused" => "Deleted unused tags".into(),
        "tags.group.create" => "Created a tag group".into(),
        "tags.group.rename" => {
            if detail == Some("merge") {
                "Merged tag groups".into()
            } else {
                "Renamed a tag group".into()
            }
        }
        _ => entry.label.clone(),
    }
}

/// `items.set_rating`'s words, `stars` from the entry's detail.
fn rating(count: u64, stars: Option<&str>) -> String {
    let stars = stars
        .and_then(|stars| stars.parse::<u32>().ok())
        .unwrap_or(0);
    let n = grouped(count as i128);
    match (count == 1, stars) {
        (true, 0) => "Cleared the rating of an item".into(),
        (false, 0) => format!("Cleared the rating of {n} items"),
        (true, 1) => "Rated an item 1 star".into(),
        (false, 1) => format!("Rated {n} items 1 star"),
        (true, stars) => format!("Rated an item {stars} stars"),
        (false, stars) => format!("Rated {n} items {stars} stars"),
    }
}

/// The newest undo entry's id, read before a change, so that
/// [`announced`] does not mistake an older entry for the change's.
pub fn newest(library: &Library) -> Option<u64> {
    library.history().state().undo.map(|entry| entry.entry_id)
}

/// `announceUndoableMutation`: the entry a change of `command` recorded, if
/// it is one that is announced and it recorded one after `before`.
pub fn announced(
    library: &Library,
    command: &str,
    before: Option<u64>,
) -> Option<HistoryEntrySummary> {
    if !ANNOUNCED.contains(&command) {
        return None;
    }
    let entry = library.history().state().undo?;
    (entry.command == command && Some(entry.entry_id) != before).then_some(entry)
}

/// Shows an announced change, with an Undo button.
pub fn announce(entry: &HistoryEntrySummary, session: Entity<Session>, cx: &mut App) {
    notify::show(
        notify::Tone::Success,
        notify::Options::new(describe(entry), "").action("Undo", move |_, cx| {
            session.update(cx, |session, cx| session.step_history_entry(true, cx))
        }),
        cx,
    );
}

/// What an undo or redo did: the entry it replayed, and whether the
/// opposite step is now possible.
pub struct Stepped {
    pub entry: HistoryEntrySummary,
    pub reversible: bool,
}

/// `history.undo` / `history.redo`: replays the newest entry, or says there
/// is nothing to replay, in the application's words.
pub fn step(library: &Library, undo: bool) -> Result<Stepped, String> {
    let nothing = || {
        if undo {
            "There is nothing to undo".to_owned()
        } else {
            "There is nothing to redo".to_owned()
        }
    };
    let state = library.history().state();
    let entry = if undo { state.undo } else { state.redo }.ok_or_else(nothing)?;
    let replayed = if undo { library.undo() } else { library.redo() };
    replayed
        .map_err(|error| error.to_string())?
        .ok_or_else(nothing)?;
    let state = library.history().state();
    Ok(Stepped {
        entry,
        reversible: if undo {
            state.redo.is_some()
        } else {
            state.undo.is_some()
        },
    })
}

/// `showHistoryResult`: "Undone" or "Redone" and what, with a button for
/// the opposite step while there is one.
pub fn show_step(stepped: &Stepped, undo: bool, session: Entity<Session>, cx: &mut App) {
    let mut options = notify::Options::new(
        if undo { "Undone" } else { "Redone" },
        describe(&stepped.entry),
    );
    if stepped.reversible {
        options = options.action(if undo { "Redo" } else { "Undo" }, move |_, cx| {
            session.update(cx, |session, cx| session.step_history_entry(!undo, cx))
        });
    }
    notify::show(notify::Tone::Info, options, cx);
}

#[cfg(test)]
mod tests {
    use super::{HistoryEntrySummary, describe};

    fn entry(command: &str, count: u64, detail: Option<&str>) -> HistoryEntrySummary {
        HistoryEntrySummary {
            entry_id: 1,
            command: command.into(),
            label: "Fallback".into(),
            count,
            detail: detail.map(str::to_owned),
        }
    }

    #[test]
    fn changes_are_described_as_production_words_them() {
        let say = |command, count, detail| describe(&entry(command, count, detail));
        assert_eq!(
            say("items.set_lifecycle", 3, Some("trash")),
            "Moved 3 items to Trash"
        );
        assert_eq!(
            say("items.set_lifecycle", 1, Some("inbox")),
            "Moved an item to the Inbox"
        );
        assert_eq!(
            say("items.set_lifecycle", 1200, Some("active")),
            "Moved 1,200 items into the library"
        );
        assert_eq!(
            say("items.set_rating", 2, Some("1")),
            "Rated 2 items 1 star"
        );
        assert_eq!(
            say("items.set_rating", 1, Some("4")),
            "Rated an item 4 stars"
        );
        assert_eq!(
            say("items.set_rating", 5, Some("0")),
            "Cleared the rating of 5 items"
        );
        assert_eq!(
            say("items.patch_metadata", 1, Some("rating:3")),
            "Rated an item 3 stars"
        );
        assert_eq!(say("folders.delete", 2, None), "Deleted 2 folders");
        assert_eq!(say("folders.delete", 1, None), "Deleted a folder");
        assert_eq!(say("items.reject", 1, None), "Rejected an item");
        assert_eq!(
            say("collections.ungroup", 1, None),
            "Ungrouped a collection"
        );
        assert_eq!(say("tags.rename_or_merge", 0, Some("merge")), "Merged tags");
        assert_eq!(say("library.change", 0, None), "Fallback");
    }
}
