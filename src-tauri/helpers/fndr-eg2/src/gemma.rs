//! EmbeddingGemma 2 on llama.cpp: the text model's mean-pooled, L2-normalized
//! output is the vector. A text-only port of DigUp's `LlamaEmbedder` (MIT), on
//! llama.cpp's C API.
//!
//! Inputs are framed as the reference frames them: `<bos> text <eos>`. The
//! model is non-causal with no KV cache, so each whole input goes through one
//! `llama_process` call, several inputs per call as separate sequences.
//!
//! Runs only in `fndr-eg2-helper`, never beside Qwen in the app: it links its
//! own llama.cpp (scripts/eg2/build-llama.sh) and initializes the backend
//! itself. One thread at a time.

use crate::sys;
use std::ffi::CString;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Once;

/// Which vectors this pipeline makes. It changes only when the vectors do;
/// the reference-vector check says when that is. Indexes record it.
pub const VECTOR_VERSION: u32 = 1;

/// One input must fit one micro-batch: 2048 tokens, the reference's cut.
const MAX_TOKENS: usize = 2048;
const MAX_SEQUENCES: i32 = 16;

static BACKEND: Once = Once::new();

pub struct GemmaEmbedder {
    model: NonNull<sys::llama_model>,
    context: NonNull<sys::llama_context>,
    batch: NonNull<sys::llama_batch_ext>,
    vocab: *const sys::llama_vocab,
    embedding_size: usize,
    max_tokens: usize,
    sequences: i32,
    tokens: usize,
    position: sys::llama_pos,
}

// The raw handles are owned by this value and used from one thread at a time.
unsafe impl Send for GemmaEmbedder {}

impl GemmaEmbedder {
    pub fn load(text_model: &Path, cpu_only: bool) -> Result<Self, String> {
        BACKEND.call_once(|| unsafe { sys::llama_backend_init() });
        let text_path = c_path(text_model)?;
        let mut model_params = unsafe { sys::llama_model_default_params() };
        model_params.n_gpu_layers = if cpu_only { 0 } else { 999 };
        let model = NonNull::new(unsafe {
            sys::llama_model_load_from_file(text_path.as_ptr(), model_params)
        })
        .ok_or_else(|| format!("can't load {}", text_model.display()))?;
        let max_tokens = MAX_TOKENS;
        let mut params = unsafe { sys::llama_context_default_params() };
        params.n_ctx = max_tokens as u32;
        params.n_batch = max_tokens as u32;
        params.n_ubatch = max_tokens as u32;
        params.n_seq_max = MAX_SEQUENCES as u32;
        params.kv_unified = true; // each input may use the whole context, as in llama-embedding
        params.embeddings = true;
        params.pooling_type = sys::LLAMA_POOLING_TYPE_MEAN;
        let Some(context) =
            NonNull::new(unsafe { sys::llama_init_from_model(model.as_ptr(), params) })
        else {
            unsafe { sys::llama_model_free(model.as_ptr()) };
            return Err("can't create a llama.cpp context".into());
        };
        let batch = NonNull::new(unsafe { sys::llama_batch_ext_init(context.as_ptr()) })
            .ok_or("can't make a batch")?;
        let mut embedder = Self {
            model,
            context,
            batch,
            vocab: unsafe { sys::llama_model_get_vocab(model.as_ptr()) },
            embedding_size: unsafe { sys::llama_model_n_embd_out(model.as_ptr()) } as usize,
            max_tokens,
            sequences: 0,
            tokens: 0,
            position: 0,
        };
        // The first calls build the Metal pipelines: pay that now.
        for _ in 0..2 {
            embedder.embed_texts(&["task: search result | query: warm up".to_string()])?;
        }
        Ok(embedder)
    }

    pub fn dimension(&self) -> usize {
        self.embedding_size
    }

    /// Whether `text` fits this context whole; longer text is cut, as the
    /// reference cuts it.
    pub fn fits(&self, text: &str) -> bool {
        self.tokenize(text)
            .is_ok_and(|tokens| tokens.len() < self.max_tokens)
    }

