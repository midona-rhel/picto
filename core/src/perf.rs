//! Lightweight in-process performance metrics snapshots.
//!
//! Keeps rolling latency samples for key endpoints so the frontend can
//! query current P50/P95/P99 without external tracing infrastructure.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

const WINDOW_SIZE: usize = 512;

#[derive(Debug, Clone, Serialize, Default)]
pub struct PercentileSnapshot {
    pub count: u64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MetadataBatchLatest {
    pub total_ms: f64,
    pub local_ms: f64,
    pub ptr_ms: f64,
    pub merge_ms: f64,
    pub req_hashes: usize,
    pub local_hits: usize,
    pub ptr_lookup: usize,
    pub ptr_hits: usize,
    pub missing: usize,
    pub ts: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PerfSnapshot {
    pub grid_page_slim: PercentileSnapshot,
    pub files_metadata_batch: PercentileSnapshot,
    pub sidebar_tree: PercentileSnapshot,
    pub selection_summary: PercentileSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_batch_latest: Option<MetadataBatchLatest>,
    pub projection_corruption_count: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SloStat {
    pub available: bool,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub target_p50_ms: f64,
    pub target_p95_ms: f64,
    pub target_p99_ms: f64,
    pub pass_p50: bool,
    pub pass_p95: bool,
    pub pass_p99: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SloCheckResult {
    pub pass: bool,
    pub click_metadata: SloStat,
    pub grid_first_page: SloStat,
    pub sidebar_tree: SloStat,
    pub selection_summary: SloStat,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_metrics: Vec<String>,
}

#[derive(Default)]
struct LatencyWindow {
    samples: VecDeque<f64>,
    total_count: u64,
    sum_ms: f64,
    max_ms: f64,
}

impl LatencyWindow {
    fn record(&mut self, ms: f64) {
        self.total_count = self.total_count.saturating_add(1);
        self.sum_ms += ms;
        self.max_ms = self.max_ms.max(ms);
        if self.samples.len() >= WINDOW_SIZE {
            self.samples.pop_front();
        }
        self.samples.push_back(ms);
    }

    fn snapshot(&self) -> PercentileSnapshot {
        if self.samples.is_empty() {
            return PercentileSnapshot {
                count: self.total_count,
                max_ms: self.max_ms,
                ..Default::default()
            };
        }

        let mut sorted: Vec<f64> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let pick = |q: f64| -> f64 {
            let idx = ((sorted.len() as f64 - 1.0) * q).round() as usize;
            sorted[idx.min(sorted.len().saturating_sub(1))]
        };

        PercentileSnapshot {
            count: self.total_count,
            p50_ms: pick(0.50),
            p95_ms: pick(0.95),
            p99_ms: pick(0.99),
            max_ms: self.max_ms,
            avg_ms: if self.total_count == 0 {
                0.0
            } else {
                self.sum_ms / self.total_count as f64
            },
        }
    }
}

#[derive(Default)]
struct PerfState {
    grid_page_slim: LatencyWindow,
    files_metadata_batch: LatencyWindow,
    sidebar_tree: LatencyWindow,
    selection_summary: LatencyWindow,
    metadata_batch_latest: Option<MetadataBatchLatest>,
    projection_corruption_count: u64,
}

static PERF_STATE: OnceLock<Mutex<PerfState>> = OnceLock::new();

fn state() -> &'static Mutex<PerfState> {
    PERF_STATE.get_or_init(|| Mutex::new(PerfState::default()))
}

pub fn record_grid_page_slim(ms: f64) {
    if let Ok(mut g) = state().lock() {
        g.grid_page_slim.record(ms);
    }
}

pub fn record_files_metadata_batch(
    total_ms: f64,
    local_ms: f64,
    ptr_ms: f64,
    merge_ms: f64,
    req_hashes: usize,
    local_hits: usize,
    ptr_lookup: usize,
    ptr_hits: usize,
    missing: usize,
) {
    if let Ok(mut g) = state().lock() {
        g.files_metadata_batch.record(total_ms);
        g.metadata_batch_latest = Some(MetadataBatchLatest {
            total_ms,
            local_ms,
            ptr_ms,
            merge_ms,
            req_hashes,
            local_hits,
            ptr_lookup,
            ptr_hits,
            missing,
            ts: chrono::Utc::now().to_rfc3339(),
        });
    }
}

pub fn record_sidebar_tree(ms: f64) {
    if let Ok(mut g) = state().lock() {
        g.sidebar_tree.record(ms);
    }
}

pub fn record_selection_summary(ms: f64) {
    if let Ok(mut g) = state().lock() {
        g.selection_summary.record(ms);
    }
}

/// Increment the projection corruption counter and return new total.
pub fn record_projection_corruption(count: u64) -> u64 {
    if let Ok(mut g) = state().lock() {
        g.projection_corruption_count += count;
        g.projection_corruption_count
    } else {
        0
    }
}

/// Get the current projection corruption count.
pub fn get_projection_corruption_count() -> u64 {
    if let Ok(g) = state().lock() {
        g.projection_corruption_count
    } else {
        0
    }
}

pub fn get_snapshot() -> PerfSnapshot {
    if let Ok(g) = state().lock() {
        PerfSnapshot {
            grid_page_slim: g.grid_page_slim.snapshot(),
            files_metadata_batch: g.files_metadata_batch.snapshot(),
            sidebar_tree: g.sidebar_tree.snapshot(),
            selection_summary: g.selection_summary.snapshot(),
            metadata_batch_latest: g.metadata_batch_latest.clone(),
            projection_corruption_count: g.projection_corruption_count,
        }
    } else {
        PerfSnapshot::default()
    }
}

fn evaluate_latency(
    snap: &PercentileSnapshot,
    target_p50_ms: f64,
    target_p95_ms: f64,
    target_p99_ms: f64,
) -> SloStat {
    if snap.count == 0 {
        return SloStat {
            available: false,
            target_p50_ms,
            target_p95_ms,
            target_p99_ms,
            ..Default::default()
        };
    }
    let pass_p50 = snap.p50_ms <= target_p50_ms;
    let pass_p95 = snap.p95_ms <= target_p95_ms;
    let pass_p99 = snap.p99_ms <= target_p99_ms;
    SloStat {
        available: true,
        p50_ms: snap.p50_ms,
        p95_ms: snap.p95_ms,
        p99_ms: snap.p99_ms,
        target_p50_ms,
        target_p95_ms,
        target_p99_ms,
        pass_p50,
        pass_p95,
        pass_p99,
    }
}

pub fn check_default_slo() -> SloCheckResult {
    let snap = get_snapshot();

    // Aggressive SLOs from V2 plan.
    let click_metadata = evaluate_latency(&snap.files_metadata_batch, 40.0, 80.0, 120.0);
    let grid_first_page = evaluate_latency(&snap.grid_page_slim, 60.0, 100.0, 150.0);
    // Sidebar target is p95<=80ms; keep p50/p99 targets aligned to avoid partial checks.
    let sidebar_tree = evaluate_latency(&snap.sidebar_tree, 60.0, 80.0, 120.0);
    // Selection summary <=10k files: p95<=120ms.
    let selection_summary = evaluate_latency(&snap.selection_summary, 60.0, 120.0, 200.0);

    let mut missing_metrics = Vec::new();
    if !click_metadata.available {
        missing_metrics.push("click_metadata".to_string());
    }
    if !grid_first_page.available {
        missing_metrics.push("grid_first_page".to_string());
    }
    if !sidebar_tree.available {
        missing_metrics.push("sidebar_tree".to_string());
    }
    // selection_summary is demand-driven — not listed as missing since it only
    // records samples when the user makes a selection.

    // Core metrics must be available and within targets.
    // selection_summary is demand-driven (only recorded when user selects),
    // so it must not block the SLO at startup — only fail if it IS available
    // and exceeds targets.
    let core_pass = click_metadata.available
        && grid_first_page.available
        && sidebar_tree.available
        && click_metadata.pass_p50
        && click_metadata.pass_p95
        && click_metadata.pass_p99
        && grid_first_page.pass_p50
        && grid_first_page.pass_p95
        && grid_first_page.pass_p99
        && sidebar_tree.pass_p50
        && sidebar_tree.pass_p95
        && sidebar_tree.pass_p99;

    let selection_pass = !selection_summary.available
        || (selection_summary.pass_p50 && selection_summary.pass_p95 && selection_summary.pass_p99);

    let pass = core_pass && selection_pass;

    SloCheckResult {
        pass,
        click_metadata,
        grid_first_page,
        sidebar_tree,
        selection_summary,
        missing_metrics,
    }
}
