//! Local text embedding backend via native ONNX Runtime.

use super::admission::{AdmissionGate, EmbeddingPriority};
use super::gemma_helper::GemmaHelper;
use super::{chunk_screen_text, TextChunker};
use crate::config::{
    ChunkingConfig, DEFAULT_EMBEDDING_CACHE_CAPACITY, DEFAULT_EMBEDDING_MODEL_NAME,
    DEFAULT_TEXT_EMBEDDING_DIM,
};
use crate::inference::model_config::{
    active_embedding_contract, embedding_v5_contract, validate_embedding_config_against_contract,
    EmbeddingContractVersion, TextEmbeddingContract,
};
use ndarray::Array2;
use ort::session::Session;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// Authoritative text embedding dimension for the primary semantic index.
pub const EMBEDDING_DIM: usize = DEFAULT_TEXT_EMBEDDING_DIM;
const MODEL_NAME: &str = DEFAULT_EMBEDDING_MODEL_NAME;
const EMBEDDING_CACHE_CAPACITY: usize = DEFAULT_EMBEDDING_CACHE_CAPACITY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingBackend {
    Real,
    Mock,
}

/// Raw, unprompted text for role-aware embedding. Document context is composed
/// before the model-specific prompt is added to each chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbeddingInput<'a> {
    Query(&'a str),
    Document {
        text: &'a str,
        app_name: &'a str,
        window_title: &'a str,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRuntimeStatus {
    pub backend: String,
    pub degraded: bool,
    pub detail: String,
    pub model_name: String,
    pub dimension: usize,
}

#[derive(Debug, Clone)]
struct EmbeddingRuntimeState {
    backend: String,
    degraded: bool,
    detail: String,
    model_name: String,
    dimension: usize,
}

static EMBEDDING_RUNTIME_STATE: OnceLock<Mutex<EmbeddingRuntimeState>> = OnceLock::new();
static SHARED_BGE_QUERY_EMBEDDER: OnceLock<SharedBgeQueryEmbedder> = OnceLock::new();

#[derive(Default)]
struct SharedBgeQueryEmbedder {
    embedder: Mutex<Option<Arc<Embedder>>>,
}

impl SharedBgeQueryEmbedder {
    fn get_or_try_init<F>(&self, factory: F) -> Result<Arc<Embedder>, String>
    where
        F: FnOnce() -> Result<Embedder, String>,
    {
        let mut guard = self
            .embedder
            .lock()
            .map_err(|err| format!("BGE query embedder lock poisoned: {err}"))?;
        if let Some(embedder) = guard.as_ref() {
            return Ok(Arc::clone(embedder));
        }

        let embedder = Arc::new(factory()?);
        *guard = Some(Arc::clone(&embedder));
        Ok(embedder)
    }
}

/// Lazily initialize and reuse the process-wide BGE query embedder.
///
/// Failed initialization is deliberately not cached so installing the model
/// assets while FNDR is running can recover on the next chunk-search request.
pub fn shared_bge_v5_query_embedder() -> Result<Arc<Embedder>, String> {
    SHARED_BGE_QUERY_EMBEDDER
        .get_or_init(SharedBgeQueryEmbedder::default)
        .get_or_try_init(|| {
            let started = std::time::Instant::now();
            let result = Embedder::new_bge_v5_for_query();
            crate::telemetry::runtime_metrics::record_ms(
                "embedding.bge_query_init_ms",
                started.elapsed().as_millis() as u64,
            );
            result
        })
}

fn runtime_state() -> &'static Mutex<EmbeddingRuntimeState> {
    EMBEDDING_RUNTIME_STATE.get_or_init(|| {
        Mutex::new(EmbeddingRuntimeState {
            backend: "unknown".to_string(),
            degraded: false,
            detail: "Embedder not initialized yet".to_string(),
            model_name: MODEL_NAME.to_string(),
            dimension: EMBEDDING_DIM,
        })
    })
}

fn set_runtime_state_for_contract(
    contract: TextEmbeddingContract,
    backend: &str,
    degraded: bool,
    detail: impl Into<String>,
) {
    if let Ok(mut guard) = runtime_state().lock() {
        guard.backend = backend.to_string();
        guard.degraded = degraded;
        guard.detail = detail.into();
        guard.model_name = contract.model_id.to_string();
        guard.dimension = contract.dimensions;
    }
}

pub fn embedding_runtime_status() -> EmbeddingRuntimeStatus {
    if let Ok(guard) = runtime_state().lock() {
        EmbeddingRuntimeStatus {
            backend: guard.backend.clone(),
            degraded: guard.degraded,
            detail: guard.detail.clone(),
            model_name: guard.model_name.clone(),
            dimension: guard.dimension,
        }
    } else {
        EmbeddingRuntimeStatus {
            backend: "unknown".to_string(),
            degraded: true,
            detail: "Embedding runtime state lock poisoned".to_string(),
            model_name: MODEL_NAME.to_string(),
            dimension: EMBEDDING_DIM,
        }
    }
}

/// Cloneable request handle. Clones share this wrapper's cache and fallback state;
/// separately constructed wrappers retain their own policy and preprocessing.
#[derive(Clone)]
pub struct Embedder {
    contract: TextEmbeddingContract,
    chunker: TextChunker,
    backend: Backend,
    degraded_to_mock: Arc<AtomicBool>,
    allow_mock_fallback: bool,
    embedding_cache: Arc<Mutex<EmbeddingCache>>,
}

pub(crate) fn cached_embedder(
    cell: &OnceLock<Embedder>,
    initialize: impl FnOnce() -> Result<Embedder, String>,
) -> Result<&Embedder, String> {
    if let Some(embedder) = cell.get() {
        return Ok(embedder);
    }
    // Cache only success so a model installed mid-session can recover. Racing
    // initializers share the real backend; retain the first published wrapper.
    let _ = cell.set(initialize()?);
    Ok(cell.get().expect("successful initialization published"))
}

#[derive(Clone)]
enum Backend {
    Real(Arc<RealEmbedder>),
    Mock(MockEmbedder),
}

#[derive(Debug)]
struct EmbeddingCache {
    capacity: usize,
    order: VecDeque<String>,
    values: HashMap<String, Vec<f32>>,
}

impl EmbeddingCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::with_capacity(capacity),
            values: HashMap::with_capacity(capacity),
        }
    }

    fn get(&self, text: &str) -> Option<Vec<f32>> {
        self.values.get(text).cloned()
    }

    fn insert(&mut self, text: String, embedding: Vec<f32>) {
        if self.values.contains_key(&text) {
            return;
        }

        if self.order.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.values.remove(&evicted);
            }
        }

        self.order.push_back(text.clone());
        self.values.insert(text, embedding);
    }
}

impl Embedder {
    #[cfg(test)]
    pub(crate) fn with_embedding_cache_locked_for_test(&self, f: impl FnOnce()) {
        let _guard = self.embedding_cache.lock().expect("embedding cache");
        f();
    }

    pub fn new() -> Result<Self, String> {
        Self::with_chunking_config(&ChunkingConfig::default())
    }

    pub fn new_bge_v5_for_reindex() -> Result<Self, String> {
        Self::with_contract_and_chunking_config(
            embedding_v5_contract(),
            &ChunkingConfig::default(),
            false,
        )
    }

    pub fn new_bge_v5_for_query() -> Result<Self, String> {
        Self::with_contract_and_chunking_config(
            embedding_v5_contract(),
            &ChunkingConfig::default(),
            false,
        )
    }

    /// Create an `Embedder` whose internal `TextChunker` uses runtime config
    /// values instead of compiled-in defaults. Prefer this at all sites that
    /// already hold a loaded `Config`.
    pub fn with_chunking_config(chunking: &ChunkingConfig) -> Result<Self, String> {
        Self::with_contract_and_chunking_config(active_embedding_contract(), chunking, true)
    }

