//! Lightweight in-process performance metrics snapshots.
//!
//! Keeps rolling latency samples for key endpoints so the frontend can
//! query current P50/P95/P99 without external tracing infrastructure.
//! Only metrics with a live record site belong here — the SLO gate must
//! never depend on a metric nothing records.

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
pub struct PerfSnapshot {
    pub sidebar_tree: PercentileSnapshot,
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
    pub sidebar_tree: SloStat,
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
    sidebar_tree: LatencyWindow,
}

static PERF_STATE: OnceLock<Mutex<PerfState>> = OnceLock::new();

fn state() -> &'static Mutex<PerfState> {
    PERF_STATE.get_or_init(|| Mutex::new(PerfState::default()))
}

pub fn record_sidebar_tree(ms: f64) {
    if let Ok(mut g) = state().lock() {
        g.sidebar_tree.record(ms);
    }
}

pub fn get_snapshot() -> PerfSnapshot {
    if let Ok(g) = state().lock() {
        PerfSnapshot {
            sidebar_tree: g.sidebar_tree.snapshot(),
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
    SloStat {
        available: true,
        p50_ms: snap.p50_ms,
        p95_ms: snap.p95_ms,
        p99_ms: snap.p99_ms,
        target_p50_ms,
        target_p95_ms,
        target_p99_ms,
        pass_p50: snap.p50_ms <= target_p50_ms,
        pass_p95: snap.p95_ms <= target_p95_ms,
        pass_p99: snap.p99_ms <= target_p99_ms,
    }
}

pub fn check_default_slo() -> SloCheckResult {
    let snap = get_snapshot();

    // Sidebar target is p95<=80ms; keep p50/p99 targets aligned to avoid partial checks.
    let sidebar_tree = evaluate_latency(&snap.sidebar_tree, 60.0, 80.0, 120.0);

    let mut missing_metrics = Vec::new();
    if !sidebar_tree.available {
        missing_metrics.push("sidebar_tree".to_string());
    }

    let pass = sidebar_tree.available
        && sidebar_tree.pass_p50
        && sidebar_tree.pass_p95
        && sidebar_tree.pass_p99;

    SloCheckResult {
        pass,
        sidebar_tree,
        missing_metrics,
    }
}
