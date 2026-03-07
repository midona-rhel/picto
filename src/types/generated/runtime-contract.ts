// Runtime contract types — mirrors core/src/runtime_contract/*.rs
// Manually maintained. Keep in sync with the Rust structs.

// ─── Mutation ────────────────────────────────────────────────────────────────

export interface MutationReceipt {
  seq: number;
  ts: string;
  origin_command: string;
  facts: MutationFacts;
  invalidate: DerivedInvalidation;
  sidebar_counts?: SidebarCounts;
}

export interface MutationFacts {
  domains: string[];
  file_hashes?: string[];
  folder_ids?: number[];
  smart_folder_ids?: number[];
  compiler_batch_done?: boolean;
}

export interface DerivedInvalidation {
  sidebar_tree?: boolean;
  grid_scopes?: string[];
  selection_summary?: boolean;
  metadata_hashes?: string[];
  view_prefs?: boolean;
}

export interface SidebarCounts {
  all_images: number;
  inbox: number;
  trash: number;
}

// ─── Tasks ───────────────────────────────────────────────────────────────────

export interface RuntimeTask {
  task_id: string;
  kind: TaskKind;
  status: TaskStatus;
  label: string;
  parent_task_id?: string;
  progress?: TaskProgress;
  detail?: unknown;
  started_at: string;
  updated_at: string;
}

export type TaskKind =
  | 'subscription'
  | 'flow'
  | 'ptr_sync'
  | 'ptr_bootstrap'
  | 'import';

export type TaskStatus =
  | 'running'
  | 'cancelling'
  | 'finished'
  | 'failed';

export interface TaskProgress {
  done: number;
  total: number;
  status_text?: string;
}

// ─── Snapshot ────────────────────────────────────────────────────────────────

export interface RuntimeSnapshot {
  seq: number;
  ts: string;
  tasks: RuntimeTask[];
}

// ─── Resource Keys ──────────────────────────────────────────────────────────

export type ResourceKey =
  | 'sidebar/tree'
  | 'sidebar/counts'
  | `grid/${string}`
  | `metadata/hash:${string}`
  | 'selection/current'
  | 'view-prefs/current';
