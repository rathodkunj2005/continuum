//! FNDR - Privacy-first local memory search
//!
//! Main entry point for the Tauri application.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Timelike;
use fndr_lib::{
    api, capture, config::Config, graph::GraphStore, store::Store, AppState, ProactiveSuggestion,
};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() {
    // Install default TLS crypto provider (required by rustls 0.23+)
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Load environment variables from .env if present
    let _ = dotenvy::dotenv();

    // Initialize logging
    use tracing_subscriber::{fmt, EnvFilter};
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "fndr=info,fndr_lib=info".into()),
        )
        .with(fmt::layer())
        .init();

    tracing::info!("Starting FNDR...");

    // Build a tokio runtime with 8 MB per worker thread (default is 2 MB).
    // Deep async chains through LanceDB, whisper transcription, and embedding
    // can overflow the default stack size, causing SIGABRT on tokio-rt-worker threads.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(8 * 1024 * 1024)
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

            // Initialize store (LanceDB)
            let data_dir = app.path().app_data_dir()?;
            let store = Store::new(&data_dir)?;
            let store_arc = Arc::new(store);
            tracing::info!("Consolidated store initialized at {:?}", data_dir);
            let state_store = Arc::new(fndr_lib::store::StateStore::new(&data_dir)?);
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

            tracing::info!("AI runtime will load lazily when FNDR first needs it");

            // Create app state
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
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    loop {
                        match api::commands::reclaim_memory_storage_silent(maintenance_state.clone()).await {
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
                        tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
                    }
                });
            }

            let runtime_state = state.clone();

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
                    let mut interval =
                        tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
                    interval.tick().await; // skip first immediate tick
                    loop {
                        interval.tick().await;
                        let now_ms = chrono::Utc::now().timestamp_millis();
                        let cutoff = now_ms - 24 * 3600 * 1000;
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
                                                / 86_400_000.0;
                                        let new_decay = (r.decay_score as f64
                                            * 0.5_f64.powf(days_since / decay_half_life as f64))
                                            .max(0.15) as f32;
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
                    use std::time::Duration;

                    // Wait 60s at startup before checking — let the system settle.
                    tokio::time::sleep(Duration::from_secs(60)).await;

                    let mut briefing_sent_today = false;
                    let mut last_stale_check =
                        std::time::Instant::now() - Duration::from_secs(7200);
                    let mut last_context_switch_check = std::time::Instant::now();
                    let mut recent_app_switches: std::collections::VecDeque<String> =
                        std::collections::VecDeque::with_capacity(20);

                    let mut interval = tokio::time::interval(Duration::from_secs(30));
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

                                if memories.len() >= 3 {
                                    let card_lines: Vec<String> = memories
                                        .iter()
                                        .take(20)
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
                        // Every 2 hours, check for tasks > 3 days old.
                        if last_stale_check.elapsed() > Duration::from_secs(7200) {
                            last_stale_check = std::time::Instant::now();
                            if let Ok(tasks) = notif_state.store.list_tasks().await {
                                let stale_cutoff =
                                    chrono::Utc::now().timestamp_millis() - 3 * 86_400_000;
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
                                        .take(3)
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
                        // Track app switches. If > 8 switches in 5 minutes,
                        // nudge the user about context switching.
                        if last_context_switch_check.elapsed() > Duration::from_secs(10) {
                            last_context_switch_check = std::time::Instant::now();
                            let current_app = fndr_lib::capture::macos_frontmost_app_name();
                            if let Some(app) = current_app {
                                let last = recent_app_switches.back().cloned();
                                if last.as_deref() != Some(&app) {
                                    recent_app_switches.push_back(app);
                                    if recent_app_switches.len() > 15 {
                                        recent_app_switches.pop_front();
                                    }
                                }
                            }
                            // Check if there are > 8 unique apps in the window
                            if recent_app_switches.len() >= 8 {
                                let unique: std::collections::HashSet<_> =
                                    recent_app_switches.iter().collect();
                                if unique.len() >= 6 {
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
            api::commands::create_autofill_overlay_window(&app.handle());

            if let Err(err) = api::commands::register_autofill_shortcut(
                &app.handle(),
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
            api::commands::search,
            api::commands::search_raw_results,
            api::commands::search_memory_cards,
            api::commands::list_memory_cards,
            api::commands::summarize_search,
            api::commands::get_fun_greeting,
            api::commands::get_status,
            // MCP
            api::commands::get_mcp_server_status,
            api::commands::start_mcp_server,
            api::commands::stop_mcp_server,
            api::commands::get_context_runtime_status,
            api::commands::list_recent_context_packs,
            api::commands::get_context_pack_detail,
            api::commands::fndr_subscribe,
            api::commands::fndr_unsubscribe,
            // Meetings
            api::commands::get_meeting_status,
            api::commands::start_meeting_recording,
            api::commands::stop_meeting_recording,
            api::commands::list_meetings,
            api::commands::delete_meeting,
            api::commands::get_meeting_transcript,
            api::commands::retranscribe_meeting,
            api::commands::export_meeting_pdf,
            api::commands::export_daily_summary_pdf,
            api::commands::open_exported_pdf,
            // Voice / Speech
            api::commands::transcribe_voice_input,
            api::commands::speak_text,
            // Capture control
            api::commands::pause_capture,
            api::commands::resume_capture,
            // Privacy & data
            api::commands::get_blocklist,
            api::commands::set_blocklist,
            api::commands::delete_all_data,
            api::commands::delete_memory,
            api::commands::get_stats,
            api::commands::get_retention_days,
            api::commands::set_retention_days,
            api::commands::delete_older_than,
            api::commands::get_app_names,
            api::commands::get_storage_health,
            api::commands::clean_dev_build_cache,
            api::commands::run_memory_repair_backfill,
            api::commands::get_memory_repair_progress,
            api::commands::get_memory_debug_inspector,
            api::commands::evaluate_recent_memory_quality,
            api::commands::get_capture_quality_dashboard,
            api::commands::rebuild_memory_context_for_range,
            api::commands::run_memory_retrieval_eval,
            api::commands::get_storage_reclaim_progress,
            api::commands::reclaim_memory_storage,
            api::commands::get_privacy_alerts,
            api::commands::dismiss_privacy_alert,
            api::commands::add_to_blocklist,
            // Tasks / Todos
            api::commands::add_todo,
            api::commands::get_todos,
            api::commands::update_todo,
            api::commands::dismiss_todo,
            api::commands::execute_todo,
            // Agent SDK
            api::commands::start_agent_task,
            api::commands::get_agent_status,
            api::commands::stop_agent,
            api::commands::get_hermes_bridge_status,
            api::commands::install_hermes_bridge,
            api::commands::save_hermes_setup,
            api::commands::sync_hermes_bridge_context,
            api::commands::start_hermes_gateway,
            api::commands::stop_hermes_gateway,
            api::commands::send_hermes_message,
            api::commands::send_direct_chat,
            api::commands::quick_setup_ollama,
            api::commands::link_audio_to_memories,
            api::commands::generate_daily_briefing,
            api::commands::generate_daily_summary_for_date,
            // Time tracking & Focus Mode
            api::commands::get_time_tracking,
            api::commands::set_focus_task,
            api::commands::get_focus_status,
            // Auto-fill
            api::commands::get_autofill_settings,
            api::commands::set_autofill_settings,
            api::commands::set_autofill_overlay_ready,
            api::commands::take_pending_autofill_payload,
            api::commands::show_autofill_overlay_window,
            api::commands::resolve_autofill,
            api::commands::inject_text,
            api::commands::dismiss_autofill,
            // Onboarding
            api::onboarding::get_onboarding_state,
            api::onboarding::save_onboarding_state,
            api::onboarding::request_biometric_auth,
            api::onboarding::check_permissions,
            api::onboarding::open_system_settings,
            api::onboarding::list_available_models,
            api::onboarding::download_model,
            api::onboarding::get_model_download_status,
            api::onboarding::refresh_ai_models,
            api::onboarding::check_model_exists,
            api::onboarding::delete_ai_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
