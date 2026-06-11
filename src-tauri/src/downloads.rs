//! Downloads folder watcher.
//!
//! Monitors the user's Downloads folder for new, completed files
//! and injects synthetic memory records so they become searchable.

use crate::config::DEFAULT_IMAGE_EMBEDDING_DIM;
use crate::embedding::{Embedder, EMBEDDING_DIM};
use crate::memory_compaction::{
    build_lexical_shadow, compact_summary_embedding_text, mean_pool_embeddings,
    support_embedding_texts_with_config,
};
use crate::storage::MemoryRecord;
use crate::AppState;
use chrono::Local;
use notify::{
    event::{ModifyKind, RenameMode},
    EventKind, RecursiveMode, Watcher,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

const DEBOUNCE_SECONDS: u64 = 10;

/// Run the async background loop watching the Downloads folder.
pub async fn run_watcher(state: Arc<AppState>) {
    let download_dir = match dirs::download_dir() {
        Some(d) => d,
        None => {
            tracing::warn!("Could not find user Downloads directory; tracker disabled.");
            return;
        }
    };

    run_watch_loop(state, download_dir).await;
}

async fn run_watch_loop(state: Arc<AppState>, watch_path: PathBuf) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let watcher_res = notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            let _ = tx.blocking_send(event);
        }
    });

    let mut watcher = match watcher_res {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("Failed to create watcher: {}", e);
            return;
        }
    };

    if let Err(e) = watcher.watch(&watch_path, RecursiveMode::NonRecursive) {
        tracing::error!("Failed to watch downloads dir: {}", e);
        return;
    }

    tracing::info!("Downloads tracker watching: {}", watch_path.display());

    let chunking_config = state.config.read().chunking.clone();
    let mut text_embedder = match Embedder::with_chunking_config(&chunking_config) {
        Ok(e) => Some(e),
        Err(e) => {
            tracing::warn!("Failed to initialize embedder for downloads tracker: {}", e);
            None
        }
    };

    let mut recent_files: HashMap<PathBuf, Instant> = HashMap::new();

    while let Some(event) = rx.recv().await {
        // Clean up old debounces periodically
        if recent_files.len() > 1000 {
            recent_files.retain(|_, v| v.elapsed().as_secs() < 60);
        }

        let is_interesting = match event.kind {
            EventKind::Create(_) => true,
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => true,
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)) => true,
            EventKind::Modify(ModifyKind::Data(_)) => true,
            EventKind::Modify(ModifyKind::Any) => true,
            _ => false,
        };

        if !is_interesting {
            continue;
        }

        for path in event.paths {
            if !path.is_file() {
                continue;
            }

            if is_temp_file(&path) {
                continue;
            }

            // Check debounce
            if let Some(last_seen) = recent_files.get(&path) {
                if last_seen.elapsed().as_secs() < DEBOUNCE_SECONDS {
                    continue;
                }
            }
            recent_files.insert(path.clone(), Instant::now());

            // A file arrived!
            if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                tracing::info!("Download detected: {}", filename);
                inject_download_memory(&state, &mut text_embedder, &path, filename).await;
            }
        }
    }

    // Loop ends correctly
}

fn is_temp_file(path: &Path) -> bool {
    let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
        return true;
    };

    if filename.starts_with('.') {
        return true; // .DS_Store, .crdownload, etc
    }

    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false; // No extension is fine, might be a binary or some app file
    };

    let ext = ext.to_lowercase();
    matches!(
        ext.as_str(),
        "crdownload" | "download" | "part" | "tmp" | "temp"
    )
}

async fn inject_download_memory(
    state: &Arc<AppState>,
    embedder: &mut Option<Embedder>,
    file_path: &Path,
    filename: &str,
) {
    let now = Local::now();
    let text = format!(
        "File downloaded locally to file system Tracker: {}",
        file_path.display()
    );
    let snippet = format!("Downloaded: {}", filename);

    let lexical_shadow = build_lexical_shadow("Downloads", &snippet, &text, None);
    let compact_summary_text =
        compact_summary_embedding_text("tracker", &snippet, &text, &lexical_shadow);
    let chunking_config = state.config.read().chunking.clone();
    let support_texts = support_embedding_texts_with_config(
        "Finder",
        "Downloads",
        &text,
        &lexical_shadow,
        Some(&chunking_config),
    );

    let (embedding, snippet_embedding, support_embedding) = if let Some(emb) = embedder {
        let mut contexts = vec![
            ("Finder".to_string(), "Downloads".to_string(), text.clone()),
            (
                "Finder".to_string(),
                "Downloads".to_string(),
                compact_summary_text,
            ),
        ];
        contexts.extend(
            support_texts
                .iter()
                .cloned()
                .map(|value| ("Finder".to_string(), "Downloads".to_string(), value)),
        );
        match emb.embed_batch_with_context(&contexts) {
            Ok(vectors) => {
                let text_vec = vectors
                    .first()
                    .cloned()
                    .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
                let snippet_vec = vectors
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
                let support_vec = if vectors.len() > 2 {
                    mean_pool_embeddings(&vectors[2..])
                } else {
                    vec![0.0; EMBEDDING_DIM]
                };
                (text_vec, snippet_vec, support_vec)
            }
            Err(_) => (
                vec![0.0; EMBEDDING_DIM],
                vec![0.0; EMBEDDING_DIM],
                vec![0.0; EMBEDDING_DIM],
            ),
        }
    } else {
        (
            vec![0.0; EMBEDDING_DIM],
            vec![0.0; EMBEDDING_DIM],
            vec![0.0; EMBEDDING_DIM],
        )
    };

    let record = MemoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: now.timestamp_millis(),
        day_bucket: now.format("%Y-%m-%d").to_string(),
        app_name: "Finder".to_string(),
        bundle_id: Some("com.apple.finder".to_string()),
        window_title: "Downloads".to_string(),
        session_id: format!("{}-downloads", now.format("%Y%m%d")),
        text: String::new(),
        clean_text: text.clone(),
        ocr_confidence: 1.0,
        ocr_block_count: 1,
        snippet: snippet.clone(),
        summary_source: "tracker".to_string(),
        noise_score: 0.0,
        session_key: "filesystem:downloads".to_string(),
        lexical_shadow,
        embedding,
        image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
        screenshot_path: None,
        url: None,
        snippet_embedding,
        support_embedding,
        decay_score: 1.0,
        last_accessed_at: now.timestamp_millis(),
        ..Default::default()
    };

    if state.store.add_batch(&[record.clone()]).await.is_err() {
        tracing::error!("Failed to store download memory");
    } else if let Err(err) =
        crate::context_runtime::sync_memory_record(state.as_ref(), &record, Some("file")).await
    {
        tracing::warn!(
            "Failed to sync download memory into context runtime: {}",
            err
        );
    }
}
