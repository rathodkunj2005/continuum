//! FNDR - Privacy-first local memory search
//!
//! Main entry point for the Tauri application.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Timelike;
use fndr_lib::{
    capture, config::Config, graph::GraphStore, ipc, models, storage::Store, AppState,
    ProactiveSuggestion,
};
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
const BRIEFING_MIN_MEMORIES: usize = 3;
const BRIEFING_MAX_CARD_LINES: usize = 20;
const GRAPH_COMMIT_INTERVAL: Duration = Duration::from_secs(90);
const MEMORY_REVIEW_INTERVAL: Duration = Duration::from_secs(45);

fn main() {
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

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
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
            let data_dir = app.path().app_data_dir()?;
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
                None,
            ));
            state.set_app_handle(app.handle().clone());

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
                    restore_state.replace_ai_engines(loaded.inference, loaded.vlm);
                });
            }

            // Start capture pipeline
            let capture_state = state.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    if let Err(e) = capture::run_capture_loop(capture_state).await {
                        tracing::error!("Capture loop error: {}", e);
                    }
                });
            });

            // Background task: compact legacy capture payloads and purge stray artifacts.
            {
                let maintenance_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(MAINTENANCE_FIRST_DELAY).await;
                    loop {
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

                        let suggestion = hits.into_iter().find(|r| {
                            r.score > config.proactive.similarity_threshold
                                && !seen_ring.contains(&r.id)
                        });

                        if let Some(hit) = suggestion {
                            // Find linked task title from graph
                            let task_title = None::<String>;

                            let suggestion = ProactiveSuggestion {
                                memory_id: hit.id.clone(),
                                snippet: hit.snippet.clone(),
                                similarity: hit.score,
                                task_title,
                            };

                            if seen_ring.len() >= config.proactive.seen_ring_capacity {
                                seen_ring.pop_front();
                            }
                            seen_ring.push_back(hit.id.clone());

                            let _ = proactive_state.proactive_tx.send(Some(suggestion.clone()));
                            let _ = app_handle.emit("proactive_suggestion", suggestion);
                        }
                    }
                });
            }

            // Background task: clipboard watcher — indexes clipboard copies into the graph.
            {
                let clip_store = state.store.clone();
                tauri::async_runtime::spawn(async move {
                    fndr_lib::capture::clipboard::run_clipboard_watcher(clip_store).await;
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

                    let mut interval = tokio::time::interval(PROACTIVE_NOTIFICATION_TICK);
                    loop {
                        interval.tick().await;

                        let now = chrono::Local::now();
                        let hour = now.hour();

                        // ── Morning/evening briefing ──────────────────────────
                        // Send a briefing notification once per day.
                        // Morning (8-10am) or evening (6-8pm).
                        if !briefing_sent_today
                            && ((8..=10).contains(&hour) || (18..=20).contains(&hour))
                        {
                            let mode = if hour < 12 { "morning" } else { "evening" };
                            if let Some(engine) = notif_state.inference_engine() {
                                // Gather today's memory snippets for briefing
                                let start_of_day = now
                                    .date_naive()
                                    .and_hms_opt(0, 0, 0)
                                    .map(|t| t.and_utc().timestamp_millis())
                                    .unwrap_or(0);
                                let now_ms = chrono::Utc::now().timestamp_millis();
                                let memories = notif_state
                                    .store
                                    .get_memories_in_range(start_of_day, now_ms)
                                    .await
                                    .unwrap_or_default();

                                if memories.len() >= BRIEFING_MIN_MEMORIES {
                                    let card_lines: Vec<String> = memories
                                        .iter()
                                        .take(BRIEFING_MAX_CARD_LINES)
                                        .map(|m| {
                                            format!(
                                                "[{}] {} — {}",
                                                m.app_name, m.window_title, m.snippet
                                            )
                                        })
                                        .collect();

                                    let briefing = engine
                                        .generate_daily_briefing(&card_lines, mode)
                                        .await;

                                    if !briefing.trim().is_empty() {
                                        let title = if mode == "morning" {
                                            "☀️ FNDR Morning Briefing"
                                        } else {
                                            "🌙 FNDR Evening Recap"
                                        };
                                        let _ = notif_handle.emit(
                                            "fndr_notification",
                                            serde_json::json!({
                                                "title": title,
                                                "body": briefing,
                                                "kind": "briefing",
                                            }),
                                        );
                                        tracing::info!(
                                            "Sent {} briefing notification",
                                            mode
                                        );
                                        briefing_sent_today = true;
                                    }
                                }
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
                                    let _ = notif_handle.emit(
                                        "fndr_notification",
                                        serde_json::json!({
                                            "title": "📋 Stale Tasks",
                                            "body": body,
                                            "kind": "stale_tasks",
                                        }),
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
                                    let _ = notif_handle.emit(
                                        "fndr_notification",
                                        serde_json::json!({
                                            "title": "🔀 High Context Switching",
                                            "body": "You've been switching between many apps. Want to refocus?",
                                            "kind": "context_switch",
                                        }),
                                    );
                                    recent_app_switches.clear();
                                }
                            }
                        }
                    }
                });
            }

            app.manage(state.clone());

            if let Err(err) =
                fndr_lib::meeting::bind_runtime(app.handle().clone(), runtime_state.clone())
            {
                tracing::warn!("Meeting runtime initialization failed: {}", err);
            }

            // Pre-create the autofill overlay window so it's loaded and ready
            // by the time the user first presses the hotkey.
            ipc::commands::create_autofill_overlay_window(app.handle());

            if let Err(err) = ipc::commands::register_autofill_shortcut(
                app.handle(),
                &state.config.read().autofill.clone(),
            ) {
                tracing::warn!("Auto-fill shortcut registration failed: {err}");
            } else {
                tracing::info!(
                    "Auto-fill global shortcut registered: {}",
                    state.config.read().autofill.shortcut
                );
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::search::search,
            ipc::commands::search::search_raw_results,
            ipc::commands::search::search_memory_cards,
            ipc::commands::search::list_memory_cards,
            ipc::commands::search::summarize_search,
            // FNDR agentic-graph-rag namespace (Phase 4)
            ipc::commands::retrieval::fndr_search,
            ipc::commands::retrieval::fndr_answer,
            ipc::commands::retrieval::fndr_build_context_pack,
            ipc::commands::retrieval::fndr_get_memory_subgraph,
            ipc::commands::retrieval::fndr_get_related_memories,
            ipc::commands::retrieval::fndr_quality_status,
            ipc::commands::retrieval::fndr_timeline,
            ipc::commands::search::find_visually_similar_memories,
            ipc::commands::get_fun_greeting,
            ipc::commands::get_status,
            // MCP
            ipc::commands::get_mcp_server_status,
            ipc::commands::start_mcp_server,
            ipc::commands::stop_mcp_server,
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
            ipc::commands::open_exported_pdf,
            // Voice / Speech
            ipc::commands::transcribe_voice_input,
            // Capture control
            ipc::commands::pause_capture,
            ipc::commands::resume_capture,
            // Privacy & data
            ipc::commands::get_blocklist,
            ipc::commands::set_blocklist,
            ipc::commands::delete_all_data,
            ipc::commands::delete_memory,
            ipc::commands::reopen_memory,
            ipc::commands::get_stats,
            ipc::commands::get_runtime_metrics,
            ipc::commands::get_retention_days,
            ipc::commands::set_retention_days,
            ipc::commands::delete_older_than,
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
            ipc::commands::search_graph,
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
            // Agent SDK
            ipc::commands::start_agent_task,
            ipc::commands::get_agent_status,
            ipc::commands::stop_agent,
            ipc::commands::build_agent_context_pack,
            ipc::commands::run_agent_request,
            ipc::commands::list_agent_audit_runs,
            ipc::commands::get_agent_audit_run,
            ipc::commands::explain_agent_retrieval,
            ipc::commands::rate_agent_result,
            ipc::commands::propose_skill_from_run,
            ipc::commands::list_agent_skill_drafts,
            ipc::commands::propose_eval_from_run,
            ipc::commands::list_agent_eval_drafts,
            ipc::commands::list_agent_prompts,
            ipc::commands::get_agent_prompt,
            ipc::commands::get_hermes_bridge_status,
            ipc::commands::install_hermes_bridge,
            ipc::commands::save_hermes_setup,
            ipc::commands::sync_hermes_bridge_context,
            ipc::commands::start_hermes_gateway,
            ipc::commands::stop_hermes_gateway,
            ipc::commands::send_hermes_message,
            ipc::commands::send_direct_chat,
            ipc::commands::quick_setup_ollama,
            ipc::commands::generate_daily_briefing,
            ipc::commands::generate_daily_summary_for_date,
            // Time tracking & Focus Mode
            ipc::commands::get_time_tracking,
            ipc::commands::set_focus_task,
            ipc::commands::get_focus_status,
            // Auto-fill
            ipc::commands::get_autofill_settings,
            ipc::commands::set_autofill_settings,
            ipc::commands::set_autofill_overlay_ready,
            ipc::commands::take_pending_autofill_payload,
            ipc::commands::resolve_autofill,
            ipc::commands::inject_text,
            ipc::commands::dismiss_autofill,
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
