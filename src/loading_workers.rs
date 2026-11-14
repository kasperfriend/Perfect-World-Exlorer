//! Shared CPU budget for independent startup worker pools.
//!
//! Terrain decoding, world-model parsing and water texture decoding overlap.
//! Letting each pool independently use every logical core oversubscribes the
//! machine; a single permit gate lets pools scale up while bounding concurrent
//! CPU work across them. `PW_LOAD_WORKERS` is an opt-in override (1..=128).
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

/// Maximum workers each independent pool may queue. The gate below controls
/// how many of those workers may run CPU-heavy work at once.
pub fn worker_limit() -> usize {
    configured_workers().unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
    })
}

fn gate() -> &'static Gate {
    GATE.get_or_init(|| {
        let permits = configured_workers().unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|count| count.get().saturating_sub(1).max(1))
                .unwrap_or(1)
        });
        Gate {
            available: Mutex::new(permits),
            wake: Condvar::new(),
        }
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
    fn permits_are_released_on_drop() {
        assert!(worker_limit() >= 1);
        drop(acquire());
        drop(acquire());
    }
}