    pub fn with_contract_and_chunking_config(
        contract: TextEmbeddingContract,
        chunking: &ChunkingConfig,
        allow_mock_fallback: bool,
    ) -> Result<Self, String> {
        let chunker = TextChunker::from_config(chunking);

        match RealEmbedder::new(contract) {
            Ok(real) => {
                set_runtime_state_for_contract(
                    contract,
                    "real",
                    false,
                    format!("{} embedder ready", contract.model_id),
                );
                Ok(Self {
                    contract,
                    chunker,
                    backend: Backend::Real(real),
                    degraded_to_mock: Arc::new(AtomicBool::new(false)),
                    allow_mock_fallback,
                    embedding_cache: Arc::new(Mutex::new(EmbeddingCache::new(
                        EMBEDDING_CACHE_CAPACITY,
                    ))),
                })
            }
            Err(err) => {
                if allow_mock_fallback && allow_mock_embedder() {
                    let reason =
                        format!("Semantic embeddings degraded to mock mode. Reason: {}", err);
                    tracing::warn!(
                        "{} embedder fallback active: using MOCK embeddings. {}",
                        contract.model_id,
                        reason
                    );
                    set_runtime_state_for_contract(contract, "mock", true, reason);
                    Ok(Self {
                        contract,
                        chunker,
                        backend: Backend::Mock(MockEmbedder::new(contract.dimensions)),
                        degraded_to_mock: Arc::new(AtomicBool::new(true)),
                        allow_mock_fallback,
                        embedding_cache: Arc::new(Mutex::new(EmbeddingCache::new(
                            EMBEDDING_CACHE_CAPACITY,
                        ))),
                    })
                } else {
                    set_runtime_state_for_contract(
                        contract,
                        "unavailable",
                        true,
                        format!(
                            "{} embedder failed and mock fallback is disabled: {}",
                            contract.model_id, err
                        ),
                    );
                    Err(format!(
                        "Failed to initialize real {} embedder and mock fallback is disabled: {err}",
                        contract.model_id
                    ))
                }
            }
        }
    }

    /// The feature-hashing mock with the active contract, for tests that need
    /// non-zero vectors without the model on disk.
    #[cfg(test)]
    pub(crate) fn mock_for_tests() -> Self {
        let contract = active_embedding_contract();
        Self {
            contract,
            chunker: TextChunker::new(),
            backend: Backend::Mock(MockEmbedder::new(contract.dimensions)),
            degraded_to_mock: Arc::new(AtomicBool::new(false)),
            allow_mock_fallback: false,
            embedding_cache: Arc::new(Mutex::new(EmbeddingCache::new(EMBEDDING_CACHE_CAPACITY))),
        }
    }

    pub fn dimension(&self) -> usize {
        self.contract.dimensions
    }

    /// The contract this embedder writes and searches under; it decides the
    /// query and document prompts (`embedding::prefixes`).
    pub fn contract(&self) -> TextEmbeddingContract {
        self.contract
    }

    pub fn backend(&self) -> EmbeddingBackend {
        if self.degraded_to_mock.load(Ordering::Relaxed) {
            return EmbeddingBackend::Mock;
        }

        match self.backend {
            Backend::Real(_) => EmbeddingBackend::Real,
            Backend::Mock(_) => EmbeddingBackend::Mock,
        }
    }

    /// Chunk text for embedding (char fallback path).
    pub fn chunk_text(&self, text: &str) -> Vec<String> {
        self.chunker.chunk(text)
    }

    /// Chunk text with app/window context so OCR-aware boundaries survive into embeddings.
    pub fn chunk_text_with_context(
        &self,
        app_name: &str,
        window_title: &str,
        text: &str,
    ) -> Vec<String> {
        if app_name.trim().is_empty() && window_title.trim().is_empty() {
            self.chunk_text(text)
        } else {
            chunk_screen_text(&self.chunker, app_name, window_title, text)
        }
    }

