//! Shared CPU budget for independent startup worker pools.
//!
//! Terrain decoding, world-model parsing and water texture decoding overlap.
//! Letting each pool independently use every logical core oversubscribes the
//! machine; a single permit gate lets pools scale up while bounding concurrent
//! CPU work across them. By default the gate uses all available logical CPUs;
//! `PW_LOAD_WORKERS` is an opt-in override (1..=128).
use std::sync::{Condvar, Mutex, OnceLock};

struct Gate {
    available: Mutex<usize>,
    wake: Condvar,
}

static GATE: OnceLock<Gate> = OnceLock::new();

fn configured_workers() -> Option<usize> {
    std::env::var("PW_LOAD_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=128).contains(value))
}

fn hardware_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
}

fn resolve_worker_count(configured: Option<usize>, hardware: usize) -> usize {
    configured.unwrap_or(hardware.max(1))
}

/// Maximum workers each independent pool may queue. The shared gate controls
/// the aggregate number of CPU-heavy tasks allowed to run concurrently.
pub fn worker_limit() -> usize {
    resolve_worker_count(configured_workers(), hardware_worker_count())
}

fn gate() -> &'static Gate {
    GATE.get_or_init(|| Gate {
        // Loading is the foreground operation. Use every CPU reported by the
        // OS rather than reserving one: progress reporting is lightweight and
        // reserving a core can needlessly lengthen CPU-bound decompression.
        available: Mutex::new(worker_limit()),
        wake: Condvar::new(),
    })
}

pub struct Permit;

/// Wait until a CPU slot is available; release it automatically at scope exit.
pub fn acquire() -> Permit {
    let gate = gate();
    let mut available = gate.available.lock().unwrap_or_else(|e| e.into_inner());
    while *available == 0 {
        available = gate.wake.wait(available).unwrap_or_else(|e| e.into_inner());
    }
    *available -= 1;
    Permit
}

impl Drop for Permit {
    fn drop(&mut self) {
        let gate = gate();
        let mut available = gate.available.lock().unwrap_or_else(|e| e.into_inner());
        *available += 1;
        gate.wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_worker_budget_uses_all_available_parallelism() {
        assert_eq!(resolve_worker_count(None, 8), 8);
        assert_eq!(resolve_worker_count(None, 1), 1);
        assert_eq!(resolve_worker_count(Some(3), 8), 3);
        assert!(worker_limit() >= 1);
    }

    #[test]
    fn permits_are_released_on_drop() {
        drop(acquire());
        drop(acquire());
    }
}
