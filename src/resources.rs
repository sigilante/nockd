//! Per-app resource sampling (CPU% + RSS). The motivation is OOM awareness (DESIGN OQ8):
//! NockApps carry large PMA/jam state, and runaway memory is the classic killer. We sample
//! the supervised pids on a periodic tick and stash the latest reading on `RuntimeStatus`
//! (ephemeral observed state, like health) — no history, no DB.
//!
//! Cross-platform via `sysinfo` (the daemon runs on Linux VPS/Pi targets and macOS for dev).
//! v1 measures the supervised pid only — not its process group/children (noted gap; matters
//! only for apps that fork workers — a NockApp is normally the process itself).

use std::collections::HashMap;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// CPU measurement window. sysinfo derives process CPU% from the gap between two refreshes;
/// ~300ms gives a stable reading (verified ~96–100% for a busy-spinner on macOS).
const WINDOW: Duration = Duration::from_millis(300);

/// Sample `(cpu_pct, rss_bytes)` for each given pid that's alive. Self-contained two-shot:
/// `new_all()` (establishes the CPU baseline — a bare `new()` reports 0% CPU on macOS) →
/// refresh → wait `WINDOW` → refresh → read. `cpu_pct` follows the `top` convention (100% ==
/// one core; can exceed 100 for multi-threaded apps). Dead pids are omitted.
///
/// Blocking: sleeps `WINDOW` and does two full-system scans. Call from a blocking context
/// (e.g. `spawn_blocking`), not directly on an async runtime.
pub fn sample(pids: &[u32]) -> HashMap<u32, (f32, u64)> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let kind = ProcessRefreshKind::everything();
    let mut sys = System::new_all();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, false, kind);
    std::thread::sleep(WINDOW);
    sys.refresh_processes_specifics(ProcessesToUpdate::All, false, kind);

    let mut out = HashMap::with_capacity(pids.len());
    for p in pids {
        if let Some(proc_) = sys.process(Pid::from_u32(*p)) {
            out.insert(*p, (proc_.cpu_usage(), proc_.memory()));
        }
    }
    out
}
