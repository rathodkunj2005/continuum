//! FNDR - Privacy-first local memory search
//!
//! Main entry point for the Tauri application.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Timelike;
use fndr_lib::{capture, config::Config, graph::GraphStore, ipc, models, storage::Store, AppState};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Tokio worker stack size — deep async chains through LanceDB, whisper
// transcription, and embedding can overflow the default 2 MB stack.
const TOKIO_WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;
// Tunable background-task cadences. Kept here so the scheduling story for the
// whole process is visible at a glance.
const MAINTENANCE_FIRST_DELAY: Duration = Duration::from_secs(60);
const STORAGE_RECLAIM_INTERVAL: Duration = Duration::from_secs(6 * 3600);
const RETENTION_FIRST_DELAY: Duration = Duration::from_secs(10 * 60);
const RETENTION_INTERVAL: Duration = Duration::from_secs(24 * 3600);
const RETENTION_RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60);
const DECAY_INTERVAL: Duration = Duration::from_secs(6 * 3600);
const DECAY_LOOKBACK_MS: i64 = 24 * 3600 * 1000;
const DECAY_FLOOR: f32 = 0.15;
const MS_PER_DAY: f64 = 86_400_000.0;
const PROACTIVE_NOTIFICATION_STARTUP_DELAY: Duration = Duration::from_secs(60);
const PROACTIVE_NOTIFICATION_TICK: Duration = Duration::from_secs(30);
const STALE_TASK_CHECK_INTERVAL: Duration = Duration::from_secs(7200);
const STALE_TASK_THRESHOLD_MS: i64 = 3 * 86_400_000;
const STALE_TASK_TITLES_SHOWN: usize = 3;
const APP_SWITCH_SAMPLE_INTERVAL: Duration = Duration::from_secs(10);
const APP_SWITCH_WINDOW: usize = 15;
const APP_SWITCH_RECENT_CAPACITY: usize = 20;
const APP_SWITCH_THRESHOLD: usize = 8;
const APP_SWITCH_UNIQUE_THRESHOLD: usize = 6;
const GRAPH_COMMIT_INTERVAL: Duration = Duration::from_secs(90);
const MEMORY_REVIEW_INTERVAL: Duration = Duration::from_secs(45);