    /// Generate embeddings for a batch of texts.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.embed_batch_with_priority(texts, EmbeddingPriority::Background)
    }

    /// Legacy query text is already composed/prefixed by its caller. Admission
    /// changes scheduling only; it must not change v4/v5 query prompts.
    pub fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed_batch_with_priority(&[text.to_string()], EmbeddingPriority::Foreground)?
            .into_iter()
            .next()
            .ok_or_else(|| "Embedder returned no query vector".into())
    }

    fn embed_batch_with_priority(
        &self,
        texts: &[String],
        priority: EmbeddingPriority,
    ) -> Result<Vec<Vec<f32>>, String> {
        let chunk_groups = texts
            .iter()
            .map(|text| {
                let chunks = self.chunk_text(text);
                if chunks.is_empty() && !text.trim().is_empty() {
                    vec![text.clone()]
                } else {
                    chunks
                }
            })
            .collect::<Vec<_>>();
        self.embed_chunk_groups(self.prompt_legacy_documents(chunk_groups), priority)
    }

    /// The legacy entry points (`embed_batch`, `embed_batch_with_context`)
    /// carry no role, so what they embed is documents. Contracts whose model
    /// wants a document prompt get it here, after chunking; text that already
    /// carries a query prompt (`embed_query` callers) is left as it is.
    fn prompt_legacy_documents(&self, chunk_groups: Vec<Vec<String>>) -> Vec<Vec<String>> {
        if self.contract.version != EmbeddingContractVersion::V7EmbeddingGemma2 {
            return chunk_groups;
        }
        use super::prefixes::{document_text_for, EMBEDDING_GEMMA_QUERY_PREFIX};
        chunk_groups
            .into_iter()
            .map(|chunks| {
                chunks
                    .into_iter()
                    .map(|chunk| {
                        if is_embedding_low_signal(&chunk)
                            || chunk.starts_with(EMBEDDING_GEMMA_QUERY_PREFIX)
                        {
                            chunk
                        } else {
                            document_text_for(self.contract, &chunk)
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// Prepare the exact prompted chunks used by `embed_inputs`, without
    /// inference or cache writes. Exposed for tokenizer/budget measurements.
    /// Like the legacy wrappers, fall back to raw text when chunking drops it;
    /// the embedding low-signal check still runs before prompting.
    pub fn prepare_input_chunks(&self, input: EmbeddingInput<'_>) -> Vec<String> {
        use super::prefixes::{document_text_for, query_text_for};
        let (text, mut chunks) = match input {
            EmbeddingInput::Query(text) => (text, self.chunk_text(text)),
            EmbeddingInput::Document {
                text,
                app_name,
                window_title,
            } => (
                text,
                self.chunk_text_with_context(app_name, window_title, text),
            ),
        };
        if chunks.is_empty() && !text.trim().is_empty() {
            chunks.push(text.to_string());
        }
        chunks
            .into_iter()
            .filter(|chunk| !is_embedding_low_signal(chunk))
            .map(|chunk| match input {
                EmbeddingInput::Query(_) => query_text_for(self.contract, &chunk),
                EmbeddingInput::Document { .. } => document_text_for(self.contract, &chunk),
            })
            .collect()
    }

    /// Embed mixed raw query/document inputs in order. Prefix every chunk after
    /// context composition. Query-only calls receive foreground admission;
    /// document/mixed calls yield between chunks. Vector pooling is unchanged.
    pub fn embed_inputs(&self, inputs: &[EmbeddingInput<'_>]) -> Result<Vec<Vec<f32>>, String> {
        let priority = if inputs
            .iter()
            .all(|input| matches!(input, EmbeddingInput::Query(_)))
        {
            EmbeddingPriority::Foreground
        } else {
            EmbeddingPriority::Background
        };
        self.embed_chunk_groups(
            inputs
                .iter()
                .map(|input| self.prepare_input_chunks(*input))
                .collect(),
            priority,
        )
    }

    /// Generate embeddings for texts while preserving app/window context during chunking.
    pub fn embed_batch_with_context(
        &self,
        texts: &[(String, String, String)],
    ) -> Result<Vec<Vec<f32>>, String> {
        let chunk_groups = texts
            .iter()
            .map(|(app_name, window_title, text)| {
                let chunks = self.chunk_text_with_context(app_name, window_title, text);
                if chunks.is_empty() && !text.trim().is_empty() {
                    vec![text.clone()]
                } else {
                    chunks
                }
            })
            .collect::<Vec<_>>();
        self.embed_chunk_groups(
            self.prompt_legacy_documents(chunk_groups),
            EmbeddingPriority::Background,
        )
    }

    /// Product-named wrapper for the capture -> chunking -> embedding boundary.
    pub fn embed_memory_chunk(
        &self,
        app_name: &str,
        window_title: &str,
        text: &str,
    ) -> Result<Vec<f32>, String> {
        self.embed_batch_with_context(&[(
            app_name.to_string(),
            window_title.to_string(),
            text.to_string(),
        )])?
        .into_iter()
        .next()
        .ok_or_else(|| "Embedder returned no vector for memory chunk".to_string())
    }

    fn embed_chunks_cached(
        &self,
        texts: &[String],
        priority: EmbeddingPriority,
    ) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut missing_unique = Vec::new();
        let mut missing_by_text: HashMap<String, usize> = HashMap::new();
        let mut missing_positions: Vec<(usize, usize)> = Vec::new();

        if let Ok(cache) = self.embedding_cache.lock() {
            for (index, text) in texts.iter().enumerate() {
                if is_embedding_low_signal(text) {
                    results[index] = Some(vec![0.0; self.dimension()]);
                    continue;
                }

                if let Some(hit) = cache.get(text) {
                    results[index] = Some(hit);
                    continue;
                }

                if let Some(unique_idx) = missing_by_text.get(text).copied() {
                    missing_positions.push((index, unique_idx));
                    continue;
                }

                let unique_idx = missing_unique.len();
                missing_by_text.insert(text.clone(), unique_idx);
                missing_unique.push(text.clone());
                missing_positions.push((index, unique_idx));
            }
        } else {
            // Cache lock poisoned: fall back to direct dedup without cache.
            for (index, text) in texts.iter().enumerate() {
                if is_embedding_low_signal(text) {
                    results[index] = Some(vec![0.0; self.dimension()]);
                    continue;
                }
                if let Some(unique_idx) = missing_by_text.get(text).copied() {
                    missing_positions.push((index, unique_idx));
                    continue;
                }
                let unique_idx = missing_unique.len();
                missing_by_text.insert(text.clone(), unique_idx);
                missing_unique.push(text.clone());
                missing_positions.push((index, unique_idx));
            }
        }

        if !missing_unique.is_empty() {
            let mut computed = Vec::with_capacity(missing_unique.len());
            // Yield after each background chunk so a newly queued query need
            // not wait for a padded multi-document ONNX batch to finish.
            let batch_size = match priority {
                EmbeddingPriority::Background => 1,
                EmbeddingPriority::Foreground => self.contract.max_batch_size.max(1),
            };
            for chunk in missing_unique.chunks(batch_size) {
                let batch = chunk.to_vec();
                let vectors = self.backend_embed_batch(&batch, priority)?;
                computed.extend(vectors);
            }

            if computed.len() != missing_unique.len() {
                return Err(format!(
                    "Embedding backend returned {} vectors for {} cache misses",
                    computed.len(),
                    missing_unique.len()
                ));
            }

            for (position, unique_idx) in &missing_positions {
                results[*position] = Some(
                    computed
                        .get(*unique_idx)
                        .cloned()
                        .unwrap_or_else(|| vec![0.0; self.dimension()]),
                );
            }

            if let Ok(mut cache) = self.embedding_cache.lock() {
                for (text, embedding) in missing_unique.into_iter().zip(computed.into_iter()) {
                    cache.insert(text, embedding);
                }
            }
        }

        Ok(results
            .into_iter()
            .map(|value| value.unwrap_or_else(|| vec![0.0; self.dimension()]))
            .collect())
    }

    fn embed_chunk_groups(
        &self,
        chunk_groups: Vec<Vec<String>>,
        priority: EmbeddingPriority,
    ) -> Result<Vec<Vec<f32>>, String> {
        if chunk_groups.is_empty() {
            return Ok(Vec::new());
        }

        let mut flattened_chunks = Vec::new();
        let mut ranges = Vec::with_capacity(chunk_groups.len());

        for chunks in chunk_groups {
            let start = flattened_chunks.len();
            flattened_chunks.extend(chunks);
            let end = flattened_chunks.len();
            ranges.push((start, end));
        }

        if flattened_chunks.is_empty() {
            return Ok(vec![vec![0.0; self.dimension()]; ranges.len()]);
        }

        let chunk_embeddings = self.embed_chunks_cached(&flattened_chunks, priority)?;
        if chunk_embeddings.len() != flattened_chunks.len() {
            return Err(format!(
                "Embedding backend returned {} vectors for {} chunks",
                chunk_embeddings.len(),
                flattened_chunks.len()
            ));
        }

        let mut merged = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            if start == end {
                merged.push(vec![0.0; self.dimension()]);
                continue;
            }
            let vectors = &chunk_embeddings[start..end];
            merged.push(mean_pool(vectors, self.dimension()));
        }

        Ok(merged)
    }

    fn backend_embed_batch(
        &self,
        texts: &[String],
        priority: EmbeddingPriority,
    ) -> Result<Vec<Vec<f32>>, String> {
        match &self.backend {
            Backend::Real(real) => {
                if self.degraded_to_mock.load(Ordering::Relaxed) {
                    return Ok(MockEmbedder::new(self.dimension()).embed_batch(texts));
                }

                match real.embed_scheduled(texts, priority) {
                    Ok(vectors) => Ok(vectors),
                    Err(err) => {
                        if self.allow_mock_fallback && allow_mock_embedder() {
                            self.degraded_to_mock.store(true, Ordering::Relaxed);
                            let detail = format!(
                                "Runtime embedding failure; switched to mock mode: {}",
                                err
                            );
                            tracing::warn!("{}", detail);
                            set_runtime_state_for_contract(self.contract, "mock", true, detail);
                            Ok(MockEmbedder::new(self.dimension()).embed_batch(texts))
                        } else {
                            set_runtime_state_for_contract(
                                self.contract,
                                "unavailable",
                                true,
                                format!("Runtime embedding failure: {}", err),
                            );
                            Err(err)
                        }
                    }
                }
            }
            Backend::Mock(mock) => Ok(mock.embed_batch(texts)),
        }
    }
}

fn is_embedding_low_signal(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let alnum = trimmed.chars().filter(|ch| ch.is_alphanumeric()).count();
    alnum < 3
}

impl Default for Embedder {
    fn default() -> Self {
        Self::new().expect("Failed to create embedder")
    }
}

struct RealEmbedder {
    admission: AdmissionGate,
    contract: TextEmbeddingContract,
    engine: Engine,
}

enum Engine {
    Onnx(OnnxEngine),
    /// EmbeddingGemma 2 in `fndr-eg2-helper`.
    Gemma(GemmaHelper),
}

struct OnnxEngine {
    session: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
    input_names: Vec<String>,
    output_name: String,
}

struct ResidentTextModel {
    contract: TextEmbeddingContract,
    model_dir: PathBuf,
    model: Weak<RealEmbedder>,
}

static RESIDENT_TEXT_MODELS: OnceLock<Mutex<Vec<ResidentTextModel>>> = OnceLock::new();

impl RealEmbedder {
    fn new(contract: TextEmbeddingContract) -> Result<Arc<Self>, String> {
        let model_dir = resolve_model_dir(contract)
            .ok_or_else(|| "Could not determine model directory".to_string())?;
        Self::shared_from_dir(contract, model_dir)
    }

    fn shared_from_dir(
        contract: TextEmbeddingContract,
        model_dir: PathBuf,
    ) -> Result<Arc<Self>, String> {
        let model_dir = model_dir.canonicalize().map_err(|e| e.to_string())?;
        let mut residents = RESIDENT_TEXT_MODELS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .map_err(|e| format!("Text model registry lock poisoned: {e}"))?;
        residents.retain(|entry| entry.model.strong_count() > 0);
        for entry in residents.iter() {
            if entry.contract == contract && entry.model_dir == model_dir {
                if let Some(model) = entry.model.upgrade() {
                    return Ok(model);
                }
            }
        }

        // Serialize initialization so racing callers cannot load duplicate
        // weights. Publish only after the real model passes its dimension probe;
        // failures remain retryable. Weak ownership permits release when the last
        // wrapper drops, while chunking/cache/fallback stay local to each wrapper.
        let model = Arc::new(Self::load(contract, &model_dir)?);
        residents.push(ResidentTextModel {
            contract,
            model_dir,
            model: Arc::downgrade(&model),
        });
        Ok(model)
    }

    fn load(contract: TextEmbeddingContract, model_dir: &std::path::Path) -> Result<Self, String> {
        let embedder = if contract.version == EmbeddingContractVersion::V7EmbeddingGemma2 {
            let model_path = model_dir.join(contract.model_filename);
            if !model_path.exists() {
                return Err(format!(
                    "EmbeddingGemma 2 model not found at {}. Download {} or set FNDR_MODEL_DIR.",
                    model_path.display(),
                    contract.model_filename
                ));
            }
            Self {
                admission: AdmissionGate::default(),
                contract,
                engine: Engine::Gemma(GemmaHelper::start(&model_path, contract.dimensions)?),
            }
        } else {
            Self::load_onnx(contract, model_dir)?
        };
        embedder.probe()?;
        Ok(embedder)
    }

    fn load_onnx(
        contract: TextEmbeddingContract,
        model_dir: &std::path::Path,
    ) -> Result<Self, String> {
        let onnx_path = model_dir.join(contract.model_filename);
        let tokenizer_path = model_dir.join(contract.tokenizer_filename);

        if !onnx_path.exists() {
            return Err(format!(
                "ONNX model not found at {}. Download {} and {} or set FNDR_MODEL_DIR.",
                onnx_path.display(),
                contract.model_filename,
                contract.tokenizer_filename
            ));
        }
        if !tokenizer_path.exists() {
            return Err(format!(
                "Tokenizer not found at {}. Download {} and {} or set FNDR_MODEL_DIR.",
                tokenizer_path.display(),
                contract.model_filename,
                contract.tokenizer_filename
            ));
        }

        let session = Session::builder()
            .map_err(|e| format!("Failed to create ort session builder: {e}"))?
            .commit_from_file(&onnx_path)
            .map_err(|e| {
                format!(
                    "Failed to load ONNX model from {}: {e}",
                    onnx_path.display()
                )
            })?;

        let input_names = session
            .inputs()
            .iter()
            .map(|input| input.name().to_string())
            .collect::<Vec<_>>();
        for required in ["input_ids", "attention_mask"] {
            if !input_names.iter().any(|name| name == required) {
                return Err(format!(
                    "Embedding model {} is missing required ONNX input '{}'. Found inputs: {:?}",
                    onnx_path.display(),
                    required,
                    input_names
                ));
            }
        }
        // A model that exports its own sentence vector (EmbeddingGemma's applies
        // pooling, two dense layers, and normalization) must be read there;
        // mean-pooling its hidden states would skip the dense layers (VS-47).
        let output_name = session
            .outputs()
            .iter()
            .find(|output| output.name() == "sentence_embedding")
            .or_else(|| {
                session
                    .outputs()
                    .iter()
                    .find(|output| output.name() == "last_hidden_state")
            })
            .or_else(|| {
                session
                    .outputs()
                    .iter()
                    .find(|output| output.name() == "token_embeddings")
            })
            .or_else(|| session.outputs().first())
            .map(|output| output.name().to_string())
            .ok_or_else(|| {
                format!(
                    "Embedding model {} exposes no ONNX outputs",
                    onnx_path.display()
                )
            })?;

        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            format!(
                "Failed to load tokenizer from {}: {e}",
                tokenizer_path.display()
            )
        })?;

        tracing::info!(
            model = %onnx_path.display(),
            output = %output_name,
            inputs = ?input_names,
            "Native ort text embedder initialized"
        );
        Ok(Self {
            admission: AdmissionGate::default(),
            contract,
            engine: Engine::Onnx(OnnxEngine {
                session: Mutex::new(session),
                tokenizer,
                input_names,
                output_name,
            }),
        })
    }

    /// One embedding must come back at the contract's size and not be all zero.
    fn probe(&self) -> Result<(), String> {
        let contract = self.contract;
        let probe = self.embed_batch(&["FNDR embedding dimension probe".to_string()])?;
        let actual_dim = probe.first().map(|vector| vector.len()).unwrap_or(0);
        if actual_dim != contract.dimensions {
            return Err(format!(
                "Embedding dimension mismatch for {}: the model returned {actual_dim}-d vectors but the FNDR contract expects {}-d for table {}. \
                 Ensure FNDR_MODEL_DIR points at a directory containing {} + {} for the active contract.",
                contract.model_id,
                contract.dimensions,
                contract.table_name,
                contract.model_filename,
                contract.tokenizer_filename
            ));
        }
        if probe
            .first()
            .map(|vector| vector.iter().all(|value| *value == 0.0))
            .unwrap_or(true)
        {
            return Err("Embedding probe returned an all-zero vector".to_string());
        }
        Ok(())
    }

    fn embed_scheduled(
        &self,
        texts: &[String],
        priority: EmbeddingPriority,
    ) -> Result<Vec<Vec<f32>>, String> {
        let queued = std::time::Instant::now();
        let _permit = self.admission.enter(priority)?;
        let queue_ms = queued.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        let result = self.embed_batch(texts);
        let service_ms = started.elapsed().as_secs_f64() * 1000.0;
        let (queue_op, service_op) = match priority {
            EmbeddingPriority::Foreground => (
                "embedding.foreground_queue_ms",
                "embedding.foreground_service_ms",
            ),
            EmbeddingPriority::Background => (
                "embedding.background_queue_ms",
                "embedding.background_service_ms",
            ),
        };
        crate::telemetry::runtime_metrics::record_ms(queue_op, queue_ms as u64);
        crate::telemetry::runtime_metrics::record_ms(service_op, service_ms as u64);
        tracing::debug!(
            ?priority,
            queue_ms,
            service_ms,
            chunks = texts.len(),
            "embedding admission completed"
        );
        result
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let onnx = match &self.engine {
            Engine::Onnx(onnx) => onnx,
            Engine::Gemma(helper) => {
                let t_helper = std::time::Instant::now();
                let vectors = helper.embed(texts)?;
                crate::telemetry::runtime_metrics::record_ms(
                    "embedding.eg2_batch_ms",
                    t_helper.elapsed().as_millis() as u64,
                );
                return Ok(vectors);
            }
        };
        let t_onnx = std::time::Instant::now();
        let encodings = onnx
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| format!("Tokenization failed: {e}"))?;

        let batch_size = texts.len();
        let seq_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0)
            .min(self.contract.max_sequence_length);

        if seq_len == 0 {
            return Ok(vec![vec![0.0f32; self.contract.dimensions]; batch_size]);
        }

        let mut input_ids = Array2::<i64>::zeros((batch_size, seq_len));
        let mut attention_mask = Array2::<i64>::zeros((batch_size, seq_len));
        let token_type_ids = Array2::<i64>::zeros((batch_size, seq_len));

        for (i, enc) in encodings.iter().enumerate() {
            let ids = enc.get_ids();
            let mask = enc.get_attention_mask();
            let len = ids.len().min(seq_len);
            for j in 0..len {
                input_ids[[i, j]] = ids[j] as i64;
                attention_mask[[i, j]] = mask[j] as i64;
            }
        }

        // Wrap ndarray arrays into ort Tensors (requires ndarray feature).
        // Clone attention_mask for mean-pooling after ownership is transferred to the session.
        let attention_mask_pooling = attention_mask.clone();
        let ids_t = ort::value::Tensor::from_array(input_ids)
            .map_err(|e| format!("Failed to create input_ids tensor: {e}"))?;
        let mask_t = ort::value::Tensor::from_array(attention_mask)
            .map_err(|e| format!("Failed to create attention_mask tensor: {e}"))?;
        let types_t = ort::value::Tensor::from_array(token_type_ids)
            .map_err(|e| format!("Failed to create token_type_ids tensor: {e}"))?;
        let mut session_guard = onnx
            .session
            .lock()
            .map_err(|e| format!("Session mutex poisoned: {e}"))?;
        let mut inputs = ort::inputs![
            "input_ids" => ids_t,
            "attention_mask" => mask_t,
        ];
        if onnx.input_names.iter().any(|name| name == "token_type_ids") {
            inputs.push((Cow::from("token_type_ids"), types_t.into()));
        }

        let outputs = session_guard
            .run(inputs)
            .map_err(|e| format!("ONNX inference failed: {e}"))?;

        let output = outputs
            .get(&onnx.output_name)
            .or_else(|| outputs.get("last_hidden_state"))
            .or_else(|| outputs.get("token_embeddings"))
            .or_else(|| {
                let first_key = outputs.keys().next()?;
                outputs.get(first_key)
            })
            .ok_or_else(|| {
                format!(
                    "ONNX inference produced no usable embedding output. Expected '{}'",
                    onnx.output_name
                )
            })?;

        // ort 2.x RC: try_extract_tensor returns (Shape, &[T]).
        let (shape, data) = output
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Failed to extract hidden state tensor: {e}"))?;

        let shape_dims = shape.iter().map(|dim| *dim as usize).collect::<Vec<_>>();
        let actual_dim = match shape_dims.as_slice() {
            [_, dim] => *dim,
            [_, _, dim] => *dim,
            _ => 0,
        };
        let truncating =
            self.contract.supports_truncation() && actual_dim > self.contract.dimensions;
        if actual_dim != self.contract.dimensions && !truncating {
            return Err(format!(
                "Unexpected hidden state dim {actual_dim}, expected {} for {}",
                self.contract.dimensions, self.contract.model_id
            ));
        }
        if self.contract.supports_truncation() && shape_dims.len() == 3 {
            return Err(format!(
                "{} needs the model's sentence_embedding output; mean-pooling its hidden \
                 states would skip its dense layers",
                self.contract.model_id
            ));
        }

        let mut embeddings = Vec::with_capacity(batch_size);
        match shape_dims.as_slice() {
            [actual_batch, output_dim] => {
                for i in 0..batch_size.min(*actual_batch) {
                    let offset = i * output_dim;
                    let embedding = data[offset..offset + output_dim].to_vec();
                    embeddings.push(truncate_and_normalize(embedding, self.contract.dimensions));
                }
            }
            [actual_batch, actual_seq, actual_dim] if *actual_dim == self.contract.dimensions => {
                for i in 0..batch_size.min(*actual_batch) {
                    let mut sum = vec![0.0f32; self.contract.dimensions];
                    let mut count = 0.0f32;
                    for j in 0..*actual_seq {
                        let mask_j = j.min(seq_len - 1);
                        if attention_mask_pooling[[i, mask_j]] > 0 {
                            let offset = (i * *actual_seq + j) * self.contract.dimensions;
                            for k in 0..self.contract.dimensions {
                                sum[k] += data[offset + k];
                            }
                            count += 1.0;
                        }
                    }
                    if count > 0.0 {
                        for v in &mut sum {
                            *v /= count;
                        }
                    }
                    normalize(&mut sum);
                    embeddings.push(sum);
                }
            }
            _ => {
                return Err(format!(
                    "Unexpected embedding output shape {:?}; expected [batch, {}] or [batch, seq, {}]",
                    shape_dims, self.contract.dimensions, self.contract.dimensions
                ));
            }
        }

        if embeddings.len() != batch_size {
            return Err(format!(
                "ONNX inference returned {} embeddings for batch size {}",
                embeddings.len(),
                batch_size
            ));
        }
        crate::telemetry::runtime_metrics::record_ms(
            "embedding.onnx_batch_ms",
            t_onnx.elapsed().as_millis() as u64,
        );
        Ok(embeddings)
    }
}

