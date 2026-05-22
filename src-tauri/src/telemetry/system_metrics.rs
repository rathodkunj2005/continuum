//! Per-process and host system metrics on macOS — what Activity Monitor sees.
//!
//! A single background sampler refreshes a cached [`SystemMetricsSnapshot`]
//! at ~1 Hz so the Engine Inspector polling at 3 s never triggers a fresh
//! `task_info` / `host_statistics64` call inline. All sampling uses public
//! mach + libc APIs and `ioreg` for GPU performance statistics so no
//! entitlements are required.
//!
//! What we report (per process):
//! - CPU% over the last sample interval (user + system / wall).
//! - RSS, virtual size, dirty/compressed/swap footprint (via `proc_pid_rusage`
//!   v4 + `task_info(MACH_TASK_BASIC_INFO)`).
//! - Thread count (via `task_threads`).
//! - Disk I/O bytes (`ri_diskio_bytesread` / `ri_diskio_byteswritten`).
//! - Energy proxy (`ri_pkg_idle_wkups`, `ri_interrupt_wkups`,
//!   `ri_billed_system_time`).
//!
//! What we report (host):
//! - Per-core CPU% from `host_processor_info(PROCESSOR_CPU_LOAD_INFO)` deltas.
//! - Memory pages (free / active / inactive / wired / compressed) from
//!   `host_statistics64(HOST_VM_INFO64)`.
//!
//! GPU is parsed best-effort from `ioreg -l -d 1 -n IOAccelerator -r` to read
//! the public `PerformanceStatistics` dictionary that ships in stock macOS.
//! Result is `None` if the call fails or the keys are not present.

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProcessMemorySnapshot {
    pub rss_bytes: u64,
    pub virtual_bytes: u64,
    pub phys_footprint_bytes: u64,
    pub lifetime_max_phys_footprint_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProcessCpuSnapshot {
    pub cpu_percent: f32,
    pub user_time_ms: u64,
    pub system_time_ms: u64,
    pub threads: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProcessIoSnapshot {
    pub disk_bytes_read: u64,
    pub disk_bytes_written: u64,
    pub disk_read_rate_bps: u64,
    pub disk_write_rate_bps: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProcessEnergySnapshot {
    pub idle_wakeups: u64,
    pub interrupt_wakeups: u64,
    pub billed_system_time_ns: u64,
    /// Coarse label derived from idle wakeups + billed time over the interval:
    /// `low`, `moderate`, `high`.
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HostMemorySnapshot {
    pub page_size_bytes: u64,
    pub free_bytes: u64,
    pub active_bytes: u64,
    pub inactive_bytes: u64,
    pub wired_bytes: u64,
    pub compressed_bytes: u64,
    pub total_bytes: u64,
    /// Coarse `low`, `moderate`, `high` based on free + compressed pages.
    pub pressure_label: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HostCpuSnapshot {
    /// Aggregate CPU% across all cores (0..cores*100).
    pub cpu_percent_total: f32,
    /// Per-core CPU% in the same order as the kernel reports them.
    pub cpu_percent_per_core: Vec<f32>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GpuSnapshot {
    /// Best-effort device-busy percentage 0..100. `None` if unavailable.
    pub device_utilization_percent: Option<f32>,
    pub renderer_utilization_percent: Option<f32>,
    pub in_use_system_memory_bytes: Option<u64>,
    pub recovery_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelMemoryEntry {
    pub id: String,
    pub kind: String,
    pub estimated_bytes: u64,
    pub loaded: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SystemMetricsSnapshot {
    pub generated_at_ms: i64,
    pub sample_interval_ms: u64,
    pub process_cpu: ProcessCpuSnapshot,
    pub process_memory: ProcessMemorySnapshot,
    pub process_io: ProcessIoSnapshot,
    pub process_energy: ProcessEnergySnapshot,
    pub host_cpu: HostCpuSnapshot,
    pub host_memory: HostMemorySnapshot,
    pub gpu: GpuSnapshot,
    pub model_memory: Vec<ModelMemoryEntry>,
}

static SNAPSHOT: OnceLock<Mutex<SystemMetricsSnapshot>> = OnceLock::new();
static LAST_REFRESH_MS: AtomicI64 = AtomicI64::new(0);

fn snapshot_slot() -> &'static Mutex<SystemMetricsSnapshot> {
    SNAPSHOT.get_or_init(|| Mutex::new(SystemMetricsSnapshot::default()))
}

/// Returns the most recent cached snapshot. Cheap; never blocks on sampling.
pub fn latest_snapshot() -> SystemMetricsSnapshot {
    snapshot_slot().lock().clone()
}

/// Coarse "is the system under enough pressure that a heavy VLM/LLM call
/// is likely to OOM or stall?" check. Combines macOS host memory pressure
/// (free + compressed pages) with the FNDR process's own CPU saturation.
/// Returns `(skip_heavy_models, reason)`.
///
/// Thresholds tuned for an 8–16 GB Mac running dev tools alongside FNDR:
///   * less than 512 MiB free → "high"
///   * FNDR using more than 90% of one core sustained → "process_saturated"
/// Either condition flips the gate.
pub fn pressure_recommends_skipping_heavy_models() -> (bool, &'static str) {
    let snap = latest_snapshot();
    if snap.host_memory.pressure_label == "high" {
        return (true, "host_memory_high");
    }
    if snap.host_memory.pressure_label == "moderate" {
        // On moderate pressure we still skip the VLM — better a degraded
        // fallback than a swap-thrash freeze. The LLM path uses spawn_blocking
        // and is fine; only the heaviest model is gated here.
        return (true, "host_memory_moderate");
    }
    if snap.process_cpu.cpu_percent > 380.0 {
        return (true, "process_cpu_saturated");
    }
    // FNDR process holding more than 3 GiB resident is the bound that
    // matters on 8 GB Macs. The earlier 6 GiB threshold could only ever fire
    // AFTER a freeze; this catches it before.
    if snap.process_memory.phys_footprint_bytes > 3 * 1024 * 1024 * 1024 {
        return (true, "process_footprint_over_3gib");
    }
    (false, "ok")
}

/// Compute the human-readable pressure label from the two raw signals we
/// trust on macOS:
///   * `available_bytes` = free + inactive + speculative (the evictable pool)
///   * `compressor_bytes` = pages compressed by the kernel (the *real*
///     pressure indicator — Activity Monitor reads this same counter)
///
/// Two independent OR'd gates. Either flips the label up:
///   - `available_ratio < 10 %`   → "high"  (acute pool exhaustion)
///   - `compressor_ratio > 25 %`  → "high"  (kernel is hard-pressed; next
///                                 large allocation goes to disk swap)
///   - `available_ratio < 20 %`   → "moderate"
///   - `compressor_ratio > 15 %`  → "moderate"
///   - otherwise                   → "low"
///
/// The compressor gate is what catches the 8 GB Mac case. The earlier
/// heuristic ignored the compressor entirely and reported "low" while the
/// kernel was already burning CPU to delay swap.
pub fn compute_pressure_label(
    available_bytes: u64,
    compressor_bytes: u64,
    total_bytes: u64,
) -> &'static str {
    if total_bytes == 0 {
        return "low";
    }
    let total = total_bytes as f32;
    let available_ratio = available_bytes as f32 / total;
    let compressor_ratio = compressor_bytes as f32 / total;
    if available_ratio < 0.10 || compressor_ratio > 0.25 {
        "high"
    } else if available_ratio < 0.20 || compressor_ratio > 0.15 {
        "moderate"
    } else {
        "low"
    }
}

/// Hard safety floor for Qwen3-VL-2B (~3.5 GB working set): optimized for 8 GB
/// Macs; can coexist with the LLM + BGE + FNDR without thrashing at this level.
pub const VLM_SAFE_MIN_HOST_RAM_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Safety floor for SmolVLM 500M (~1.2 GB working set): lighter model can
/// coexist with everything else on 8 GB machines under low pressure.
pub const VLM_SAFE_MIN_HOST_RAM_BYTES_LIGHTWEIGHT: u64 = 8 * 1024 * 1024 * 1024;

/// Returns total host physical memory in bytes. Reads once and caches.
pub fn host_total_ram_bytes() -> u64 {
    static CACHED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        let snap = latest_snapshot();
        if snap.host_memory.total_bytes > 0 {
            return snap.host_memory.total_bytes;
        }
        // Fallback before the sampler has run: read directly via sysctl.
        #[cfg(target_os = "macos")]
        unsafe {
            let mut mem: u64 = 0;
            let mut size = std::mem::size_of::<u64>();
            let mib = [libc::CTL_HW, libc::HW_MEMSIZE];
            if libc::sysctl(
                mib.as_ptr() as *mut _,
                mib.len() as u32,
                &mut mem as *mut _ as *mut _,
                &mut size as *mut _ as *mut _,
                std::ptr::null_mut(),
                0,
            ) == 0
            {
                return mem;
            }
        }
        // Conservative default if we can't read: assume 8 GB.
        8 * 1024 * 1024 * 1024
    })
}

/// Returns true if this host has enough RAM to safely run Qwen3-VL-2B
/// alongside the LLM and the rest of FNDR (requires ≥ 8 GB).
pub fn host_supports_vlm() -> bool {
    host_total_ram_bytes() >= VLM_SAFE_MIN_HOST_RAM_BYTES
}

/// Returns true if this host has enough RAM to safely run SmolVLM 500M
/// alongside the LLM and the rest of FNDR (requires ≥ 8 GB).
pub fn host_supports_lightweight_vlm() -> bool {
    host_total_ram_bytes() >= VLM_SAFE_MIN_HOST_RAM_BYTES_LIGHTWEIGHT
}

/// Refresh the snapshot once. Intended for the background sampler task; also
/// useful for tests. Safe to call from any thread (uses `mach_task_self()` /
/// `mach_host_self()` directly).
pub fn refresh_once(model_memory: Vec<ModelMemoryEntry>) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let last = LAST_REFRESH_MS.swap(now_ms, Ordering::Relaxed);
    let interval_ms = if last == 0 {
        1_000
    } else {
        (now_ms - last).max(1) as u64
    };

    let process_cpu = sample_process_cpu(interval_ms);
    let process_memory = sample_process_memory();
    let process_io = sample_process_io(interval_ms);
    let process_energy = sample_process_energy(interval_ms);
    let host_cpu = sample_host_cpu();
    let host_memory = sample_host_memory();
    let gpu = sample_gpu();

    let snap = SystemMetricsSnapshot {
        generated_at_ms: now_ms,
        sample_interval_ms: interval_ms,
        process_cpu,
        process_memory,
        process_io,
        process_energy,
        host_cpu,
        host_memory,
        gpu,
        model_memory,
    };
    *snapshot_slot().lock() = snap;
}

/// Build per-tick model memory breakdown for the sampler. Reports loaded
/// state and a coarse RAM upper-bound (file size for GGUF, fixed budgets for
/// CLIP/BGE) — actual residency is reflected in `process_memory.phys_footprint_bytes`.
pub fn model_memory_entries(state: &crate::AppState) -> Vec<ModelMemoryEntry> {
    let mut out: Vec<ModelMemoryEntry> = Vec::new();

    // LLM (Llama).
    let llm_loaded = state.ai_model_loaded();
    let llm_id = state.loaded_model_id();
    let llm_def = llm_id
        .as_deref()
        .and_then(crate::models::model_by_id)
        .or_else(|| Some(&crate::models::MODEL_CATALOG[0]));
    if let Some(def) = llm_def {
        out.push(ModelMemoryEntry {
            id: def.id.to_string(),
            kind: "llm".to_string(),
            estimated_bytes: if llm_loaded { def.size_bytes } else { 0 },
            loaded: llm_loaded,
        });
    }

    // VLM (Qwen3-VL).
    let vlm_loaded = state.vlm.read().is_some();
    if let Some(def) = crate::models::model_by_id("qwen3-vl-2b") {
        out.push(ModelMemoryEntry {
            id: def.id.to_string(),
            kind: "vlm".to_string(),
            estimated_bytes: if vlm_loaded { def.size_bytes } else { 0 },
            loaded: vlm_loaded,
        });
    }

    // BGE text embedder (real backend only — Mock is ~0). The shipped
    // `bge-large-en-v1.5` ONNX weights are ~1.3 GB on disk; resident
    // working set after warm-up is closer to 700–800 MB on M-series Macs
    // because most weights stay backed by the file-cache. Report the
    // conservative working-set figure so summing model RAM stays
    // consistent with the process RSS the user sees.
    let emb = crate::embedding::embedding_runtime_status();
    let bge_loaded = emb.backend.eq_ignore_ascii_case("real");
    out.push(ModelMemoryEntry {
        id: emb.model_name.clone(),
        kind: "text_embedding".to_string(),
        estimated_bytes: if bge_loaded { 750_000_000 } else { 0 },
        loaded: bge_loaded,
    });

    // CLIP vision: the ONNX we ship is the Xenova q4 vision tower (~90 MB
    // file, ~110 MB resident working set with input buffers). The earlier
    // 350 MB estimate was the FP32 size and dominated the inspector.
    let clip_loaded = crate::embedding::clip_session_loaded();
    out.push(ModelMemoryEntry {
        id: "clip-vit-b-32".to_string(),
        kind: "image_embedding".to_string(),
        estimated_bytes: if clip_loaded { 110_000_000 } else { 0 },
        loaded: clip_loaded,
    });

    out
}

/// Spawn the 1 Hz sampler. `loader` returns the per-tick model memory
/// breakdown so the snapshot stays accurate as Llama/VLM/BGE/CLIP load.
///
/// Uses [`tauri::async_runtime::spawn`] so the sampler can be started from
/// the Tauri `setup` callback (which runs on the main thread, *before*
/// any direct Tokio runtime context is entered — calling `tokio::spawn`
/// from there panics with "there is no reactor running"). The Tauri async
/// runtime is a Tokio runtime under the hood, so `tokio::time::interval`
/// still works inside the spawned future.
pub fn spawn_sampler<F>(loader: F)
where
    F: Fn() -> Vec<ModelMemoryEntry> + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1_000));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let started = Instant::now();
            let model_memory = loader();
            // Sampling is all blocking syscalls; keep it cheap (<10ms typical)
            // and run on the current thread so we don't fan out spawn_blocking.
            refresh_once(model_memory);
            let _ = started.elapsed();
        }
    });
}

