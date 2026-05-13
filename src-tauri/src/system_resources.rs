//! macOS-oriented heuristics for **idle** background jobs (graph commit, etc.).

use crate::AppState;
use std::process::Command;
use std::sync::atomic::Ordering;

/// Returns true when graph Lance writes are allowed (charging or battery > 40%, CPU heuristic, not paused).
pub fn allows_graph_idle_commit(state: &AppState) -> bool {
    if state.is_paused.load(Ordering::Relaxed) {
        return false;
    }
    if state.graph_governor_battery_saver.load(Ordering::Relaxed) {
        return false;
    }
    if !charging_or_battery_above(0.40) {
        return false;
    }
    if !cpu_load_below(0.60) {
        return false;
    }
    true
}

#[cfg(target_os = "macos")]
fn charging_or_battery_above(min_fraction: f32) -> bool {
    let Ok(out) = Command::new("pmset").args(["-g", "batt"]).output() else {
        return true;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    if text.contains("AC Power") || text.contains("charged") || text.contains("charging") {
        return true;
    }
    if let Some(idx) = text.find('%') {
        let slice = &text[..idx];
        if let Some(digits) = slice
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
            .parse::<u32>()
            .ok()
        {
            return (digits as f32 / 100.0) >= min_fraction;
        }
    }
    true
}

#[cfg(not(target_os = "macos"))]
fn charging_or_battery_above(_min_fraction: f32) -> bool {
    true
}

#[cfg(target_os = "macos")]
fn cpu_load_below(max_fraction: f32) -> bool {
    let mut load: [f64; 3] = [0.0; 3];
    let n = unsafe { libc::getloadavg(load.as_mut_ptr(), 3) };
    if n <= 0 {
        return true;
    }
    let ncpu = std::thread::available_parallelism()
        .map(|n| n.get().max(1))
        .unwrap_or(4) as f64;
    (load[0] / ncpu) < max_fraction as f64
}

#[cfg(not(target_os = "macos"))]
fn cpu_load_below(_max_fraction: f32) -> bool {
    true
}