#[derive(Debug, Clone)]
struct MockEmbedder {
    dimensions: usize,
}

impl MockEmbedder {
    fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }

    fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        texts.iter().map(|text| self.embed_single(text)).collect()
    }

    fn embed_single(&self, text: &str) -> Vec<f32> {
        // Feature-hashing bag-of-words fallback for dev/test only.
        let mut vector = vec![0.0f32; self.dimensions];
        let lower = text.to_lowercase();

        for token in lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|tok| tok.len() > 2)
        {
            let idx = stable_hash(token) % self.dimensions;
            vector[idx] += 1.0;

            if token.len() > 4 {
                // Three characters, not bytes: a chunk can start mid-word.
                let chars = token.chars().collect::<Vec<_>>();
                let prefix = chars.iter().take(3).collect::<String>();
                let suffix = chars[chars.len().saturating_sub(3)..]
                    .iter()
                    .collect::<String>();
                vector[stable_hash(&prefix) % self.dimensions] += 0.4;
                vector[stable_hash(&suffix) % self.dimensions] += 0.4;
            }
        }

        for window in lower.as_bytes().windows(3) {
            let idx = stable_hash_bytes(window) % self.dimensions;
            vector[idx] += 0.05;
        }

        normalize(&mut vector);
        vector
    }
}

