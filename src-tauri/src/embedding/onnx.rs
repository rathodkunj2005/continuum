//! Local text embedding backend via native ONNX Runtime.

use super::{chunk_screen_text, TextChunker};
use crate::config::{
    ChunkingConfig, DEFAULT_EMBEDDING_CACHE_CAPACITY, DEFAULT_EMBEDDING_MODEL_NAME,
    DEFAULT_TEXT_EMBEDDING_DIM,
};
use crate::inference::model_config::{
    active_embedding_contract, embedding_v5_contract, validate_embedding_config_against_contract,
    TextEmbeddingContract,
};
use ndarray::Array2;
use ort::session::Session;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Authoritative text embedding dimension for the primary semantic index.
pub const EMBEDDING_DIM: usize = DEFAULT_TEXT_EMBEDDING_DIM;
const MODEL_NAME: &str = DEFAULT_EMBEDDING_MODEL_NAME;
const EMBEDDING_CACHE_CAPACITY: usize = DEFAULT_EMBEDDING_CACHE_CAPACITY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingBackend {
    Real,
    Mock,
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

/// Embedder with pluggable backend.
pub struct Embedder {
    contract: TextEmbeddingContract,
    chunker: TextChunker,
    backend: Backend,
    degraded_to_mock: AtomicBool,
    allow_mock_fallback: bool,
    embedding_cache: Mutex<EmbeddingCache>,
}

enum Backend {
    Real(RealEmbedder),
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
                    degraded_to_mock: AtomicBool::new(false),
                    allow_mock_fallback,
                    embedding_cache: Mutex::new(EmbeddingCache::new(EMBEDDING_CACHE_CAPACITY)),
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
                        degraded_to_mock: AtomicBool::new(true),
                        allow_mock_fallback,
                        embedding_cache: Mutex::new(EmbeddingCache::new(EMBEDDING_CACHE_CAPACITY)),
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

    pub fn dimension(&self) -> usize {
        self.contract.dimensions
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
        self.embed_chunk_groups(chunk_groups)
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
        self.embed_chunk_groups(chunk_groups)
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

    fn embed_chunks_cached(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
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
            for chunk in missing_unique.chunks(self.contract.max_batch_size.max(1)) {
                let batch = chunk.to_vec();
                let vectors = self.backend_embed_batch(&batch)?;
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

    fn embed_chunk_groups(&self, chunk_groups: Vec<Vec<String>>) -> Result<Vec<Vec<f32>>, String> {
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

        let chunk_embeddings = self.embed_chunks_cached(&flattened_chunks)?;
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

    fn backend_embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        match &self.backend {
            Backend::Real(real) => {
                if self.degraded_to_mock.load(Ordering::Relaxed) {
                    return Ok(MockEmbedder::new(self.dimension()).embed_batch(texts));
                }

                match real.embed_batch(texts) {
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
    contract: TextEmbeddingContract,
    session: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
    input_names: Vec<String>,
    output_name: String,
}

impl RealEmbedder {
    fn new(contract: TextEmbeddingContract) -> Result<Self, String> {
        let model_dir = resolve_model_dir(contract)
            .ok_or_else(|| "Could not determine model directory".to_string())?;

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
        let output_name = session
            .outputs()
            .iter()
            .find(|output| output.name() == "last_hidden_state")
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
        let embedder = Self {
            contract,
            session: Mutex::new(session),
            tokenizer,
            input_names,
            output_name,
        };

        let probe = embedder.embed_batch(&["FNDR embedding dimension probe".to_string()])?;
        let actual_dim = probe.first().map(|vector| vector.len()).unwrap_or(0);
        if actual_dim != contract.dimensions {
            return Err(format!(
                "Embedding dimension mismatch for {}: model file at {} returned {actual_dim}-d vectors but the FNDR contract expects {}-d for table {}. \
                 Ensure FNDR_MODEL_DIR points at a directory containing {} + {} for the active contract.",
                contract.model_id,
                onnx_path.display(),
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
        Ok(embedder)
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let t_onnx = std::time::Instant::now();
        let encodings = self
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
        let mut session_guard = self
            .session
            .lock()
            .map_err(|e| format!("Session mutex poisoned: {e}"))?;
        let mut inputs = ort::inputs![
            "input_ids" => ids_t,
            "attention_mask" => mask_t,
        ];
        if self.input_names.iter().any(|name| name == "token_type_ids") {
            inputs.push((Cow::from("token_type_ids"), types_t.into()));
        }

        let outputs = session_guard
            .run(inputs)
            .map_err(|e| format!("ONNX inference failed: {e}"))?;

        let output = outputs
            .get(&self.output_name)
            .or_else(|| outputs.get("last_hidden_state"))
            .or_else(|| outputs.get("token_embeddings"))
            .or_else(|| {
                let first_key = outputs.keys().next()?;
                outputs.get(first_key)
            })
            .ok_or_else(|| {
                format!(
                    "ONNX inference produced no usable embedding output. Expected '{}'",
                    self.output_name
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
        if actual_dim != self.contract.dimensions {
            return Err(format!(
                "Unexpected hidden state dim {actual_dim}, expected {} for {}",
                self.contract.dimensions, self.contract.model_id
            ));
        }

        let mut embeddings = Vec::with_capacity(batch_size);
        match shape_dims.as_slice() {
            [actual_batch, actual_dim] if *actual_dim == self.contract.dimensions => {
                for i in 0..batch_size.min(*actual_batch) {
                    let offset = i * self.contract.dimensions;
                    let mut embedding = data[offset..offset + self.contract.dimensions].to_vec();
                    normalize(&mut embedding);
                    embeddings.push(embedding);
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

#[derive(Debug)]
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
                let prefix = &token[..3];
                let suffix = &token[token.len() - 3..];
                vector[stable_hash(prefix) % self.dimensions] += 0.4;
                vector[stable_hash(suffix) % self.dimensions] += 0.4;
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
    fn mock_embedding_vectors_match_schema_dimension() {
        let vectors =
            MockEmbedder::new(EMBEDDING_DIM).embed_batch(&["dimension probe".to_string()]);
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), EMBEDDING_DIM);
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