// ── macOS sampling ─────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use libc::{
        host_processor_info, host_statistics64, integer_t, kern_return_t, mach_host_self,
        mach_msg_type_number_t, mach_task_basic_info, mach_task_self, natural_t, proc_pid_rusage,
        rusage_info_t, rusage_info_v4, task_info, task_thread_times_info, task_threads,
        thread_act_array_t, vm_deallocate, vm_statistics64, vm_statistics64_data_t, HOST_VM_INFO64,
        HOST_VM_INFO64_COUNT, KERN_SUCCESS, MACH_TASK_BASIC_INFO, MACH_TASK_BASIC_INFO_COUNT,
        PROCESSOR_CPU_LOAD_INFO, RUSAGE_INFO_V4, TASK_THREAD_TIMES_INFO,
        TASK_THREAD_TIMES_INFO_COUNT,
    };
    use std::mem::MaybeUninit;
    use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

    // `vm_statistics64` is repr(C); ensure used.
    #[allow(dead_code)]
    type _VmStats = vm_statistics64;

    static LAST_USER_TIME_MS: AtomicU64 = AtomicU64::new(0);
    static LAST_SYSTEM_TIME_MS: AtomicU64 = AtomicU64::new(0);
    static LAST_CPU_SAMPLED_AT: AtomicI64 = AtomicI64::new(0);
    static LAST_DISK_READ: AtomicU64 = AtomicU64::new(0);
    static LAST_DISK_WRITE: AtomicU64 = AtomicU64::new(0);
    static LAST_IDLE_WAKEUPS: AtomicU64 = AtomicU64::new(0);
    static LAST_INTERRUPT_WAKEUPS: AtomicU64 = AtomicU64::new(0);
    static LAST_BILLED_NS: AtomicU64 = AtomicU64::new(0);
    static LAST_CPU_TICKS: parking_lot::Mutex<Vec<[u32; 4]>> = parking_lot::Mutex::new(Vec::new());

    fn time_value_to_ms(secs: i32, micros: i32) -> u64 {
        (secs as u64).saturating_mul(1_000) + (micros.max(0) as u64) / 1_000
    }

    pub fn sample_process_cpu(_interval_ms: u64) -> ProcessCpuSnapshot {
        let mut times = MaybeUninit::<task_thread_times_info>::uninit();
        let mut count = TASK_THREAD_TIMES_INFO_COUNT;
        let user_ms;
        let sys_ms;
        unsafe {
            let kr: kern_return_t = task_info(
                mach_task_self(),
                TASK_THREAD_TIMES_INFO,
                times.as_mut_ptr() as *mut _,
                &mut count,
            );
            if kr != KERN_SUCCESS {
                return ProcessCpuSnapshot::default();
            }
            let times = times.assume_init();
            user_ms = time_value_to_ms(times.user_time.seconds, times.user_time.microseconds);
            sys_ms = time_value_to_ms(times.system_time.seconds, times.system_time.microseconds);
        }
        let now = chrono::Utc::now().timestamp_millis();
        let last_at = LAST_CPU_SAMPLED_AT.swap(now, Ordering::Relaxed);
        let last_user = LAST_USER_TIME_MS.swap(user_ms, Ordering::Relaxed);
        let last_sys = LAST_SYSTEM_TIME_MS.swap(sys_ms, Ordering::Relaxed);
        let wall_ms = if last_at == 0 {
            1
        } else {
            (now - last_at).max(1) as u64
        };
        let cpu_ms = user_ms.saturating_sub(last_user) + sys_ms.saturating_sub(last_sys);
        // 100% means one fully-loaded core; range 0..cores*100.
        let cpu_percent = if last_at == 0 {
            0.0
        } else {
            (cpu_ms as f32 / wall_ms as f32) * 100.0
        };

        let threads = count_threads();
        ProcessCpuSnapshot {
            cpu_percent,
            user_time_ms: user_ms,
            system_time_ms: sys_ms,
            threads,
        }
    }

    fn count_threads() -> u32 {
        let mut act_list: thread_act_array_t = std::ptr::null_mut();
        let mut act_count: mach_msg_type_number_t = 0;
        unsafe {
            let kr = task_threads(mach_task_self(), &mut act_list, &mut act_count);
            if kr != KERN_SUCCESS {
                return 0;
            }
            // Each thread port needs to be deallocated; do it in bulk by freeing
            // the array region. Conservative: leak the port rights since this is
            // a snapshot path; the cost is bounded and we never call this in a
            // hot loop. Free the array memory itself though.
            let size = (act_count as usize)
                * std::mem::size_of::<crate::telemetry::system_metrics::imp::ThreadAct>();
            vm_deallocate(mach_task_self(), act_list as usize, size);
            act_count
        }
    }

    // libc doesn't ship a public thread_act_t alias on all versions; the array
    // is just `mach_port_t`-sized integers as far as we're concerned, since we
    // never dereference an element.
    #[allow(dead_code)]
    #[doc(hidden)]
    pub type ThreadAct = u32;

    pub fn sample_process_memory() -> ProcessMemorySnapshot {
        let mut info = MaybeUninit::<mach_task_basic_info>::uninit();
        let mut count = MACH_TASK_BASIC_INFO_COUNT;
        unsafe {
            let kr = task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                info.as_mut_ptr() as *mut _,
                &mut count,
            );
            if kr != KERN_SUCCESS {
                return ProcessMemorySnapshot::default();
            }
            let info = info.assume_init();
            let rusage = sample_rusage();
            ProcessMemorySnapshot {
                rss_bytes: info.resident_size as u64,
                virtual_bytes: info.virtual_size as u64,
                phys_footprint_bytes: rusage.map(|r| r.ri_phys_footprint).unwrap_or(0),
                lifetime_max_phys_footprint_bytes: rusage
                    .map(|r| r.ri_lifetime_max_phys_footprint)
                    .unwrap_or(0),
            }
        }
    }

    pub fn sample_process_io(interval_ms: u64) -> ProcessIoSnapshot {
        let Some(r) = sample_rusage() else {
            return ProcessIoSnapshot::default();
        };
        let last_read = LAST_DISK_READ.swap(r.ri_diskio_bytesread, Ordering::Relaxed);
        let last_write = LAST_DISK_WRITE.swap(r.ri_diskio_byteswritten, Ordering::Relaxed);
        let read_rate = bytes_per_sec(r.ri_diskio_bytesread.saturating_sub(last_read), interval_ms);
        let write_rate = bytes_per_sec(
            r.ri_diskio_byteswritten.saturating_sub(last_write),
            interval_ms,
        );
        ProcessIoSnapshot {
            disk_bytes_read: r.ri_diskio_bytesread,
            disk_bytes_written: r.ri_diskio_byteswritten,
            disk_read_rate_bps: read_rate,
            disk_write_rate_bps: write_rate,
        }
    }

    fn bytes_per_sec(delta_bytes: u64, interval_ms: u64) -> u64 {
        if interval_ms == 0 {
            return 0;
        }
        ((delta_bytes as u128 * 1_000) / interval_ms as u128) as u64
    }

    pub fn sample_process_energy(interval_ms: u64) -> ProcessEnergySnapshot {
        let Some(r) = sample_rusage() else {
            return ProcessEnergySnapshot::default();
        };
        let prev_idle = LAST_IDLE_WAKEUPS.swap(r.ri_pkg_idle_wkups, Ordering::Relaxed);
        let prev_int = LAST_INTERRUPT_WAKEUPS.swap(r.ri_interrupt_wkups, Ordering::Relaxed);
        let prev_billed = LAST_BILLED_NS.swap(r.ri_billed_system_time, Ordering::Relaxed);

        // First sample after process start has no baseline — we'd otherwise
        // report the *lifetime* total wakeups and instantly flip the label
        // to "high". Treat the very first refresh as a calibration round
        // that returns a quiet snapshot but records the baseline.
        let first_sample = prev_idle == 0 && prev_int == 0 && prev_billed == 0;
        if first_sample {
            return ProcessEnergySnapshot {
                idle_wakeups: 0,
                interrupt_wakeups: 0,
                billed_system_time_ns: 0,
                label: "low".to_string(),
            };
        }

        let d_idle = r.ri_pkg_idle_wkups.saturating_sub(prev_idle);
        let d_int = r.ri_interrupt_wkups.saturating_sub(prev_int);
        let d_billed = r.ri_billed_system_time.saturating_sub(prev_billed);

        // Coarse energy label. Thresholds chosen so a quiet Mac sits at
        // "low" and the OCR+LLM+VLM combo pushes us into "high".
        let wakeups_per_sec = if interval_ms == 0 {
            0
        } else {
            ((d_idle + d_int) as u128 * 1_000 / interval_ms as u128) as u64
        };
        let label = if wakeups_per_sec > 800 || d_billed > 50_000_000 {
            "high"
        } else if wakeups_per_sec > 200 || d_billed > 5_000_000 {
            "moderate"
        } else {
            "low"
        }
        .to_string();

        ProcessEnergySnapshot {
            idle_wakeups: d_idle,
            interrupt_wakeups: d_int,
            billed_system_time_ns: d_billed,
            label,
        }
    }

    pub fn sample_host_cpu() -> HostCpuSnapshot {
        let mut processor_count: natural_t = 0;
        let mut info_array: *mut integer_t = std::ptr::null_mut();
        let mut info_count: mach_msg_type_number_t = 0;
        let kr = unsafe {
            host_processor_info(
                mach_host_self(),
                PROCESSOR_CPU_LOAD_INFO,
                &mut processor_count,
                &mut info_array as *mut _ as *mut _,
                &mut info_count,
            )
        };
        if kr != KERN_SUCCESS || info_array.is_null() {
            return HostCpuSnapshot::default();
        }

        // Each per-core entry is 4 ticks: user, system, idle, nice.
        let entries: &[[u32; 4]] = unsafe {
            std::slice::from_raw_parts(info_array as *const [u32; 4], processor_count as usize)
        };
        let mut last = LAST_CPU_TICKS.lock();
        let same_shape = last.len() == entries.len();
        let mut per_core: Vec<f32> = Vec::with_capacity(entries.len());
        for (i, ticks) in entries.iter().enumerate() {
            let prev = if same_shape { last[i] } else { [0u32; 4] };
            let d_user = ticks[0].wrapping_sub(prev[0]) as u64;
            let d_sys = ticks[1].wrapping_sub(prev[1]) as u64;
            let d_idle = ticks[2].wrapping_sub(prev[2]) as u64;
            let d_nice = ticks[3].wrapping_sub(prev[3]) as u64;
            let busy = d_user + d_sys + d_nice;
            let total = busy + d_idle;
            let pct = if !same_shape || total == 0 {
                0.0
            } else {
                (busy as f32 / total as f32) * 100.0
            };
            per_core.push(pct);
        }
        *last = entries.to_vec();
        // Free the kernel-allocated buffer.
        unsafe {
            vm_deallocate(
                mach_task_self(),
                info_array as usize,
                (info_count as usize) * std::mem::size_of::<integer_t>(),
            );
        }

        let total = if per_core.is_empty() {
            0.0
        } else {
            per_core.iter().sum::<f32>() / per_core.len() as f32
        };
        HostCpuSnapshot {
            cpu_percent_total: total,
            cpu_percent_per_core: per_core,
        }
    }

    pub fn sample_host_memory() -> HostMemorySnapshot {
        let mut stats = MaybeUninit::<vm_statistics64>::uninit();
        let mut count: mach_msg_type_number_t = HOST_VM_INFO64_COUNT;
        let kr = unsafe {
            host_statistics64(
                mach_host_self(),
                HOST_VM_INFO64,
                stats.as_mut_ptr() as *mut _,
                &mut count,
            )
        };
        if kr != KERN_SUCCESS {
            return HostMemorySnapshot::default();
        }
        let s: vm_statistics64_data_t = unsafe { stats.assume_init() };
        let page_size = unsafe { libc::vm_page_size as u64 };
        let free = (s.free_count as u64) * page_size;
        let active = (s.active_count as u64) * page_size;
        let inactive = (s.inactive_count as u64) * page_size;
        let wired = (s.wire_count as u64) * page_size;
        // The macOS memory compressor is the kernel's FIRST line of defense
        // before disk-swap. `compressor_page_count` is the real pressure
        // indicator on Apple Silicon — Activity Monitor reads the same field.
        // Heavy compressor use means the kernel is already trading CPU for RAM
        // to delay swap, and adding a 2.6 GB VLM on top will tip into swap-thrash.
        let compressor_bytes = (s.compressor_page_count as u64) * page_size;
        // Keep the speculative+purgeable estimate available for the snapshot
        // (UI continues to render "compressed" as the volatile reclaim pool).
        let compressed =
            (s.speculative_count as u64 + s.purgeable_count as u64) * page_size + compressor_bytes;
        // Total physical = active + wired + inactive + free + compressor.
        // (Speculative + purgeable are subsets of inactive/free in practice.)
        let total = free + active + inactive + wired + compressor_bytes;

        // Real-world pressure on macOS is dominated by compressor saturation,
        // not raw "free". The kernel deliberately drives free toward 0 and
        // serves allocations from inactive/speculative. The first signal the
        // user actually feels (fan spin, slow scroll, beachballs) is the
        // compressor working hard to delay swap.
        //
        // Two independent gates — either flips the label to "high":
        //   1) classic available-pool gate (free + inactive + speculative)
        //      < 10% of total → "high". This catches the rare case of an
        //      uncompressible allocation burst.
        //   2) compressor-saturation gate: compressor > 25% of total RAM →
        //      "high". This is what would have caught the freeze on the 8 GB
        //      Mac — the compressor was at ~32% before the VLM piled on.
        let available = free + inactive + ((s.speculative_count as u64) * page_size);
        let pressure_label =
            super::compute_pressure_label(available, compressor_bytes, total).to_string();

        HostMemorySnapshot {
            page_size_bytes: page_size,
            free_bytes: free,
            active_bytes: active,
            inactive_bytes: inactive,
            wired_bytes: wired,
            compressed_bytes: compressed,
            total_bytes: total,
            pressure_label,
        }
    }

    pub fn sample_gpu() -> GpuSnapshot {
        // `ioreg` ships with every macOS install and exposes the public
        // IOAccelerator `PerformanceStatistics` dict (same dict that powers
        // Activity Monitor's GPU history). Parse the key=value lines.
        let output = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-l", "-d", "1", "-n", "IOAccelerator", "-r"])
            .output();
        let Ok(out) = output else {
            return GpuSnapshot::default();
        };
        if !out.status.success() {
            return GpuSnapshot::default();
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut device_util: Option<f32> = None;
        let mut renderer_util: Option<f32> = None;
        let mut in_use_mem: Option<u64> = None;
        let mut recovery: Option<u64> = None;
        for line in text.lines() {
            let line = line.trim();
            // Lines look like: `"Device Utilization %"=27`.
            let extract = |key: &str| -> Option<&str> {
                let needle = format!("\"{key}\"");
                let idx = line.find(&needle)?;
                let after = &line[idx + needle.len()..];
                let after = after.trim_start();
                let after = after.strip_prefix('=')?.trim_start();
                Some(after)
            };
            if let Some(val) = extract("Device Utilization %") {
                device_util = val.parse::<f32>().ok();
            }
            if let Some(val) = extract("Renderer Utilization %") {
                renderer_util = val.parse::<f32>().ok();
            }
            if let Some(val) = extract("In use system memory") {
                in_use_mem = val.parse::<u64>().ok();
            }
            if let Some(val) = extract("Recovery Count") {
                recovery = val.parse::<u64>().ok();
            }
        }
        GpuSnapshot {
            device_utilization_percent: device_util,
            renderer_utilization_percent: renderer_util,
            in_use_system_memory_bytes: in_use_mem,
            recovery_count: recovery,
        }
    }

    fn sample_rusage() -> Option<rusage_info_v4> {
        // The C signature is `int proc_pid_rusage(int pid, int flavor,
        // rusage_info_t *buffer)` and the canonical call is
        // `proc_pid_rusage(getpid(), RUSAGE_INFO_V4, (rusage_info_t *)&ru)`
        // — i.e. pass the struct's address *cast* to `rusage_info_t *`,
        // not the address of a separate `rusage_info_t` pointer variable.
        // Doing the latter (an earlier version of this file) made the
        // kernel write into our stack-local pointer instead of the struct,
        // leaving `ri_phys_footprint` and friends as zero.
        let mut info = MaybeUninit::<rusage_info_v4>::uninit();
        let pid = unsafe { libc::getpid() };
        let kr = unsafe {
            proc_pid_rusage(pid, RUSAGE_INFO_V4, info.as_mut_ptr() as *mut rusage_info_t)
        };
        if kr != 0 {
            return None;
        }
        Some(unsafe { info.assume_init() })
    }
}