fn allow_mock_embedder() -> bool {
    if let Ok(value) = std::env::var("FNDR_ALLOW_MOCK_EMBEDDER") {
        return parse_env_bool(&value);
    }

    if let Ok(value) = std::env::var("FNDR_DISABLE_MOCK_EMBEDDER") {
        if parse_env_bool(&value) {
            return false;
        }
    }

    false
}

fn parse_env_bool(value: &str) -> bool {
    value == "1"
        || value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("yes")
        || value.eq_ignore_ascii_case("on")
}

/// Resolve the directory containing ONNX model files.
/// Priority chain (first match wins):
///   1. FNDR_EMBED_MODEL_DIR env var (new, embed-specific)
///   2. FNDR_MODEL_DIR env var (legacy, any model)
///   3. ~/.fndr/models (user-installed, common for Homebrew/manual installs)
///   4. ProjectDirs data dir / models (app data location)
///   5. CARGO_MANIFEST_DIR/models (dev build fallback)
fn resolve_model_dir(contract: TextEmbeddingContract) -> Option<PathBuf> {
    // 1. FNDR_EMBED_MODEL_DIR (new, dedicated embed env var)
    for env_key in &["FNDR_EMBED_MODEL_DIR", "FNDR_MODEL_DIR"] {
        if let Ok(dir) = std::env::var(env_key) {
            let p = PathBuf::from(&dir);
            if model_assets_present(&p, contract) {
                tracing::info!("Embedder model loaded from ${} = {}", env_key, p.display());
                return Some(p);
            }
            if p.exists() {
                tracing::warn!(
                    "${} is set to {}, but {} or {} is missing. \
                    Download text embeddings with: ./scripts/download_model.sh (or scripts/bootstrap/download-embedding-model.sh).",
                    env_key,
                    p.display(),
                    contract.model_filename,
                    contract.tokenizer_filename
                );
            }
        }
    }

    for (label, dir) in candidate_embedding_model_dirs() {
        if model_assets_present(&dir, contract) {
            tracing::info!("Embedder model found at {} ({label})", dir.display());
            return Some(dir);
        }
    }

    // Fallback: return the canonical app-data models directory if it exists so
    // the caller's error message points at the place onboarding/dev scripts use.
    for (label, dir) in candidate_embedding_model_dirs() {
        if dir.exists() {
            tracing::warn!(
                "Embedder model directory exists at {} ({label}), but {} or {} is missing.",
                dir.display(),
                contract.model_filename,
                contract.tokenizer_filename
            );
            return Some(dir);
        }
    }

    None
}

