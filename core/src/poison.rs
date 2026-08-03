//! Helpers that recover from poisoned std::sync locks instead of panicking.
//! A poisoned lock means a thread panicked while holding it. These helpers
//! log a warning and return the inner data, preventing crash cascades.

use std::sync::{Mutex, MutexGuard};

pub fn mutex_or_recover<'a, T>(lock: &'a Mutex<T>, ctx: &str) -> MutexGuard<'a, T> {
    lock.lock().unwrap_or_else(|e| {
        tracing::warn!(context = %ctx, "Mutex poisoned, recovering inner data");
        e.into_inner()
    })
}
