//! What a merge of another library into this one needs from the library
//! besides the ordinary imports of a task: the stored input that marks a task
//! as a merge, the backup of the database that is made before the merge
//! starts, and the translation of the other library's smart folder rules to
//! this library's tags and folders.
//!
//! The merge itself is a foreground task (`tasks.rs`). Its items are imported
//! by `Library::ingest_batch_for_task` and
//! `Library::ingest_collection_for_task`, its tags, folders and smart folders
//! are created by `ensure_tag_definitions_for_task`,
//! `create_folder_like_for_task` and `create_smart_folder_for_task`, and
//! undoing the task takes all of it back.

use std::path::PathBuf;

use super::*;
use crate::predicate::{FilterClause, FilterExpr, ViewQuerySpec};
use crate::TagId;

/// The key in a task's stored input whose value is the folder of the library
/// that the task merges into this one. A task with this key is a merge.
pub const MERGE_LIBRARY: &str = "merge_library";
/// The key in a task's stored input whose value is the path of the backup of
/// this library's database that was made before the merge started.
pub const MERGE_BACKUP: &str = "merge_backup";

/// What the stored input of a merge task names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeTaskInput {
    /// The folder of the library that is merged into this one.
    pub library: PathBuf,
    /// The backup of this library's database from before the merge.
    pub backup: PathBuf,
}

impl MergeTaskInput {
    /// Returns the text that is stored as the task's input.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            MERGE_LIBRARY: self.library.to_string_lossy(),
            MERGE_BACKUP: self.backup.to_string_lossy(),
        })
        .to_string()
    }

    /// Reads a task's stored input. Returns `None` for a task that is not a
    /// merge.
    pub fn from_json(input_json: &str) -> Option<Self> {
        let value = serde_json::from_str::<serde_json::Value>(input_json).ok()?;
        Some(Self {
            library: PathBuf::from(value.get(MERGE_LIBRARY)?.as_str()?),
            backup: PathBuf::from(value.get(MERGE_BACKUP)?.as_str()?),
        })
    }
}

impl Library {
    /// Writes a backup of the database as it is committed now, in the way
    /// the schema migration does (`schema::backup`), and returns its path:
    /// `library.sqlite.<label>-<milliseconds since 1970>.backup` in the
    /// library's folder.
    pub fn backup_database(&self, label: &str) -> Result<PathBuf> {
        let path = crate::schema::backup_path(self.database.path(), label);
        self.database.read(WorkPriority::Maintenance, |connection| {
            crate::schema::backup(connection, &path)
        })?;
        Ok(path)
    }

    /// Returns every task that merges another library, whatever its state:
    /// its id, its state, what its stored input names, and whether one of
    /// its files could not be imported. The query reads the task table and,
    /// for each merge, at most one row of the index of its jobs.
    pub fn merge_tasks(&self) -> Result<Vec<(crate::TaskId, crate::TaskState, MergeTaskInput, bool)>> {
        self.database.read(WorkPriority::CanonicalIngest, |connection| {
            let mut statement = connection.prepare_cached(
                "SELECT t.task_id, t.state, t.input_json,
                        EXISTS(SELECT 1 FROM ingest_job j
                               WHERE j.task_id = t.task_id AND j.status = 'failed')
                 FROM task t
                 WHERE instr(t.input_json, ?1) > 0
                 ORDER BY t.task_id",
            )?;
            let rows = statement
                .query_map([MERGE_LIBRARY], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, bool>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .filter_map(|(task_id, state, input, failed)| {
                    Some((
                        crate::TaskId(task_id),
                        crate::TaskState::parse(&state)?,
                        MergeTaskInput::from_json(&input)?,
                        failed,
                    ))
                })
                .collect())
        })
    }

    /// Marks a merge for undo that did not bring everything: one that was
    /// stopped, and one with a file that could not be imported, whether it
    /// is still running or has already finished. The jobs that are still
    /// queued are dropped. `error` is stored as the reason, and without it
    /// the reason that the task already has is kept. The ingest worker
    /// undoes the task next (`revert_task`).
    pub fn undo_incomplete_merge(
        &self,
        task: crate::TaskId,
        error: Option<&str>,
        now_ms: i64,
    ) -> Result<()> {
        self.auxiliary_write(
            WorkPriority::CanonicalIngest,
            ["tasks".to_owned(), "ingest".to_owned()],
            [],
            |transaction, _| {
                transaction.execute(
                    "DELETE FROM ingest_job WHERE task_id = ?1 AND status = 'pending'",
                    [task.0],
                )?;
                transaction.execute(
                    "UPDATE task SET state = 'reverting', scan_complete = 1,
                         error = COALESCE(?2, error),
                         finished_at_ms = COALESCE(finished_at_ms, ?3)
                     WHERE task_id = ?1
                       AND state IN ('running', 'paused', 'finished', 'stopped')",
                    rusqlite::params![task.0, error, now_ms],
                )?;
                Ok(())
            },
        )?;
        Ok(())
    }
}