fn candidate_embedding_model_dirs() -> Vec<(&'static str, PathBuf)> {
    let mut dirs = Vec::new();

    if let Some(home) = dirs::home_dir() {
        // Canonical Tauri 2 app-data path from tauri.conf.json identifier.
        dirs.push((
            "tauri-app-data",
            home.join("Library/Application Support/com.fndr.app/models"),
        ));
        // Legacy path from older README/bootstrap scripts. Keep it readable so
        // existing local downloads still work, but do not make it the default.
        dirs.push((
            "legacy-readme-path",
            home.join("Library/Application Support/com.fndr.FNDR/models"),
        ));
        dirs.push(("user-home", home.join(".fndr").join("models")));
    }

    if let Some(project_models) = directories::ProjectDirs::from("com", "fndr", "FNDR")
        .map(|proj| proj.data_dir().join("models"))
    {
        dirs.push(("project-dirs-legacy", project_models));
    }

    dirs.push((
        "dev-cargo-manifest",
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models"),
    ));

    let mut seen = std::collections::HashSet::new();
    dirs.into_iter()
        .filter(|(_, dir)| seen.insert(dir.clone()))
        .collect()
}

fn model_assets_present(dir: &PathBuf, contract: TextEmbeddingContract) -> bool {
    dir.join(contract.model_filename).exists() && dir.join(contract.tokenizer_filename).exists()
}

/// Outcome of an embedding-environment preflight check.
///
/// Distinguishes "model files on disk + matching contract" from the
/// well-known failure modes so the caller can log an actionable warning
/// before any heavy ONNX load is attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingPreflight {
    /// All assets present and the contract constants are internally
    /// consistent (config dim == module dim == central contract dim).
    Ready { model_dir: PathBuf },
    /// Contract constants disagree across modules. Caller should treat
    /// as a hard build error; production should never see this.
    ContractDrift { detail: String },
    /// No usable model directory could be located on disk. Embedding
    /// will fall back to mock if `FNDR_ALLOW_MOCK_EMBEDDER=1`.
    MissingModelDir {
        searched: Vec<String>,
        detail: String,
    },
    /// Found the directory but the ONNX model file is missing.
    MissingModelFile { model_dir: PathBuf, detail: String },
    /// Found the directory but the tokenizer file is missing.
    MissingTokenizer { model_dir: PathBuf, detail: String },
}

impl EmbeddingPreflight {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// Human-readable, actionable summary suitable for tracing / startup logs.
    pub fn describe(&self) -> String {
        match self {
            Self::Ready { model_dir } => format!(
                "Embedding preflight OK: model={MODEL_NAME} dim={EMBEDDING_DIM} dir={}",
                model_dir.display()
            ),
            Self::ContractDrift { detail } => detail.clone(),
            Self::MissingModelDir { detail, .. } => detail.clone(),
            Self::MissingModelFile { detail, .. } => detail.clone(),
            Self::MissingTokenizer { detail, .. } => detail.clone(),
        }
    }
}

/// Cheap, non-blocking validation of the embedding contract + on-disk assets.
///
/// Run this once at startup (after `Config::load_or_create()`) so the user
/// sees a clear actionable error in the log before any silent mock-fallback
/// engages. This does NOT load the ONNX model — that still happens lazily on
/// the first `Embedder::new()` and probes the actual output dimension there.
pub fn preflight_embedding_environment(
    config: &crate::config::EmbeddingConfig,
) -> EmbeddingPreflight {
    preflight_embedding_contract(config, active_embedding_contract())
}

pub fn preflight_embedding_contract(
    config: &crate::config::EmbeddingConfig,
    contract: TextEmbeddingContract,
) -> EmbeddingPreflight {
    // 1. Contract consistency — config + module + central all agree.
    if let Err(detail) = validate_embedding_config_against_contract(config, contract) {
        return EmbeddingPreflight::ContractDrift { detail };
    }

    // 2. Locate the model directory on disk.
    let Some(model_dir) = resolve_model_dir(contract) else {
        let searched: Vec<String> = candidate_embedding_model_dirs()
            .into_iter()
            .map(|(label, dir)| format!("{label}: {}", dir.display()))
            .collect();
        let detail = format!(
            "No embedding model directory found. Install {} + {} ({}-d {}) before using table {}. \
             Searched: [{}].",
            contract.model_filename,
            contract.tokenizer_filename,
            contract.dimensions,
            contract.model_id,
            contract.table_name,
            searched.join("; ")
        );
        return EmbeddingPreflight::MissingModelDir { searched, detail };
    };

    // 3. Confirm both required files exist.
    let onnx_path = model_dir.join(contract.model_filename);
    if !onnx_path.exists() {
        return EmbeddingPreflight::MissingModelFile {
            model_dir: model_dir.clone(),
            detail: format!(
                "Embedding ONNX model missing at {}. Install the {}-d {} contract assets before writing {}.",
                onnx_path.display(),
                contract.dimensions,
                contract.model_id,
                contract.table_name
            ),
        };
    }
    let tokenizer_path = model_dir.join(contract.tokenizer_filename);
    if !tokenizer_path.exists() {
        return EmbeddingPreflight::MissingTokenizer {
            model_dir: model_dir.clone(),
            detail: format!(
                "Embedding tokenizer missing at {}. Install {} for {} before writing {}.",
                tokenizer_path.display(),
                contract.tokenizer_filename,
                contract.model_id,
                contract.table_name
            ),
        };
    }

    EmbeddingPreflight::Ready { model_dir }
}

fn stable_hash(input: &str) -> usize {
    stable_hash_bytes(input.as_bytes())
}