fn main() {
    // `fndr operator-mcp`: Notch Do's computer-use tools, started by Codex as
    // a child process. It must not initialise the app, and stdout is the protocol.
    if std::env::args().nth(1).as_deref() == Some("operator-mcp") {
        std::process::exit(fndr_lib::operator::mcp::run());
    }
    // Install default TLS crypto provider (required by rustls 0.23+)
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Load environment variables from .env if present
    let _ = dotenvy::dotenv();

    // Quiet ggml / Metal chatter before any llama.cpp init (shell can override).
    if std::env::var_os("GGML_METAL_LOG_INFO").is_none() {
        std::env::set_var("GGML_METAL_LOG_INFO", "0");
    }
    if std::env::var_os("GGML_METAL_LOG_WARN").is_none() {
        std::env::set_var("GGML_METAL_LOG_WARN", "0");
    }
    if std::env::var_os("GGML_LOG_LEVEL").is_none() {
        std::env::set_var("GGML_LOG_LEVEL", "0");
    }

    // Initialize logging
    use tracing_subscriber::{fmt, EnvFilter};
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "fndr=info,fndr_lib=info".into()),
        )
        .with(fmt::layer())
        .init();

    tracing::info!("Starting FNDR...");

    // Build a tokio runtime with a bigger worker stack (default is 2 MB) to
    // prevent SIGABRT on deep async chains.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(TOKIO_WORKER_STACK_BYTES)
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");
    // Leak so it lives for the process lifetime; tauri holds the handle only.
    let rt_ref: &'static tokio::runtime::Runtime = Box::leak(Box::new(rt));
    tauri::async_runtime::set(rt_ref.handle().clone());

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            // Load configuration
            let config = Config::load_or_create()?;
            tracing::info!("Configuration loaded");

            // Validate the embedding environment up-front so the user sees one
            // clear actionable error if the on-disk model files don't match
            // the centralized contract (model file, tokenizer, dimension).
            // This is intentionally non-fatal: a missing model still falls
            // back to mock when FNDR_ALLOW_MOCK_EMBEDDER=1, and the real
            // dimension check still runs inside RealEmbedder::new(). The
            // preflight just turns silent fallback into an obvious log line.
            let preflight = fndr_lib::embedding::preflight_embedding_environment(&config.embedding);
            if preflight.is_ready() {
                tracing::info!("{}", preflight.describe());
            } else {
                tracing::warn!(target: "fndr::embedding", "{}", preflight.describe());
            }

            // Initialize store (LanceDB) — open_all_tables internally calls
            // validate_memory_vector_schema, which surfaces a clear error if
            // an existing Lance table's vector dimension diverges from the
            // current contract.
            let data_dir = fndr_lib::config::fndr_app_data_dir(app.path())?;
            if fndr_lib::config::data_dir_override().is_some() {
                tracing::warn!(path = %data_dir.display(), "FNDR_DATA_DIR override active (demo profile)");
            }
            match fndr_lib::speech::cleanup_stale_voice_inputs(&data_dir) {
                Ok(removed) if removed > 0 => {
                    tracing::info!(removed, "Removed abandoned temporary voice inputs")
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(
                    "Could not clean abandoned temporary voice inputs: {}",
                    error
                ),
            }
            fndr_lib::ipc::commands::start_screen_guide_diagnostic_maintenance(
                data_dir.clone(),
            );
            let store = Store::new(&data_dir)?;
            let store_arc = Arc::new(store);
            tracing::info!("Consolidated store initialized at {:?}", data_dir);
            let state_store = Arc::new(fndr_lib::storage::StateStore::new(&data_dir)?);
            tracing::info!("State store initialized");

            let graph = GraphStore::new(store_arc.clone());
            tracing::info!("Graph store initialized");

            if let Err(err) = tauri::async_runtime::block_on(fndr_lib::meeting::init(
                data_dir.clone(),
                store_arc.clone(),
            )) {
                tracing::warn!("Meeting subsystem initialization failed: {}", err);
            }

            // Apply retention: remove records older than config.retention_days (0 = keep forever)
            if config.retention_days > 0 {
                match tauri::async_runtime::block_on(
                    store_arc.delete_older_than(config.retention_days),
                ) {
                    Ok(n) if n > 0 => tracing::info!("Retention: removed {} old records", n),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("Retention cleanup failed: {}", e),
                }
            }

            if config.import_retention_days > 0 {
                match tauri::async_runtime::block_on(
                    store_arc.delete_imports_older_than(config.import_retention_days),
                ) {
                    Ok(n) if n > 0 => tracing::info!("Import retention: removed {} old records", n),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("Import retention cleanup failed: {}", e),
                }
            }

            // Verbatim page text follows its own switch, window and budget.
            tauri::async_runtime::block_on(fndr_lib::verbatim::sweep_now(
                &store_arc,
                &config.verbatim,
            ));

            // Create app state with no engines yet — we kick off the eager
            // load below as a background task so the Tauri setup callback
            // is not blocked on Metal initialization. The capture loop
            // also has its own lazy fallback via `ensure_inference_engine`,
            // so even if this restore fails the user is never stuck.
            let state = Arc::new(AppState::new(
                data_dir.clone(),
                config,
                store_arc,
                state_store,
                graph,
                None,
            ));
            state.set_app_handle(app.handle().clone());
            fndr_lib::privacy_proof::init_model_request_log(&data_dir);
            ipc::commands::sweep_screen_guide_scratch();
            ipc::commands::reap_stale_hermes_gateway(&data_dir);
            // A hand-off that was working when FNDR quit has no worker now.
            if let Err(error) = fndr_lib::handoff::store::sweep_after_restart(
                &state.state_store,
                chrono::Utc::now().timestamp_millis(),
            ) {
                tracing::warn!(%error, "hand_off:sweep_after_restart_failed");
            }

            // Restore last-session model: if onboarding is complete and the
            // preferred GGUF is on disk, load it eagerly. This means the
            // first capture frame after relaunch already has an LLM ready
            // for structured-memory extraction — without it the user pays
            // first-touch latency and the grounding gate quietly drops
            // every frame between launch and the first lazy load.
            {
                let restore_state = state.clone();
                let restore_data_dir = data_dir.clone();
                tauri::async_runtime::spawn(async move {
                    let config = restore_state.config.read().clone();
                    let onboarding_complete = restore_state.preferred_model_id().is_some();
                    let preferred =
                        models::inference_preferred_model_id(restore_data_dir.as_path(), &config);
                    let resolved =
                        models::resolve_model(preferred.as_deref(), Some(restore_data_dir.as_path()));
                    if !onboarding_complete {
                        tracing::info!(
                            "Skipping AI eager restore: no preferred model recorded in onboarding"
                        );
                        return;
                    }
                    let Some(_) = resolved else {
                        tracing::info!(
                            "Skipping AI eager restore: preferred model {:?} is not on disk yet",
                            preferred
                        );
                        return;
                    };
                    tracing::info!(
                        "Restoring last-session AI engine ({:?}) eagerly so capture starts hot",
                        preferred
                    );
                    let loaded =
                        fndr_lib::load_ai_engines(restore_data_dir.as_path(), &config).await;
                    restore_state.replace_ai_engines(loaded.inference);
                });
            }

            // Start capture pipeline (named thread + 8MB stack; see capture::spawn_capture_loop)
            capture::spawn_capture_loop(state.clone());

            // Apply retention during long-running sessions and collapse Lance
            // fragments after capture has gone idle. The frame lock prevents
            // compaction from racing an admitted capture frame.
            {
                let retention_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(RETENTION_FIRST_DELAY).await;
                    loop {
                        let config = retention_state.config.read().clone();
                        let capturing = retention_state.is_capturing();
                        let fps = if capturing {
                            capture::AdaptiveSampler::new().get_current_fps(&config)
                        } else {
                            0.0
                        };
                        let Some(frame_guard) =
                            capture::try_idle_maintenance_guard(capturing, fps)
                        else {
                            tokio::time::sleep(RETENTION_RETRY_INTERVAL).await;
                            continue;
                        };

                        let mut failed = false;
                        if config.retention_days > 0 {
                            match retention_state
                                .store
                                .delete_older_than(config.retention_days)
                                .await
                            {
                                Ok(removed) if removed > 0 => {
                                    tracing::info!("Periodic retention removed {} old records", removed)
                                }
                                Ok(_) => {}
                                Err(err) => {
                                    failed = true;
                                    tracing::warn!("Periodic retention failed: {err}");
                                }
                            }
                        }
                        if config.import_retention_days > 0 {
                            match retention_state
                                .store
                                .delete_imports_older_than(config.import_retention_days)
                                .await
                            {
                                Ok(removed) if removed > 0 => tracing::info!(
                                    "Periodic import retention removed {} old records",
                                    removed
                                ),
                                Ok(_) => {}
                                Err(err) => {
                                    failed = true;
                                    tracing::warn!("Periodic import retention failed: {err}");
                                }
                            }
                        }
                        fndr_lib::verbatim::sweep_now(&retention_state.store, &config.verbatim)
                            .await;
                        match retention_state.store.compact_and_prune(7).await {
                            Ok(_) => tracing::info!("Periodic Lance compaction finished"),
                            Err(err) => {
                                failed = true;
                                tracing::warn!("Periodic Lance compaction failed: {err}");
                            }
                        }
                        drop(frame_guard);
                        tokio::time::sleep(if failed {
                            RETENTION_RETRY_INTERVAL
                        } else {
                            RETENTION_INTERVAL
                        })
                        .await;
                    }
                });
            }

            // Background task: compact legacy capture payloads and purge stray artifacts.
            {
                let maintenance_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(MAINTENANCE_FIRST_DELAY).await;
                    loop {
                        let image_dir = maintenance_state.store.data_dir().to_path_buf();
                        match tauri::async_runtime::spawn_blocking(move || {
                            fndr_lib::privacy::vault::seal_capture_images(
                                &fndr_lib::privacy::vault::KeychainKey::default(),
                                &image_dir,
                            )
                        })
                        .await
                        {
                            Ok(report) if report.sealed > 0 || report.failed > 0 => {
                                tracing::info!(
                                    "Capture image sealing: {} sealed, {} failed",
                                    report.sealed,
                                    report.failed
                                );
                            }
                            Err(error) => tracing::warn!("Capture image sealing failed: {error}"),
                            _ => {}
                        }
                        match ipc::commands::reclaim_memory_storage_silent(maintenance_state.clone()).await {
                            Ok(summary)
                                if summary.bytes_reclaimed > 0
                                    || summary.chars_reclaimed > 0
                                    || summary.screenshot_files_deleted > 0 =>
                            {
                                tracing::info!(
                                    "Automatic storage reclaim: rewrote {} / {} cards, removed {} files, reclaimed {} bytes",
                                    summary.records_rewritten,
                                    summary.records_scanned,
                                    summary.screenshot_files_deleted,
                                    summary.bytes_reclaimed
                                );
                            }
                            Ok(_) => tracing::debug!("Automatic storage reclaim found nothing to trim"),
                            Err(err) => tracing::debug!("Automatic storage reclaim skipped: {}", err),
                        }
                        tokio::time::sleep(STORAGE_RECLAIM_INTERVAL).await;
                    }
                });
            }

            // Background: idle insight-graph Lance commits (non-blocking).
            {
                let graph_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(MAINTENANCE_FIRST_DELAY).await;
                    let mut interval = tokio::time::interval(GRAPH_COMMIT_INTERVAL);
                    loop {
                        interval.tick().await;
                        if let Err(err) =
                            fndr_lib::ipc::commands::commit_graph_updates(graph_state.clone()).await
                        {
                            tracing::debug!("commit_graph_updates: {err}");
                        }
                    }
                });
            }

            // Background: post-capture memory_review worker. Drains the
            // pending_memory_reviews queue one job per tick, pressure-gated and
            // serialized through the global model pipeline lock. See ADR 007
            // and the memory_review module docs for the full lifecycle.
            {
                let review_state = state.clone();
                fndr_lib::memory_review::spawn_worker(review_state, MEMORY_REVIEW_INTERVAL);
            }

            // Background: daily memory-review scheduler. Wakes hourly and runs
            // the previous calendar day's batch review pass once per day when
            // the inference engine is loaded and the pressure gate allows. The
            // pipeline itself acquires the model_pipeline_lock so capture is
            // never blocked.
            {
                let daily_state = state.clone();
                fndr_lib::memory_review::spawn_daily_scheduler(daily_state);
            }

            // First run: install the pinned Hermes into FNDR's app data so the
            // agent works without a system install. Off the main thread; a
            // failure is reported when Hermes is first started.
            {
                let hermes_state = state.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(error) = ipc::commands::ensure_pinned_hermes(&hermes_state) {
                        tracing::warn!(%error, "hermes:pinned_install_failed");
                    }
                });
            }

            // Demo profile only: queue every stored memory for on-device review
            // once, so the seeded week is reviewed by the real local model.
            if std::env::var("FNDR_DEMO_REVIEW_BACKFILL").ok().as_deref() == Some("1") {
                let backfill_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    // Let the model finish loading before queueing review work.
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    match fndr_lib::memory_review::backfill_memory_review_in_range(
                        &backfill_state,
                        &backfill_state.store,
                        0,
                        now_ms,
                        now_ms,
                        false,
                    )
                    .await
                    {
                        Ok(_) => tracing::info!("demo review backfill queued"),
                        Err(err) => tracing::warn!("demo review backfill failed: {err}"),
                    }
                });
            }

            let runtime_state = state.clone();

            // Background: 1 Hz Activity-Monitor-grade system metrics sampler.
            // Surfaces CPU%, RAM, threads, energy, disk I/O, GPU%, and the
            // per-model RAM breakdown to the Engine Inspector.
            {
                let metrics_state = state.clone();
                fndr_lib::telemetry::system_metrics::spawn_sampler(move || {
                    fndr_lib::telemetry::system_metrics::model_memory_entries(&metrics_state)
                });
            }

            // Background: optional NDJSON metrics dump for baseline/soak reports
            // (CAP-02). No-op unless FNDR_METRICS_DUMP names a file.
            fndr_lib::telemetry::metrics_dump::spawn_if_enabled(state.clone());

            // Background task: Track downloads folder
            let uploads_state = state.clone();
            tauri::async_runtime::spawn(async move {
                fndr_lib::downloads::run_watcher(uploads_state).await;
            });

            // Background task: Ebbinghaus decay — runs every 6 hours.
            {
                let decay_store = state.store.clone();
                let decay_half_life = state.config.read().decay_half_life_days;
                tauri::async_runtime::spawn(async move {
                    let mut interval = tokio::time::interval(DECAY_INTERVAL);
                    interval.tick().await; // skip first immediate tick
                    loop {
                        interval.tick().await;
                        let now_ms = chrono::Utc::now().timestamp_millis();
                        let cutoff = now_ms - DECAY_LOOKBACK_MS;
                        let range_result = decay_store
                            .get_memories_in_range(0, cutoff)
                            .await
                            .map_err(|e| e.to_string());
                        match range_result {
                            Ok(records) => {
                                let updates: Vec<(String, f32)> = records
                                    .iter()
                                    .map(|r| {
                                        let days_since =
                                            (now_ms - r.last_accessed_at.max(r.timestamp)) as f64
                                                / MS_PER_DAY;
                                        let new_decay = (r.decay_score as f64
                                            * 0.5_f64.powf(days_since / decay_half_life as f64))
                                            .max(DECAY_FLOOR as f64)
                                            as f32;
                                        (r.id.clone(), new_decay)
                                    })
                                    .collect();
                                let count = updates.len();
                                match decay_store.apply_decay_batch(&updates).await {
                                    Ok(()) => tracing::info!("Decay job applied {count} updates"),
                                    Err(e) => tracing::warn!("Decay batch failed: {e}"),
                                }
                            }
                            Err(e) => tracing::warn!("Decay job query failed: {e}"),
                        }
                    }
                });
            }

            // Background task: proactive surface — runs every 30 seconds.
            {
                let proactive_state = state.clone();
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let proactive_config = proactive_state.config.read().proactive.clone();
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                        proactive_config.interval_secs,
                    ));
                    let mut seen_ring: std::collections::VecDeque<String> =
                        std::collections::VecDeque::with_capacity(
                            proactive_config.seen_ring_capacity,
                        );
                    interval.tick().await; // skip first tick
                    loop {
                        interval.tick().await;

                        let config = proactive_state.config.read().clone();
                        if !config.proactive_surface_enabled {
                            continue;
                        }

                        let embedding = proactive_state.last_embedding.read().clone();
                        if embedding.is_empty() {
                            continue;
                        }

                        let hits = match proactive_state
                            .store
                            .vector_search(
                                &embedding,
                                config.proactive.search_limit,
                                Some(&config.proactive.lookback_filter),
                                None,
                            )
                            .await
                        {
                            Ok(h) => h,
                            Err(e) => {
                                tracing::debug!("Proactive surface search failed: {e}");
                                continue;
                            }
                        };

                        // The toast obeys the rules every other surface does:
                        // current blocklist, quality gate, a real sentence.
                        let ids = hits.iter().map(|hit| hit.id.clone()).collect::<Vec<_>>();
                        let Ok(records) = proactive_state.store.get_memories_by_ids(&ids).await
                        else {
                            continue;
                        };
                        let suggestion = fndr_lib::context_runtime::proactive::pick_suggestion(
                            &hits,
                            &records,
                            &config.blocklist,
                            config.proactive.similarity_threshold,
                            |id| seen_ring.iter().any(|seen| seen == id),
                            chrono::Utc::now().timestamp_millis(),
                        );

                        if let Some(suggestion) = suggestion {
                            if seen_ring.len() >= config.proactive.seen_ring_capacity {
                                seen_ring.pop_front();
                            }
                            seen_ring.push_back(suggestion.memory_id.clone());

                            let _ = proactive_state.proactive_tx.send(Some(suggestion.clone()));
                            let _ = app_handle.emit("proactive_suggestion", suggestion);
                        }
                    }
                });
            }

            // Background task: clipboard watcher — indexes clipboard copies into the graph.
            {
                let clip_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    fndr_lib::capture::clipboard::run_clipboard_watcher(clip_state).await;
                });
            }

            // Background task: proactive intelligence — morning/evening briefings
            // and stale task nudges delivered as system notifications.
            {
                let notif_state = state.clone();
                let notif_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(PROACTIVE_NOTIFICATION_STARTUP_DELAY).await;

                    let mut briefing_sent_today = false;
                    let mut last_stale_check =
                        std::time::Instant::now() - STALE_TASK_CHECK_INTERVAL;
                    let mut last_context_switch_check = std::time::Instant::now();
                    let mut recent_app_switches: std::collections::VecDeque<String> =
                        std::collections::VecDeque::with_capacity(APP_SWITCH_RECENT_CAPACITY);

                    let mut signal_clock = fndr_lib::proactive_signals::SignalClock::default();

                    let mut interval = tokio::time::interval(PROACTIVE_NOTIFICATION_TICK);
                    loop {
                        interval.tick().await;

                        // Stuck detector, thread updates and meeting prep:
                        // each on its own cadence, switch and daily limit.
                        fndr_lib::proactive_signals::run_due(
                            &notif_handle,
                            &notif_state,
                            &mut signal_clock,
                        )
                        .await;

                        let now = chrono::Local::now();
                        let hour = now.hour();

                        // ── Morning/evening briefing ──────────────────────────
                        // Send a briefing notification once per day.
                        // Morning (8-10am) or evening (6-8pm).
                        if !briefing_sent_today
                            && ((8..=10).contains(&hour) || (18..=20).contains(&hour))
                        {
                            let mode = if hour < 12 { "morning" } else { "evening" };
                            // Written without a model, from the same code as
                            // the To-dos briefing. Empty when the window holds
                            // too little to say, and then nothing is sent.
                            let briefing =
                                fndr_lib::briefing::briefing_for(&notif_state, mode, now).await;
                            if !briefing.trim().is_empty() {
                                let title = if mode == "morning" {
                                    "☀️ FNDR Morning Briefing"
                                } else {
                                    "🌙 FNDR Evening Recap"
                                };
                                fndr_lib::notify::send(
                                    &notif_handle,
                                    &notif_state,
                                    "briefing",
                                    title,
                                    &briefing,
                                );
                                tracing::info!("Sent {} briefing notification", mode);
                                briefing_sent_today = true;
                            }
                        }

                        // Reset flag at midnight
                        if hour == 0 {
                            briefing_sent_today = false;
                        }

                        // ── Stale task nudge ──────────────────────────────────
                        // Every STALE_TASK_CHECK_INTERVAL, surface tasks older than
                        // STALE_TASK_THRESHOLD_MS.
                        if last_stale_check.elapsed() > STALE_TASK_CHECK_INTERVAL {
                            last_stale_check = std::time::Instant::now();
                            if let Ok(tasks) = notif_state.store.list_tasks().await {
                                let stale_cutoff =
                                    chrono::Utc::now().timestamp_millis() - STALE_TASK_THRESHOLD_MS;
                                let stale: Vec<_> = tasks
                                    .iter()
                                    .filter(|t| {
                                        !t.is_completed
                                            && !t.is_dismissed
                                            && t.created_at < stale_cutoff
                                    })
                                    .collect();

                                if !stale.is_empty() {
                                    let titles: Vec<_> = stale
                                        .iter()
                                        .take(STALE_TASK_TITLES_SHOWN)
                                        .map(|t| t.title.as_str())
                                        .collect();
                                    let body = format!(
                                        "You have {} stale tasks: {}",
                                        stale.len(),
                                        titles.join(", ")
                                    );
                                    fndr_lib::notify::send(
                                        &notif_handle,
                                        &notif_state,
                                        "stale_tasks",
                                        "📋 Stale Tasks",
                                        &body,
                                    );
                                }
                            }
                        }

                        // ── Context-switch alert ─────────────────────────────
                        // Sample the frontmost app at APP_SWITCH_SAMPLE_INTERVAL and
                        // surface a nudge when too many distinct apps appear in the
                        // rolling window.
                        if last_context_switch_check.elapsed() > APP_SWITCH_SAMPLE_INTERVAL {
                            last_context_switch_check = std::time::Instant::now();
                            let current_app = fndr_lib::capture::macos_frontmost_app_name();
                            if let Some(app) = current_app {
                                let last = recent_app_switches.back().cloned();
                                if last.as_deref() != Some(&app) {
                                    recent_app_switches.push_back(app);
                                    if recent_app_switches.len() > APP_SWITCH_WINDOW {
                                        recent_app_switches.pop_front();
                                    }
                                }
                            }
                            if recent_app_switches.len() >= APP_SWITCH_THRESHOLD {
                                let unique: std::collections::HashSet<_> =
                                    recent_app_switches.iter().collect();
                                if unique.len() >= APP_SWITCH_UNIQUE_THRESHOLD {
                                    fndr_lib::notify::send(
                                        &notif_handle,
                                        &notif_state,
                                        "context_switch",
                                        "🔀 High Context Switching",
                                        "You've been switching between many apps. Want to refocus?",
                                    );
                                    recent_app_switches.clear();
                                }
                            }
                        }
                    }
                });
            }

            app.manage(state.clone());
            app.manage(fndr_lib::voice::VoiceManager::for_app(app.handle().clone()));

            // Restore the local MCP endpoint only when this app connected a client.
            {
                let mcp_app = app.handle().clone();
                let mcp_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = ipc::commands::resume_if_connected(mcp_app, mcp_state).await {
                        tracing::warn!(%error, "mcp:resume_failed");
                    }
                });
            }
            fndr_lib::privacy::recording::create_indicator(app.handle(), state.as_ref());
            if let Err(error) = fndr_lib::privacy::recording::register_pause_shortcut(app.handle()) {
                tracing::warn!("Capture pause shortcut registration failed: {error}");
            }

            // The Companion API (iPhone/Watch local-network surface) starts at
            // launch only once a device is paired; otherwise nothing listens
            // on the local network until the person starts it from the Mac
            // (`companion_start_server`). TLS init runs off the startup path.
            if fndr_lib::companion::should_start_at_launch(&state) {
                let companion_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    match fndr_lib::companion::start(companion_state, None, None).await {
                        Ok(s) => tracing::info!(
                            host = %s.host,
                            port = s.port,
                            tls = s.tls,
                            "Companion API ready"
                        ),
                        Err(err) => tracing::warn!("Companion API failed to start: {}", err),
                    }
                });
            }

            if let Err(err) =
                fndr_lib::meeting::bind_runtime(app.handle().clone(), runtime_state.clone())
            {
                tracing::warn!("Meeting runtime initialization failed: {}", err);
            }

            // Pre-create the full-screen, click-through Screen Guide overlay
            // before shortcuts can make it visible.
            ipc::commands::create_screen_guide_notch_companion(app.handle());
            ipc::commands::create_screen_guide_overlay_window(app.handle());

            // The command bar (Alt+Space): memory search, and a request to do
            // something handed to Notch Do. It was off for the Alpha demo
            // (2026-09-18); back on as the command bar (2026-10-14). Auto-Fill
            // stays off.
            ipc::commands::create_omnibar_window(app.handle());
            if let Err(err) = ipc::commands::register_omnibar_shortcut(app.handle()) {
                tracing::warn!("Command bar shortcut registration failed: {err}");
            } else {
                tracing::info!(
                    "Command bar global shortcut registered: {}",
                    ipc::commands::OMNIBAR_SHORTCUT
                );
            }

            // Smart Fill: the glow window, and the Command watch if it is on.
            fndr_lib::smart_fill::overlay::create(app.handle());
            fndr_lib::smart_fill::apply_setting(
                app.handle(),
                state.config.read().autofill.smart_fill,
            );

            // Pre-create the notch HUD (hidden) and register its toggle.
            ipc::commands::create_notch_hud_window(app.handle());
            if let Err(err) = ipc::commands::register_notch_hud_shortcut(app.handle()) {
                tracing::warn!("Notch HUD shortcut registration failed: {err}");
            } else {
                tracing::info!(
                    "Notch HUD global shortcut registered: {}",
                    ipc::commands::NOTCH_HUD_SHORTCUT
                );
            }

            if let Err(err) = ipc::commands::register_screen_guide_shortcut(
                app.handle(),
                &state.config.read().screen_guide.clone(),
            ) {
                tracing::warn!("Screen Guide shortcut registration failed: {err}");
                if let Err(save_err) =
                    ipc::commands::reconcile_screen_guide_startup_failure(state.as_ref())
                {
                    tracing::warn!(
                        "Screen Guide could not persist its disabled startup state: {save_err}"
                    );
                }
            } else if state.config.read().screen_guide.enabled {
                tracing::info!(
                    "Screen Guide global shortcut registered: {}",
                    state.config.read().screen_guide.shortcut
                );
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::search::search,
            ipc::commands::search::search_raw_results,
            ipc::commands::search::search_memory_cards,
            ipc::commands::search::list_memory_cards,
            ipc::commands::search::list_needs_signal_memory_cards,
            ipc::commands::search::summarize_search,
            // FNDR agentic-graph-rag namespace (Phase 4)
            ipc::commands::retrieval::fndr_search,
            ipc::commands::retrieval::fndr_answer,
            ipc::commands::retrieval::fndr_build_context_pack,
            ipc::commands::retrieval::fndr_get_related_memories,
            ipc::commands::retrieval::fndr_get_memory_source_statements,
            ipc::commands::retrieval::fndr_quality_status,
            ipc::commands::retrieval::fndr_timeline,
            ipc::commands::search::find_visually_similar_memories,
            ipc::commands::get_fun_greeting,
            ipc::commands::get_status,
            ipc::commands::get_memory_review_status,
            ipc::commands::run_daily_memory_review_cmd,
            ipc::commands::backfill_memory_review,
            // MCP
            ipc::commands::get_mcp_server_status,
            ipc::commands::start_mcp_server,
            ipc::commands::stop_mcp_server,
            ipc::commands::resolve_mcp_approval,
            ipc::commands::mcp_connect_status,
            ipc::commands::mcp_connect_preview,
            ipc::commands::mcp_connect_apply,
            ipc::commands::mcp_connect_test,
            ipc::commands::mcp_connect_rotate_token,
            ipc::commands::mcp_connect_disconnect,
            ipc::commands::mcp_connect_revoke,
            ipc::commands::mcp_connect_set_keep_running,
            ipc::commands::mcp_connect_set_hide_more,
            ipc::commands::get_action_settings,
            ipc::commands::set_action_settings,
            ipc::commands::list_action_approvals,
            ipc::commands::resolve_action_approval,
            // Companion API (iPhone / Apple Watch)
            ipc::commands::companion_get_status,
            ipc::commands::companion_get_endpoint,
            ipc::commands::companion_start_server,
            ipc::commands::companion_stop_server,
            ipc::commands::companion_start_pairing,
            ipc::commands::companion_list_devices,
            ipc::commands::companion_revoke_device,
            ipc::commands::get_context_runtime_status,
            ipc::commands::list_recent_context_packs,
            ipc::commands::fndr_subscribe,
            ipc::commands::fndr_unsubscribe,
            // Meetings
            ipc::commands::get_meeting_status,
            ipc::commands::start_meeting_recording,
            ipc::commands::stop_meeting_recording,
            ipc::commands::list_meetings,
            ipc::commands::delete_meeting,
            ipc::commands::get_meeting_transcript,
            ipc::commands::retranscribe_meeting,
            ipc::commands::export_daily_summary_pdf,
            ipc::commands::export_weekly_wrapped_pdf,
            ipc::commands::open_exported_pdf,
            // Voice / Speech
            ipc::commands::transcribe_voice_input,
            fndr_lib::voice::voice_start,
            fndr_lib::voice::voice_stop,
            fndr_lib::voice::voice_cancel,
            fndr_lib::voice::notch_do::get_notch_do_muted,
            fndr_lib::voice::notch_do::set_notch_do_muted,
            fndr_lib::voice::notch_do::get_notch_do_autonomy,
            fndr_lib::voice::notch_do::set_notch_do_autonomy,
            fndr_lib::voice::notch_do::set_notch_escape_monitor,
            // Capture control
            ipc::commands::pause_capture,
            ipc::commands::resume_capture,
            // Privacy & data
            ipc::commands::get_agent_notes_enabled,
            ipc::commands::set_agent_notes_enabled,
            ipc::commands::get_verbatim_memory_enabled,
            ipc::commands::set_verbatim_memory_enabled,
            ipc::commands::get_blocklist,
            ipc::commands::set_blocklist,
            ipc::commands::delete_all_data,
            ipc::commands::delete_memory,
            ipc::commands::delete_memories_in_range,
            ipc::commands::export_memories_archive,
            ipc::commands::import_memories_archive,
            ipc::commands::get_encryption_statement,
            ipc::commands::get_privacy_exclusions,
            ipc::commands::reopen_memory,
            ipc::commands::get_stats,
            ipc::commands::get_runtime_metrics,
            ipc::commands::get_privacy_proof,
            ipc::commands::resume_work,
            ipc::commands::what_changed_since,
            ipc::commands::mark_thread_seen,
            ipc::commands::calendar_meeting_prep_state,
            ipc::commands::set_calendar_meeting_prep,
            ipc::commands::open_calendar_privacy_settings,
            ipc::commands::next_meeting_brief,
            ipc::commands::get_retention_days,
            ipc::commands::set_retention_days,
            ipc::commands::delete_older_than,
            ipc::commands::import_list_sources,
            ipc::commands::import_get_consent,
            ipc::commands::import_set_consent,
            ipc::commands::import_record_first_answer,
            ipc::commands::import_first_answer_ms,
            ipc::commands::import_start,
            ipc::commands::import_status,
            ipc::commands::import_cancel,
            ipc::commands::import_forget,
            ipc::commands::get_app_names,
            ipc::commands::get_storage_health,
            ipc::commands::clean_dev_build_cache,
            ipc::commands::run_memory_repair_backfill,
            ipc::commands::reindex_memories_v5,
            ipc::commands::get_memory_repair_progress,
            ipc::commands::get_memory_debug_inspector,
            ipc::commands::debug::inspect_memory_pipeline,
            ipc::commands::debug::get_memory_timeline_thread,
            ipc::commands::evaluate_recent_memory_quality,
            ipc::commands::rebuild_memory_context_for_range,
            ipc::commands::backfill_insight_layers_for_range,
            ipc::commands::run_memory_retrieval_eval,
            ipc::commands::get_storage_reclaim_progress,
            ipc::commands::reclaim_memory_storage,
            ipc::commands::run_idle_wiki_knowledge_compile,
            ipc::commands::get_graph_for_project,
            ipc::commands::get_full_graph,
            ipc::commands::get_node_detail,
            ipc::commands::find_graph_path,
            ipc::commands::get_god_nodes,
            ipc::commands::backfill_graph_from_existing_memories,
            ipc::commands::get_privacy_alerts,
            ipc::commands::dismiss_privacy_alert,
            ipc::commands::add_to_blocklist,
            // Tasks / Todos
            ipc::commands::add_todo,
            ipc::commands::get_todos,
            ipc::commands::update_todo,
            ipc::commands::dismiss_todo,
            ipc::commands::complete_todo,
            ipc::commands::set_todo_completed,
            // Agent SDK
            ipc::commands::get_hermes_bridge_status,
            ipc::commands::install_hermes_bridge,
            ipc::commands::save_hermes_setup,
            ipc::commands::send_hermes_message,
            ipc::commands::cancel_hermes_message,
            ipc::commands::list_configured_peers,
            ipc::commands::add_configured_peer,
            ipc::commands::remove_configured_peer,
            ipc::commands::preview_peer_delegation,
            ipc::commands::send_peer_delegation,
            ipc::commands::list_peer_runs,
            ipc::commands::refresh_peer_task,
            ipc::commands::cancel_peer_task,
            ipc::commands::list_agent_chats,
            ipc::commands::get_agent_chat,
            ipc::commands::delete_agent_chat,
            ipc::commands::codex_account_status,
            ipc::commands::codex_login_start,
            ipc::commands::codex_login_cancel,
            ipc::commands::codex_logout,
            ipc::commands::voice_out_status,
            ipc::commands::voice_out_voices,
            ipc::commands::voice_out_start,
            ipc::commands::voice_out_speak,
            ipc::commands::voice_out_cancel,
            ipc::commands::voice_out_stop,
            ipc::commands::tts_status,
            ipc::commands::tts_voices,
            ipc::commands::tts_install,
            ipc::commands::tts_remove,
            ipc::commands::tts_cancel,
            ipc::commands::tts_synthesize,
            ipc::commands::openclicky_bridge_status,
            ipc::commands::computer_use_status,
            ipc::commands::set_computer_use_enabled,
            ipc::commands::computer_use_plan,
            ipc::commands::setup_components,
            ipc::commands::install_component,
            ipc::commands::computer_use_permissions,
            ipc::commands::computer_use_start,
            ipc::commands::notch_do_rate_run,
            ipc::commands::notch_do_save_skill,
            ipc::commands::notch_do_runs,
            ipc::commands::notch_do_skills,
            ipc::commands::notch_do_delete_skill,
            ipc::commands::notch_do_request,
            ipc::commands::computer_use_respond,
            ipc::commands::computer_use_stop,
            ipc::commands::resolve_work_set,
            ipc::commands::hand_off_status,
            ipc::commands::hand_off_install_toolchain,
            ipc::commands::hand_off_draft,
            ipc::commands::hand_off_update_draft,
            ipc::commands::hand_off_start,
            ipc::commands::hand_off_stop,
            ipc::commands::hand_off_open,
            ipc::commands::hand_off_rate,
            ipc::commands::open_work_set,
            ipc::commands::list_named_sets,
            ipc::commands::save_named_set,
            ipc::commands::delete_named_set,
            ipc::commands::routine_offers,
            ipc::commands::dismiss_routine_offer,
            ipc::commands::generate_daily_briefing,
            ipc::commands::generate_daily_output,
            ipc::commands::render_daily_output,
            ipc::commands::save_daily_output,
            ipc::commands::output_destination_status,
            ipc::commands::set_output_destination,
            ipc::commands::propose_daily_output_send,
            ipc::commands::resolve_daily_output_send,
            ipc::commands::send_daily_output_allowed,
            ipc::commands::trust_output_destination,
            ipc::commands::list_output_allowed_destinations,
            ipc::commands::forget_output_allowed_destination,
            ipc::commands::generate_daily_summary_for_date,
            ipc::commands::generate_daily_summary_narrative,
            ipc::commands::get_daily_summary_followups,
            ipc::commands::get_weekly_wrapped,
            // Time tracking & Focus Mode
            ipc::commands::get_time_tracking,
            ipc::commands::set_focus_task,
            ipc::commands::get_focus_status,
            // Auto-fill
            fndr_lib::smart_fill::get_smart_fill_enabled,
            fndr_lib::smart_fill::saved_details_count,
            fndr_lib::smart_fill::forget_saved_details,
            fndr_lib::smart_fill::set_smart_fill_enabled,
            ipc::commands::get_autofill_settings,
            ipc::commands::set_autofill_settings,
            ipc::commands::get_voice_output_settings,
            ipc::commands::set_voice_output_settings,
            ipc::commands::set_autofill_overlay_ready,
            ipc::commands::take_pending_autofill_payload,
            ipc::commands::resolve_autofill,
            ipc::commands::inject_text,
            ipc::commands::dismiss_autofill,
            // Omnibar
            ipc::commands::dismiss_omnibar,
            ipc::commands::omnibar_open_memory,
            // Notch HUD
            ipc::commands::get_notch_hud_geometry,
            ipc::commands::set_notch_hud_hit_rect,
            ipc::commands::set_notch_hud_keyboard,
            ipc::commands::dismiss_notch_hud,
            ipc::commands::toggle_notch_hud,
            ipc::commands::notch_hud_open_memory,
            // Screen Guide
            ipc::commands::get_screen_guide_settings,
            ipc::commands::set_screen_guide_settings,
            ipc::commands::screen_guide_press,
            ipc::commands::screen_guide_release,
            ipc::commands::submit_screen_guide_text,
            ipc::commands::cancel_screen_guide_turn,
            ipc::commands::transcribe_screen_guide_voice_input,
            ipc::commands::report_screen_guide_state,
            ipc::commands::set_screen_guide_overlay_ready,
            ipc::commands::screen_guide_microphone_started,
            ipc::commands::acknowledge_screen_guide_microphone_stopped,
            ipc::commands::ask_screen_guide,
            ipc::commands::get_screen_guide_cursor_position,
            ipc::commands::finish_screen_guide_visual,
            ipc::commands::arm_screen_guide_diagnostic,
            ipc::commands::get_screen_guide_diagnostic_status,
            ipc::commands::delete_screen_guide_diagnostics,
            ipc::commands::reveal_screen_guide_diagnostics,
            // Debug-only Memory Journey evidence. These command symbols are
            // compiled out of release builds together with their recorder.
            #[cfg(debug_assertions)]
            ipc::commands::arm_memory_journey,
            #[cfg(debug_assertions)]
            ipc::commands::get_memory_journey_status,
            #[cfg(debug_assertions)]
            ipc::commands::create_reconstructed_memory_journey,
            #[cfg(debug_assertions)]
            ipc::commands::run_memory_journey_query,
            #[cfg(debug_assertions)]
            ipc::commands::export_memory_journey,
            #[cfg(debug_assertions)]
            ipc::commands::delete_memory_journey,
            #[cfg(debug_assertions)]
            ipc::commands::delete_all_memory_journeys,
            // Synthetic image replay is debug-only and refuses any profile
            // without the explicit Quality Lab marker.
            #[cfg(debug_assertions)]
            ipc::commands::get_quality_lab_fixtures,
            #[cfg(debug_assertions)]
            ipc::commands::replay_quality_lab_fixture,
            // Clipboard history
            ipc::commands::get_clipboard_history,
            ipc::commands::copy_clipboard_entry,
            ipc::commands::paste_clipboard_entry,
            // Onboarding
            ipc::onboarding::get_onboarding_state,
            ipc::onboarding::save_onboarding_state,
            ipc::onboarding::request_biometric_auth,
            ipc::onboarding::check_permissions,
            ipc::onboarding::open_system_settings,
            ipc::onboarding::list_available_models,
            ipc::onboarding::download_model,
            ipc::onboarding::get_model_download_status,
            ipc::onboarding::refresh_ai_models,
            ipc::onboarding::get_apple_model_status,
            ipc::onboarding::set_apple_model_enabled,
            ipc::onboarding::check_model_exists,
            ipc::onboarding::delete_ai_model,
            ipc::onboarding::set_preferred_inference_model,
            ipc::commands::import_meta_glasses_photo,
            ipc::commands::models_cleanup_dry_run,
            ipc::commands::models_cleanup_confirm,
            // Graph projection layer for 3D visualization
            fndr_lib::graph::projection_commands::get_memory_graph_atlas,
            fndr_lib::graph::projection_commands::get_memory_graph_context,
            fndr_lib::graph::projection_commands::get_graph_node_neighborhood,
            fndr_lib::graph::projection_commands::get_graph_communities,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            ipc::commands::shutdown_screen_guide(app_handle);
            ipc::commands::shutdown_computer_use();
            ipc::commands::shutdown_hermes_gateway();
            fndr_lib::voice::shutdown(app_handle);
            fndr_lib::speech::shutdown_speech();
        }
    });
}
