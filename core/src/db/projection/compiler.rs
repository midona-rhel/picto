//! Event-driven compiler loop.
//! Receives structured change signals from write operations and
//! schedules the appropriate projection rebuilds.

use rusqlite::Connection;

use super::bitmaps::BitmapStore;

/// What needs to be rebuilt after a batch of changes.
#[derive(Debug, Default)]
pub struct CompilerPlan {
    pub rebuild_status: bool,
    pub rebuild_all_tags: bool,
    pub dirty_tag_ids: Vec<i64>,
    pub rebuild_tag_graph: bool,
    pub rebuild_all_smart_folders: bool,
    pub dirty_smart_folder_ids: Vec<i64>,
    pub rebuild_sidebar: bool,
    pub rebuild_all: bool,
}

impl CompilerPlan {
    pub fn is_empty(&self) -> bool {
        !self.rebuild_status
            && !self.rebuild_all_tags
            && self.dirty_tag_ids.is_empty()
            && !self.rebuild_tag_graph
            && !self.rebuild_all_smart_folders
            && self.dirty_smart_folder_ids.is_empty()
            && !self.rebuild_sidebar
            && !self.rebuild_all
    }
}

/// Execute a compiler plan against the database and bitmap store.
pub fn execute_plan(conn: &Connection, bitmaps: &BitmapStore, plan: &CompilerPlan) {
    if plan.rebuild_all || plan.rebuild_status {
        super::tags::compile_status_bitmaps(conn, bitmaps);
    }

    if plan.rebuild_all || plan.rebuild_all_tags {
        super::tags::compile_all_tag_bitmaps(conn, bitmaps);
    } else {
        for tag_id in &plan.dirty_tag_ids {
            super::tags::compile_tag_bitmap(conn, bitmaps, *tag_id);
        }
    }

    if plan.rebuild_all || plan.rebuild_tag_graph {
        super::tags::compile_implied_tags(conn, bitmaps);
    }

    if plan.rebuild_all || plan.rebuild_all_tags || plan.rebuild_tag_graph || plan.rebuild_status {
        super::tags::compile_tagged_bitmap(conn, bitmaps);
    }

    if plan.rebuild_all || plan.rebuild_all_smart_folders {
        super::smart_folders::compile_all_smart_folders(conn, bitmaps);
    } else {
        for sf_id in &plan.dirty_smart_folder_ids {
            super::smart_folders::compile_smart_folder(conn, bitmaps, *sf_id);
        }
    }

    if plan.rebuild_all || plan.rebuild_sidebar {
        super::sidebar::compile_sidebar(conn);
    }
}

/// Full rebuild of all projections from authoritative data.
pub fn full_rebuild(conn: &Connection, bitmaps: &BitmapStore) {
    execute_plan(conn, bitmaps, &CompilerPlan {
        rebuild_all: true,
        rebuild_sidebar: true,
        ..Default::default()
    });
}