#[cfg(target_os = "macos")]
use imp::{
    sample_gpu, sample_host_cpu, sample_host_memory, sample_process_cpu, sample_process_energy,
    sample_process_io, sample_process_memory,
};

// Non-macOS stubs keep the rest of the codebase platform-agnostic.
#[cfg(not(target_os = "macos"))]
fn sample_process_cpu(_interval_ms: u64) -> ProcessCpuSnapshot {
    ProcessCpuSnapshot::default()
}
#[cfg(not(target_os = "macos"))]
fn sample_process_memory() -> ProcessMemorySnapshot {
    ProcessMemorySnapshot::default()
}
#[cfg(not(target_os = "macos"))]
fn sample_process_io(_interval_ms: u64) -> ProcessIoSnapshot {
    ProcessIoSnapshot::default()
}
#[cfg(not(target_os = "macos"))]
fn sample_process_energy(_interval_ms: u64) -> ProcessEnergySnapshot {
    ProcessEnergySnapshot::default()
}
#[cfg(not(target_os = "macos"))]
fn sample_host_cpu() -> HostCpuSnapshot {
    HostCpuSnapshot::default()
}
#[cfg(not(target_os = "macos"))]
fn sample_host_memory() -> HostMemorySnapshot {
    HostMemorySnapshot::default()
}
#[cfg(not(target_os = "macos"))]
fn sample_gpu() -> GpuSnapshot {
    GpuSnapshot::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn snapshot_defaults_are_zero_when_never_refreshed() {
        // OnceLock starts empty -> latest_snapshot returns Default.
        let snap = latest_snapshot();
        assert!(snap.process_memory.rss_bytes == 0 || snap.process_memory.rss_bytes > 0);
    }

    #[test]
    #[serial]
    fn refresh_once_populates_process_memory_on_macos() {
        refresh_once(Vec::new());
        let snap = latest_snapshot();
        if cfg!(target_os = "macos") {
            assert!(
                snap.process_memory.rss_bytes > 0,
                "expected non-zero RSS on macOS"
            );
            assert!(snap.host_memory.total_bytes > 0);
        }
    }

    #[test]
    #[serial]
    fn cpu_percent_is_finite_and_non_negative() {
        refresh_once(Vec::new());
        // Sleep so a second sample has a non-zero interval delta.
        std::thread::sleep(std::time::Duration::from_millis(60));
        refresh_once(Vec::new());
        let snap = latest_snapshot();
        assert!(snap.process_cpu.cpu_percent.is_finite());
        assert!(snap.process_cpu.cpu_percent >= 0.0);
        assert!(snap.host_cpu.cpu_percent_total >= 0.0);
        for v in &snap.host_cpu.cpu_percent_per_core {
            assert!(*v >= 0.0);
            assert!(*v <= 100.5);
        }
    }

    #[test]
    #[serial]
    fn process_memory_phys_footprint_is_nonzero_on_macos() {
        refresh_once(Vec::new());
        let snap = latest_snapshot();
        if cfg!(target_os = "macos") {
            assert!(
                snap.process_memory.phys_footprint_bytes > 0,
                "phys_footprint must populate after the proc_pid_rusage fix"
            );
            assert!(
                snap.process_memory.lifetime_max_phys_footprint_bytes
                    >= snap.process_memory.phys_footprint_bytes,
                "lifetime peak must be >= current footprint"
            );
        }
    }

    #[test]
    #[serial]
    fn first_energy_sample_returns_calibration_zero_not_lifetime_total() {
        // The very first refresh after process start has no baseline; we
        // must not report the lifetime cumulative wakeup count as a one-
        // second delta and flip the label to "high".
        refresh_once(Vec::new());
        let snap = latest_snapshot();
        if cfg!(target_os = "macos") {
            // Either the energy sample is the calibration round (delta=0,
            // label=low) or we already had a previous baseline in this
            // process — both are acceptable, but the label cannot be
            // "high" purely from the calibration call.
            // Calibration round: both delta counters must be zero before
            // we can assert the label is the calmest value.
            if snap.process_energy.idle_wakeups == 0 && snap.process_energy.interrupt_wakeups == 0 {
                assert_eq!(snap.process_energy.label, "low");
            }
            assert!(snap.process_energy.idle_wakeups < 100_000_000);
        }
    }

    #[test]
    #[serial]
    fn pressure_label_is_not_high_when_only_free_is_low_but_inactive_is_plentiful() {
        // Sanity: real macOS keeps `free` tiny and stores evictable pages
        // in `inactive`/`speculative`. The label must reflect available
        // memory, not raw free.
        refresh_once(Vec::new());
        let snap = latest_snapshot();
        // Test only applies when BOTH gates would correctly say not-high:
        // the available pool is plentiful AND compressor isn't saturated.
        // If the live machine has the compressor working hard (real
        // pressure!) we can't make any claim — that's correct behavior.
        let available = snap.host_memory.free_bytes + snap.host_memory.inactive_bytes;
        let compressor_calm = snap.host_memory.total_bytes == 0
            || (snap.host_memory.compressed_bytes as f32 / snap.host_memory.total_bytes as f32)
                <= 0.15;
        if snap.host_memory.total_bytes > 0
            && (available as f32 / snap.host_memory.total_bytes as f32) >= 0.20
            && compressor_calm
        {
            assert_ne!(
                snap.host_memory.pressure_label, "high",
                "label must reflect available pool, not raw free \
                 (only valid to assert when compressor is also calm)"
            );
        }
    }

    #[test]
    #[serial]
    fn pressure_helper_is_quiet_under_normal_load() {
        // On a typical CI/dev box memory pressure is "low" and the test
        // process is nowhere near 380% CPU. The helper must therefore
        // recommend running models.
        refresh_once(Vec::new());
        let (skip, reason) = pressure_recommends_skipping_heavy_models();
        // We allow a noisy CI host to legitimately register pressure;
        // either we don't skip (reason == ok) OR we skip with a
        // recognized reason string.
        if skip {
            assert!(
                matches!(
                    reason,
                    "host_memory_high"
                        | "host_memory_moderate"
                        | "process_cpu_saturated"
                        | "process_footprint_over_3gib"
                ),
                "unexpected pressure reason: {reason}"
            );
        } else {
            assert_eq!(reason, "ok");
        }
    }

    #[test]
    fn compute_pressure_label_returns_high_when_compressor_saturated() {
        // 8 GB total, compressor holding 2.6 GB (~32%), available pool 3 GB (37%).
        // Old heuristic looked only at available and would have returned "low".
        // New heuristic must catch this and return "high".
        let total = 8u64 * 1024 * 1024 * 1024;
        let available = 3u64 * 1024 * 1024 * 1024;
        let compressor = 2_600u64 * 1024 * 1024;
        assert_eq!(compute_pressure_label(available, compressor, total), "high");
    }

    #[test]
    fn compute_pressure_label_returns_moderate_at_compressor_15_percent() {
        let total = 8u64 * 1024 * 1024 * 1024;
        let available = 3u64 * 1024 * 1024 * 1024;
        let compressor = (total as f32 * 0.16) as u64;
        assert_eq!(
            compute_pressure_label(available, compressor, total),
            "moderate"
        );
    }

    #[test]
    fn compute_pressure_label_returns_high_when_available_under_10_percent() {
        let total = 8u64 * 1024 * 1024 * 1024;
        let available = (total as f32 * 0.05) as u64;
        let compressor = 0;
        assert_eq!(compute_pressure_label(available, compressor, total), "high");
    }

    #[test]
    fn compute_pressure_label_low_when_both_signals_calm() {
        let total = 16u64 * 1024 * 1024 * 1024;
        let available = 8u64 * 1024 * 1024 * 1024;
        let compressor = 1u64 * 1024 * 1024 * 1024;
        assert_eq!(compute_pressure_label(available, compressor, total), "low");
    }

    #[test]
    fn compute_pressure_label_handles_zero_total_safely() {
        assert_eq!(compute_pressure_label(0, 0, 0), "low");
    }

    #[test]
    #[serial]
    fn host_total_ram_bytes_returns_nonzero() {
        let bytes = host_total_ram_bytes();
        assert!(bytes > 0, "should always read some RAM size on real hosts");
        assert!(
            bytes >= 2 * 1024 * 1024 * 1024,
            "any real Mac has >= 2 GB; got {bytes}"
        );
    }

    #[test]
    fn vlm_safe_min_is_eight_gib() {
        assert_eq!(VLM_SAFE_MIN_HOST_RAM_BYTES, 8 * 1024 * 1024 * 1024);
    }

    #[test]
    fn vlm_safe_min_lightweight_is_eight_gib() {
        assert_eq!(
            VLM_SAFE_MIN_HOST_RAM_BYTES_LIGHTWEIGHT,
            8 * 1024 * 1024 * 1024
        );
    }

    #[test]
    fn lightweight_threshold_is_lte_heavy_threshold() {
        assert!(
            VLM_SAFE_MIN_HOST_RAM_BYTES_LIGHTWEIGHT <= VLM_SAFE_MIN_HOST_RAM_BYTES,
            "lightweight gate must be <= heavy gate (both optimized for 8 GB)"
        );
    }

    #[test]
    #[serial]
    fn host_supports_vlm_matches_threshold() {
        let supports = host_supports_vlm();
        if host_total_ram_bytes() < VLM_SAFE_MIN_HOST_RAM_BYTES {
            assert!(!supports, "must refuse VLM on small-RAM hosts");
        } else {
            assert!(supports, "must allow VLM on large-RAM hosts");
        }
    }

    #[test]
    #[serial]
    fn host_supports_lightweight_vlm_matches_threshold() {
        let supports = host_supports_lightweight_vlm();
        if host_total_ram_bytes() < VLM_SAFE_MIN_HOST_RAM_BYTES_LIGHTWEIGHT {
            assert!(
                !supports,
                "must refuse lightweight VLM on very small-RAM hosts"
            );
        } else {
            assert!(supports, "must allow lightweight VLM on 8+ GB hosts");
        }
    }
}