fn stable_hash_bytes(input: &[u8]) -> usize {
    let mut hash: u64 = 1469598103934665603; // FNV offset
    for b in input {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash as usize
}

/// Keeps the first `dimensions` values and renormalizes: how a Matryoshka
/// model's vector is shortened (EmbeddingGemma's 768 to 256, VS-47). A vector
/// already `dimensions` long is only normalized.
fn truncate_and_normalize(mut embedding: Vec<f32>, dimensions: usize) -> Vec<f32> {
    embedding.truncate(dimensions);
    normalize(&mut embedding);
    embedding
}

fn mean_pool(vectors: &[Vec<f32>], dimensions: usize) -> Vec<f32> {
    if vectors.is_empty() {
        return vec![0.0; dimensions];
    }

    let mut pooled = vec![0.0f32; dimensions];
    for vec in vectors {
        for (idx, value) in vec.iter().enumerate().take(dimensions) {
            pooled[idx] += *value;
        }
    }

    let scale = 1.0 / vectors.len() as f32;
    for value in &mut pooled {
        *value *= scale;
    }

    normalize(&mut pooled);
    pooled
}

fn normalize(vec: &mut [f32]) {
    let norm = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for val in vec {
            *val /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn query_admission_preserves_legacy_text_and_cloned_handle_state() {
        for contract in [active_embedding_contract(), embedding_v5_contract()] {
            let mut original = Embedder::mock_for_tests();
            original.contract = contract;
            original.backend = Backend::Mock(MockEmbedder::new(contract.dimensions));
            let text = super::super::prefixes::query_text_for(contract, "Find release validation");
            let expected = original.embed_batch(&[text.clone()]).unwrap().remove(0);
            let handle = original.clone();
            assert_eq!(handle.embed_query(&text).unwrap(), expected);
            assert!(Arc::ptr_eq(
                &handle.embedding_cache,
                &original.embedding_cache
            ));
            handle.degraded_to_mock.store(true, Ordering::Relaxed);
            assert!(original.degraded_to_mock.load(Ordering::Relaxed));
            let independent = Embedder::mock_for_tests();
            assert!(!independent.degraded_to_mock.load(Ordering::Relaxed));
            assert!(!Arc::ptr_eq(
                &independent.embedding_cache,
                &original.embedding_cache
            ));
        }
    }

    #[test]
    fn cached_embedder_retries_missing_assets_then_reuses_success() {
        let cell = OnceLock::new();
        assert_eq!(
            cached_embedder(&cell, || Err("model missing".into()))
                .err()
                .as_deref(),
            Some("model missing")
        );
        assert_eq!(
            cached_embedder(&cell, || Err("tokenizer missing".into()))
                .err()
                .as_deref(),
            Some("tokenizer missing")
        );
        let ready = cached_embedder(&cell, || Ok(Embedder::mock_for_tests()))
            .expect("recover after install");
        let reused =
            cached_embedder(&cell, || panic!("must reuse successful initialization")).unwrap();
        assert!(std::ptr::eq(ready, reused));
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "requires pinned real MiniLM assets via FNDR_EMBED_MODEL_DIR"]
    fn real_model_registry_retries_isolates_and_releases() {
        let contract = active_embedding_contract();
        let source =
            PathBuf::from(std::env::var_os("FNDR_EMBED_MODEL_DIR").expect("pinned assets"))
                .canonicalize()
                .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let first_dir = temp.path().join("first");
        let second_dir = temp.path().join("second");
        std::fs::create_dir(&first_dir).unwrap();
        assert!(RealEmbedder::shared_from_dir(contract, first_dir.clone()).is_err());
        std::fs::create_dir(&second_dir).unwrap();
        for dir in [&first_dir, &second_dir] {
            for file in [contract.model_filename, contract.tokenizer_filename] {
                std::os::unix::fs::symlink(source.join(file), dir.join(file)).unwrap();
            }
        }
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let path = first_dir.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    RealEmbedder::shared_from_dir(contract, path).unwrap()
                })
            })
            .collect();
        let models: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert!(models.iter().all(|model| Arc::ptr_eq(model, &models[0])));
        let alias_dir = temp.path().join("alias");
        std::os::unix::fs::symlink(&first_dir, &alias_dir).unwrap();
        let alias = RealEmbedder::shared_from_dir(contract, alias_dir).unwrap();
        assert!(Arc::ptr_eq(&models[0], &alias));
        let other_path = RealEmbedder::shared_from_dir(contract, second_dir).unwrap();
        assert!(!Arc::ptr_eq(&models[0], &other_path));
        let mut other_contract = contract;
        other_contract.max_sequence_length /= 2;
        let other_model = RealEmbedder::shared_from_dir(other_contract, first_dir.clone()).unwrap();
        assert!(!Arc::ptr_eq(&models[0], &other_model));
        let released = Arc::downgrade(&models[0]);
        drop(models);
        assert!(released.upgrade().is_some());
        drop(alias);
        assert!(
            released.upgrade().is_none(),
            "registry must not keep idle weights alive"
        );
        let reloaded = RealEmbedder::shared_from_dir(contract, first_dir).unwrap();
        let text = ["Find the release validation checklist".to_string()];
        assert_eq!(
            reloaded.embed_batch(&text).unwrap(),
            other_path.embed_batch(&text).unwrap()
        );
    }

    #[test]
    #[ignore = "requires pinned real MiniLM assets via FNDR_EMBED_MODEL_DIR"]
    fn real_model_session_is_shared_without_changing_chunking() {
        let contract = active_embedding_contract();
        let narrow = ChunkingConfig {
            max_tokens: 32,
            overlap_tokens: 4,
            min_tokens: 1,
            ..Default::default()
        };
        let capture = Embedder::with_contract_and_chunking_config(contract, &narrow, false)
            .expect("real capture embedder");
        let search = Embedder::with_contract_and_chunking_config(
            contract,
            &ChunkingConfig::default(),
            false,
        )
        .expect("real search embedder");
        let (Backend::Real(capture_model), Backend::Real(search_model)) =
            (&capture.backend, &search.backend)
        else {
            panic!("real model required")
        };
        assert!(
            Arc::ptr_eq(capture_model, search_model),
            "capture and search must share the resident model session"
        );
        let text = "The release checklist documents validation and deployment steps. ".repeat(24);
        assert!(capture.chunk_text(&text).len() > search.chunk_text(&text).len());
        let query = vec!["Find the release validation checklist".to_string()];
        assert_eq!(
            capture.embed_batch(&query).unwrap(),
            search.embed_batch(&query).unwrap()
        );
    }

    #[test]
    fn role_prompt_reaches_every_long_document_chunk() {
        use crate::embedding::prefixes::EMBEDDING_GEMMA_DOCUMENT_PREFIX;
        let mut embedder = Embedder::mock_for_tests();
        embedder.contract = crate::inference::model_config::embedding_v6_contract(256).unwrap();
        embedder.backend = Backend::Mock(MockEmbedder::new(256));
        let text = (0..500)
            .map(|i| format!("Section {i}: the migration keeps cited source passages and the rollback checkpoint. "))
            .collect::<String>();
        embedder
            .embed_inputs(&[EmbeddingInput::Document {
                text: &text,
                app_name: "",
                window_title: "",
            }])
            .unwrap();
        let cache = embedder.embedding_cache.lock().unwrap();
        assert!(
            cache.values.len() >= 3,
            "exercise later chunks, not just the first"
        );
        assert!(
            cache
                .values
                .keys()
                .all(|chunk| chunk.starts_with(EMBEDDING_GEMMA_DOCUMENT_PREFIX)),
            "every model input, including the final chunk, needs the document prompt"
        );
    }

    #[test]
    fn role_inputs_preserve_order_context_and_cache_identity() {
        use crate::embedding::prefixes::{
            EMBEDDING_GEMMA_DOCUMENT_PREFIX, EMBEDDING_GEMMA_QUERY_PREFIX,
        };
        let mut embedder = Embedder::mock_for_tests();
        embedder.contract = crate::inference::model_config::embedding_v6_contract(256).unwrap();
        embedder.backend = Backend::Mock(MockEmbedder::new(256));
        let text = "The release checkpoint includes migration notes and rollback steps.";
        let inputs = [
            EmbeddingInput::Query(text),
            EmbeddingInput::Document {
                text,
                app_name: "Editor",
                window_title: "Release checklist",
            },
            EmbeddingInput::Query("  "),
            EmbeddingInput::Document {
                text,
                app_name: "",
                window_title: "",
            },
            EmbeddingInput::Query(text),
        ];
        let vectors = embedder.embed_inputs(&inputs).unwrap();
        assert_eq!(vectors.len(), inputs.len());
        for (input, vector) in inputs.iter().zip(&vectors) {
            assert_eq!(
                embedder.embed_inputs(&[*input]).unwrap(),
                vec![vector.clone()]
            );
        }
        assert_eq!(vectors[0], vectors[4]);
        assert_ne!(vectors[0], vectors[3], "roles need distinct model inputs");
        assert!(vectors[2].iter().all(|value| *value == 0.0));
        let cache = embedder.embedding_cache.lock().unwrap();
        assert_eq!(
            cache.values.len(),
            3,
            "blank input creates no prompt and duplicate queries reuse cache"
        );
        assert!(cache
            .values
            .contains_key(&format!("{EMBEDDING_GEMMA_QUERY_PREFIX}{text}")));
        assert!(cache
            .values
            .contains_key(&format!("{EMBEDDING_GEMMA_DOCUMENT_PREFIX}{text}")));
        assert!(
            cache.values.keys().any(|chunk| chunk.starts_with(&format!(
                "{EMBEDDING_GEMMA_DOCUMENT_PREFIX}Release checklist\n"
            )) && chunk.ends_with(text)),
            "document prompt must precede title and body"
        );
    }

    #[test]
    fn role_prompt_reaches_every_long_query_chunk() {
        use crate::embedding::prefixes::EMBEDDING_GEMMA_QUERY_PREFIX;
        let mut embedder = Embedder::mock_for_tests();
        embedder.contract = crate::inference::model_config::embedding_v6_contract(256).unwrap();
        embedder.backend = Backend::Mock(MockEmbedder::new(256));
        let text = (0..100).map(|i| format!("Find context for question {i} about the index migration and rollback evidence. ")).collect::<String>();
        let vectors = embedder
            .embed_inputs(&[EmbeddingInput::Query(&text)])
            .unwrap();
        assert_eq!(vectors.len(), 1);
        assert!(vectors[0].iter().any(|value| *value != 0.0));
        let cache = embedder.embedding_cache.lock().unwrap();
        assert!(cache.values.len() >= 3);
        assert!(cache
            .values
            .keys()
            .all(|chunk| chunk.starts_with(EMBEDDING_GEMMA_QUERY_PREFIX)
                && chunk.matches(EMBEDDING_GEMMA_QUERY_PREFIX).count() == 1));
    }

    #[test]
    fn role_prompts_do_not_turn_low_signal_into_content() {
        let mut embedder = Embedder::mock_for_tests();
        embedder.contract = crate::inference::model_config::embedding_v6_contract(256).unwrap();
        embedder.backend = Backend::Mock(MockEmbedder::new(256));
        for text in ["", " \n ", "!!!", "ab"] {
            let vectors = embedder
                .embed_inputs(&[
                    EmbeddingInput::Query(text),
                    EmbeddingInput::Document {
                        text,
                        app_name: "",
                        window_title: "",
                    },
                ])
                .unwrap();
            assert_eq!(vectors, vec![vec![0.0; 256]; 2]);
        }
        assert!(embedder.embedding_cache.lock().unwrap().values.is_empty());
    }

    #[test]
    fn legacy_embedding_wrappers_keep_unprompted_inputs() {
        for contract in [
            crate::inference::model_config::embedding_v4_contract(),
            embedding_v5_contract(),
        ] {
            let mut embedder = Embedder::mock_for_tests();
            embedder.contract = contract;
            embedder.backend = Backend::Mock(MockEmbedder::new(contract.dimensions));
            let text = "The source passages include the release checkpoint and rollback steps.";
            let plain = embedder.embed_batch(&[text.into()]).unwrap();
            assert_eq!(
                plain,
                embedder
                    .embed_batch_with_context(&[("".into(), "".into(), text.into())])
                    .unwrap()
            );
            let cache = embedder.embedding_cache.lock().unwrap();
            assert_eq!(cache.values.len(), 1);
            assert!(cache.values.contains_key(text));
        }
    }

    #[test]
    fn a_matryoshka_vector_is_cut_then_renormalized() {
        let full = vec![3.0, 4.0, 12.0];
        let cut = truncate_and_normalize(full.clone(), 2);
        assert_eq!(cut.len(), 2);
        assert!((cut[0] - 0.6).abs() < 1e-6 && (cut[1] - 0.8).abs() < 1e-6);
        let same = truncate_and_normalize(full, 3);
        assert_eq!(same.len(), 3);
        assert!((same.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn only_embeddinggemma_may_be_truncated() {
        use crate::inference::model_config::{
            embedding_v4_contract, embedding_v5_contract, embedding_v6_contract,
        };
        let short = embedding_v6_contract(256).expect("supported dimension");
        assert!(short.supports_truncation());
        assert_eq!(short.dimensions, 256);
        assert_eq!(short.table_name, "memories_v6_embeddinggemma_256");
        assert_eq!(
            embedding_v6_contract(768)
                .expect("supported dimension")
                .dimensions,
            768
        );
        assert!(!embedding_v4_contract().supports_truncation());
        assert!(!embedding_v5_contract().supports_truncation());
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn preflight_flags_config_dimension_drift() {
        let mut config = crate::config::EmbeddingConfig::default();
        config.dimension = 256;
        let outcome = preflight_embedding_environment(&config);
        assert!(!outcome.is_ready());
        match outcome {
            EmbeddingPreflight::ContractDrift { detail } => {
                assert!(detail.contains("contract drift"));
                assert!(detail.contains("256"));
            }
            other => panic!("expected ContractDrift, got {other:?}"),
        }
    }

    #[test]
    fn preflight_flags_config_filename_drift() {
        let mut config = crate::config::EmbeddingConfig::default();
        config.model_filename = "bge-large-en-v1.5-quantized.onnx".to_string();
        let outcome = preflight_embedding_environment(&config);
        match outcome {
            EmbeddingPreflight::ContractDrift { detail } => {
                assert!(detail.contains("model_filename"));
                assert!(detail.contains("bge-large-en-v1.5"));
            }
            other => panic!("expected ContractDrift for filename, got {other:?}"),
        }
    }

    #[test]
    fn similar_phrases_score_higher_than_unrelated() {
        std::env::set_var("FNDR_ALLOW_MOCK_EMBEDDER", "1");
        let embedder = Embedder::new().expect("embedder should initialize in tests");
        let phrases = vec![
            "schedule project kickoff meeting with alice".to_string(),
            "plan kickoff meeting with alice for the project".to_string(),
            "buy groceries and cook dinner tonight".to_string(),
        ];
        let embeddings = embedder
            .embed_batch(&phrases)
            .expect("embedding should work");

        let similar = cosine(&embeddings[0], &embeddings[1]);
        let unrelated = cosine(&embeddings[0], &embeddings[2]);

        assert!(
            similar > unrelated,
            "expected similar phrases ({similar}) to outrank unrelated ({unrelated})"
        );
    }

    #[test]
    fn mock_embeds_multibyte_words_without_panicking() {
        // VS-68 corpus case an-008: a chunk that starts mid-word gave the token
        // "ot\u{e9}e", and byte slicing split its accented 'e'.
        let mock = MockEmbedder::new(EMBEDDING_DIM);
        let vectors = mock.embed_batch(&["ot\u{e9}e, caf\u{e9}".to_string()]);
        assert!(vectors[0].iter().any(|value| *value != 0.0));
        // ASCII words keep the vectors they had.
        let ascii = mock.embed_single("parser");
        let mut expected = vec![0.0f32; EMBEDDING_DIM];
        expected[stable_hash("parser") % EMBEDDING_DIM] += 1.0;
        expected[stable_hash("par") % EMBEDDING_DIM] += 0.4;
        expected[stable_hash("ser") % EMBEDDING_DIM] += 0.4;
        for window in b"parser".windows(3) {
            expected[stable_hash_bytes(window) % EMBEDDING_DIM] += 0.05;
        }
        normalize(&mut expected);
        assert_eq!(ascii, expected);
    }

    #[test]
    fn mock_embedding_vectors_match_schema_dimension() {
        let vectors =
            MockEmbedder::new(EMBEDDING_DIM).embed_batch(&["dimension probe".to_string()]);
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), EMBEDDING_DIM);
    }

    #[test]
    fn shared_bge_query_embedder_reuses_success_and_retries_failure() {
        let shared = SharedBgeQueryEmbedder::default();
        let initializations = AtomicUsize::new(0);
        let factory = || {
            initializations.fetch_add(1, Ordering::Relaxed);
            Ok(Embedder {
                contract: embedding_v5_contract(),
                chunker: TextChunker::new(),
                backend: Backend::Mock(MockEmbedder::new(embedding_v5_contract().dimensions)),
                degraded_to_mock: Arc::new(AtomicBool::new(false)),
                allow_mock_fallback: false,
                embedding_cache: Arc::new(Mutex::new(EmbeddingCache::new(8))),
            })
        };

        let first = shared
            .get_or_try_init(factory)
            .expect("first initialization");
        let second = shared
            .get_or_try_init(factory)
            .expect("cached initialization");

        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(initializations.load(Ordering::Relaxed), 1);

        let retryable = SharedBgeQueryEmbedder::default();
        let attempts = AtomicUsize::new(0);
        let result = retryable.get_or_try_init(|| {
            attempts.fetch_add(1, Ordering::Relaxed);
            Err("model missing".to_string())
        });
        assert_eq!(result.err().as_deref(), Some("model missing"));
        assert!(retryable
            .get_or_try_init(|| {
                attempts.fetch_add(1, Ordering::Relaxed);
                Err("model still missing".to_string())
            })
            .is_err());
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn embedding_model_dirs_prefer_tauri_identifier_path_before_legacy_readme_path() {
        let dirs = candidate_embedding_model_dirs();
        let canonical = dirs
            .iter()
            .position(|(label, _)| *label == "tauri-app-data")
            .expect("canonical app-data models dir");
        let legacy = dirs
            .iter()
            .position(|(label, _)| *label == "legacy-readme-path")
            .expect("legacy README models dir");

        assert!(
            canonical < legacy,
            "com.fndr.app must be searched before the legacy com.fndr.FNDR path"
        );
    }
}