    pub fn embed_texts(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.reset();
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            let tokens = self.tokenize(text)?;
            if self.sequences == MAX_SEQUENCES || self.tokens + tokens.len() > self.max_tokens {
                vectors.extend(self.run()?);
            }
            self.begin();
            for token in tokens {
                self.add_token(token)?;
            }
        }
        if self.sequences > 0 {
            vectors.extend(self.run()?);
        }
        Ok(vectors)
    }

    /// `<bos> text <eos>`, cut to the context the way transformers cuts (keeping both special tokens).
    fn tokenize(&self, text: &str) -> Result<Vec<sys::llama_token>, String> {
        let bytes = text.as_bytes();
        let mut tokens = vec![0 as sys::llama_token; bytes.len() + 8];
        let mut count = unsafe {
            sys::llama_tokenize(
                self.vocab,
                bytes.as_ptr().cast(),
                bytes.len() as i32,
                tokens.as_mut_ptr(),
                tokens.len() as i32,
                true,
                true,
            )
        };
        if count < 0 {
            tokens = vec![0; (-count) as usize];
            count = unsafe {
                sys::llama_tokenize(
                    self.vocab,
                    bytes.as_ptr().cast(),
                    bytes.len() as i32,
                    tokens.as_mut_ptr(),
                    tokens.len() as i32,
                    true,
                    true,
                )
            };
        }
        if count < 0 {
            return Err("can't tokenize".into());
        }
        tokens.truncate(count as usize);
        if tokens.len() > self.max_tokens {
            tokens.truncate(self.max_tokens - 1);
            tokens.push(unsafe { sys::llama_vocab_eos(self.vocab) });
        }
        Ok(tokens)
    }

    fn begin(&mut self) {
        self.sequences += 1;
        self.position = 0;
    }

    fn reset(&mut self) {
        unsafe { sys::llama_batch_ext_clear(self.batch.as_ptr()) };
        self.sequences = 0;
        self.tokens = 0;
    }

    fn add_token(&mut self, token: sys::llama_token) -> Result<(), String> {
        let index = unsafe {
            sys::llama_batch_ext_add_token(self.batch.as_ptr(), self.sequences - 1, token)
        };
        self.place(index)
    }

    fn place(&mut self, index: i32) -> Result<(), String> {
        if index < 0 {
            return Err(format!("batch full ({index})"));
        }
        let position = [self.position, 0, 0, 0];
        let placed = unsafe {
            sys::llama_batch_ext_set_pos(self.batch.as_ptr(), index, position.as_ptr())
                && sys::llama_batch_ext_set_output_embd(self.batch.as_ptr(), index, true)
        };
        if !placed {
            return Err(format!("can't place token {index}"));
        }
        self.position += 1;
        self.tokens += 1;
        Ok(())
    }

    /// Runs the text model over the batch; one L2-normalized vector per input, in order.
    fn run(&mut self) -> Result<Vec<Vec<f32>>, String> {
        let status = unsafe {
            sys::llama_process(
                self.context.as_ptr(),
                sys::LLAMA_PROCESS_TYPE_ENCODE,
                self.batch.as_ptr(),
            )
        };
        let sequences = self.sequences;
        self.reset();
        if status != 0 {
            return Err(format!("llama_process failed ({status})"));
        }
        (0..sequences)
            .map(|sequence| {
                let pooled =
                    unsafe { sys::llama_get_embeddings_seq(self.context.as_ptr(), sequence) };
                if pooled.is_null() {
                    return Err(format!("no pooled embedding for input {sequence}"));
                }
                let pooled = unsafe { std::slice::from_raw_parts(pooled, self.embedding_size) };
                Ok(normalize(pooled))
            })
            .collect()
    }
}

impl Drop for GemmaEmbedder {
    fn drop(&mut self) {
        unsafe {
            sys::llama_batch_ext_free(self.batch.as_ptr());
            sys::llama_free(self.context.as_ptr());
            sys::llama_model_free(self.model.as_ptr());
        }
    }
}

fn c_path(path: &Path) -> Result<CString, String> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| format!("bad path {}", path.display()))
}

/// L2-normalized, in f64 so long sums keep their precision.
pub fn normalize(values: &[f32]) -> Vec<f32> {
    let sum: f64 = values.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
    let scale = if sum > 0.0 { 1.0 / sum.sqrt() } else { 0.0 };
    values
        .iter()
        .map(|v| (f64::from(*v) * scale) as f32)
        .collect()
}
