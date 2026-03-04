//! Helpers that recover from poisoned std::sync locks instead of panicking.
//! A poisoned lock means a thread panicked while holding it. These helpers
//! log a warning and return the inner data, preventing crash cascades.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub fn mutex_or_recover<'a, T>(lock: &'a Mutex<T>, ctx: &str) -> MutexGuard<'a, T> {
    lock.lock().unwrap_or_else(|e| {
        tracing::warn!(context = %ctx, "Mutex poisoned, recovering inner data");
        e.into_inner()
    })
}

pub fn read_or_recover<'a, T>(lock: &'a RwLock<T>, ctx: &str) -> RwLockReadGuard<'a, T> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(context = %ctx, "RwLock poisoned (read), recovering");
        e.into_inner()
    })
}

pub fn write_or_recover<'a, T>(lock: &'a RwLock<T>, ctx: &str) -> RwLockWriteGuard<'a, T> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(context = %ctx, "RwLock poisoned (write), recovering");
        e.into_inner()
    })
}