/// Adds the ids of the tags that the rules `expression` name to `tags`.
pub(super) fn tag_references(expression: &FilterExpr, tags: &mut RoaringBitmap) {
    match expression {
        FilterExpr::All(children) | FilterExpr::Any(children) => {
            for child in children {
                tag_references(child, tags);
            }
        }
        FilterExpr::Not(child) => tag_references(child, tags),
        FilterExpr::Clause(FilterClause::Tags { tag_ids, .. }) => {
            tags.extend(tag_ids.iter().map(|tag_id| tag_id.0));
        }
        FilterExpr::Clause(_) => {}
    }
}

/// Returns the view of another library's smart folder with every tag id and
/// folder id replaced by the id that `tag` and `folder` give for it, which is
/// the id of the same tag or folder in this library. Returns `None` when the
/// rules name a tag or a folder that has no id in this library.
pub fn translate_view(
    view: &ViewQuerySpec,
    tag: &dyn Fn(TagId) -> Option<TagId>,
    folder: &dyn Fn(FolderId) -> Option<FolderId>,
) -> Option<ViewQuerySpec> {
    Some(ViewQuerySpec {
        filter: translate_expression(&view.filter, tag, folder)?,
        sort: view.sort.clone(),
    })
}

fn translate_expression(
    expression: &FilterExpr,
    tag: &dyn Fn(TagId) -> Option<TagId>,
    folder: &dyn Fn(FolderId) -> Option<FolderId>,
) -> Option<FilterExpr> {
    Some(match expression {
        FilterExpr::All(children) => FilterExpr::All(
            children
                .iter()
                .map(|child| translate_expression(child, tag, folder))
                .collect::<Option<_>>()?,
        ),
        FilterExpr::Any(children) => FilterExpr::Any(
            children
                .iter()
                .map(|child| translate_expression(child, tag, folder))
                .collect::<Option<_>>()?,
        ),
        FilterExpr::Not(child) => {
            FilterExpr::Not(Box::new(translate_expression(child, tag, folder)?))
        }
        FilterExpr::Clause(FilterClause::Tags { tag_ids, mode }) => {
            let mut tag_ids = tag_ids
                .iter()
                .map(|tag_id| tag(*tag_id))
                .collect::<Option<Vec<_>>>()?;
            tag_ids.sort_unstable();
            tag_ids.dedup();
            FilterExpr::Clause(FilterClause::Tags {
                tag_ids,
                mode: *mode,
            })
        }
        FilterExpr::Clause(FilterClause::Folders { folder_ids, mode }) => {
            let mut folder_ids = folder_ids
                .iter()
                .map(|folder_id| folder(*folder_id))
                .collect::<Option<Vec<_>>>()?;
            folder_ids.sort_unstable();
            folder_ids.dedup();
            FilterExpr::Clause(FilterClause::Folders {
                folder_ids,
                mode: *mode,
            })
        }
        FilterExpr::Clause(clause) => FilterExpr::Clause(clause.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predicate::SetMatchMode;

    #[test]
    fn a_view_is_translated_to_the_ids_of_this_library_or_not_at_all() {
        let view = ViewQuerySpec {
            filter: FilterExpr::All(vec![
                FilterExpr::Clause(FilterClause::Tags {
                    tag_ids: vec![TagId(7), TagId(3)],
                    mode: SetMatchMode::All,
                }),
                FilterExpr::Not(Box::new(FilterExpr::Clause(FilterClause::Folders {
                    folder_ids: vec![FolderId(5)],
                    mode: SetMatchMode::Any,
                }))),
            ]),
            sort: Default::default(),
        };
        let translated = translate_view(
            &view,
            &|tag| Some(TagId(tag.0 + 100)),
            &|folder| Some(FolderId(folder.0 + 200)),
        )
        .unwrap();
        assert_eq!(
            translated.filter,
            FilterExpr::All(vec![
                FilterExpr::Clause(FilterClause::Tags {
                    tag_ids: vec![TagId(103), TagId(107)],
                    mode: SetMatchMode::All,
                }),
                FilterExpr::Not(Box::new(FilterExpr::Clause(FilterClause::Folders {
                    folder_ids: vec![FolderId(205)],
                    mode: SetMatchMode::Any,
                }))),
            ])
        );
        assert_eq!(
            translate_view(&view, &|_| None, &|folder| Some(folder)),
            None
        );
        let mut tags = RoaringBitmap::new();
        tag_references(&view.filter, &mut tags);
        assert_eq!(tags.into_iter().collect::<Vec<_>>(), [3, 7]);
    }
}
