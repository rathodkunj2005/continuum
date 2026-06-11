//! Canonical embedding document and provenance helpers for memory storage.
//!
//! This module is the capture-to-storage contract for retrieval text. It keeps
//! text, image, graph, and BGE reindex vectors in separate vector spaces while
//! making their source text, roles, and provenance inspectable from one place.

use crate::config::{ChunkingConfig, DEFAULT_IMAGE_EMBEDDING_DIM, DEFAULT_TEXT_EMBEDDING_DIM};
use crate::graph::schema::{GraphNode, GraphNodeType};
use crate::inference::model_config::{embedding_v4_contract, embedding_v5_contract};
use crate::memory_compaction::{
    best_embedding_text, best_snippet_embedding_text, best_support_embedding_texts_with_config,
};
use crate::memory_insight::compose_insight_embedding_text;
use crate::storage::MemoryRecord;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const EMBEDDING_DOCUMENT_VERSION: u32 = 1;
pub const EMBEDDING_MANIFEST_KEY: &str = "embedding_manifest";

const PRIMARY_TEXT_MAX_CHARS: usize = 2_000;
const CHUNK_SOURCE_MAX_CHARS: usize = 8_000;
const GRAPH_NODE_TEXT_MAX_CHARS: usize = 1_200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingRole {
    Primary,
    Snippet,
    Support,
    Chunk,
    Image,
    GraphNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingVectorSpace {
    TextMiniLm384,
    TextBge1024,
    ClipImage512,
    GraphNodeText384,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingStatus {
    Ready,
    ZeroVectorFallback,
    Unavailable,
    StaleSourceText,
    DimensionMismatch,
    LegacyUnverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualSemanticSource {
    None,
    TextCapture,
    PixelVlmOrOcrGrounded,
    ClipMetadataFallback,
    ClipImageEmbedding,
    VisualSemanticsFailed,
    LlmOcrGrounded,
    OcrOnly,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingSourceHash {
    pub role: EmbeddingRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<usize>,
    pub sha256: String,
    pub char_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingContract {
    pub role: EmbeddingRole,
    pub vector_space: EmbeddingVectorSpace,
    pub model_id: String,
    pub dimension: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingRoleStatus {
    pub role: EmbeddingRole,
    pub status: EmbeddingStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingManifest {
    pub version: u32,
    pub document_version: u32,
    pub source_hashes: Vec<EmbeddingSourceHash>,
    pub contracts: Vec<EmbeddingContract>,
    pub statuses: Vec<EmbeddingRoleStatus>,
    pub visual_semantic_source: VisualSemanticSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEmbeddingRoleProvenance {
    pub role: EmbeddingRole,
    pub status: EmbeddingStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_space: Option<EmbeddingVectorSpace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(default)]
    pub source_char_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEmbeddingProvenance {
    pub document_version: u32,
    pub roles: Vec<SearchEmbeddingRoleProvenance>,
    pub visual_semantic_source: VisualSemanticSource,
    #[serde(default)]
    pub status_labels: Vec<String>,
}

impl SearchEmbeddingProvenance {
    pub fn role(&self, role: EmbeddingRole) -> Option<&SearchEmbeddingRoleProvenance> {
        self.roles.iter().find(|provenance| provenance.role == role)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingRetrievalAdjustment {
    pub role: EmbeddingRole,
    pub status: EmbeddingStatus,
    pub score_multiplier: f32,
    pub reason_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEmbeddingDocument {
    pub primary_text: String,
    pub snippet_text: String,
    pub support_texts: Vec<String>,
    pub chunk_source_text: String,
    pub visual_semantic_text: Option<String>,
}

impl MemoryEmbeddingDocument {
    pub fn text_embedding_inputs(&self) -> Vec<String> {
        let mut inputs = vec![self.primary_text.clone(), self.snippet_text.clone()];
        inputs.extend(self.support_texts.iter().cloned());
        inputs
    }

    pub fn source_hashes(&self) -> Vec<EmbeddingSourceHash> {
        let mut hashes = vec![
            source_hash(EmbeddingRole::Primary, None, &self.primary_text),
            source_hash(EmbeddingRole::Snippet, None, &self.snippet_text),
            source_hash(EmbeddingRole::Chunk, None, &self.chunk_source_text),
        ];
        for (index, text) in self.support_texts.iter().enumerate() {
            hashes.push(source_hash(EmbeddingRole::Support, Some(index), text));
        }
        if let Some(visual) = self.visual_semantic_text.as_deref() {
            hashes.push(source_hash(EmbeddingRole::Image, None, visual));
        }
        hashes
    }
}

pub fn compose_memory_embedding_document(
    record: &MemoryRecord,
    chunking_cfg: Option<&ChunkingConfig>,
) -> MemoryEmbeddingDocument {
    let insight_text = normalize_document_text(&compose_insight_embedding_text(record));
    let primary_text = if insight_text.is_empty() {
        trim_chars(&best_embedding_text(record), PRIMARY_TEXT_MAX_CHARS)
    } else {
        trim_chars(&insight_text, PRIMARY_TEXT_MAX_CHARS)
    };

    let snippet_text = best_snippet_embedding_text(record);
    let support_texts = best_support_embedding_texts_with_config(record, chunking_cfg);
    let chunk_source_text = chunk_source_text(record);
    let visual_semantic_text = visual_semantic_text(record);

    MemoryEmbeddingDocument {
        primary_text,
        snippet_text,
        support_texts,
        chunk_source_text,
        visual_semantic_text,
    }
}

pub fn source_hash(
    role: EmbeddingRole,
    ordinal: Option<usize>,
    source_text: &str,
) -> EmbeddingSourceHash {
    let normalized = normalize_document_text(source_text);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    EmbeddingSourceHash {
        role,
        ordinal,
        sha256: format!("{:x}", hasher.finalize()),
        char_count: normalized.chars().count(),
    }
}

pub fn build_embedding_manifest(
    document: &MemoryEmbeddingDocument,
    primary_status: EmbeddingStatus,
    image_status: EmbeddingStatus,
    visual_source: VisualSemanticSource,
) -> EmbeddingManifest {
    EmbeddingManifest {
        version: 1,
        document_version: EMBEDDING_DOCUMENT_VERSION,
        source_hashes: document.source_hashes(),
        contracts: vec![
            minilm_contract(EmbeddingRole::Primary),
            minilm_contract(EmbeddingRole::Snippet),
            minilm_contract(EmbeddingRole::Support),
            clip_image_contract(),
        ],
        statuses: vec![
            EmbeddingRoleStatus {
                role: EmbeddingRole::Primary,
                status: primary_status,
                reason: None,
            },
            EmbeddingRoleStatus {
                role: EmbeddingRole::Image,
                status: image_status,
                reason: None,
            },
        ],
        visual_semantic_source: visual_source,
    }
}

pub fn minilm_contract(role: EmbeddingRole) -> EmbeddingContract {
    let contract = embedding_v4_contract();
    EmbeddingContract {
        role,
        vector_space: EmbeddingVectorSpace::TextMiniLm384,
        model_id: contract.model_id.to_string(),
        dimension: contract.dimensions,
        table_name: Some(contract.table_name.to_string()),
    }
}

pub fn bge_v5_contract_for(role: EmbeddingRole) -> EmbeddingContract {
    let contract = embedding_v5_contract();
    EmbeddingContract {
        role,
        vector_space: EmbeddingVectorSpace::TextBge1024,
        model_id: contract.model_id.to_string(),
        dimension: contract.dimensions,
        table_name: Some(contract.table_name.to_string()),
    }
}

pub fn clip_image_contract() -> EmbeddingContract {
    EmbeddingContract {
        role: EmbeddingRole::Image,
        vector_space: EmbeddingVectorSpace::ClipImage512,
        model_id: "clip-vit-base-patch32-vision-onnx".to_string(),
        dimension: DEFAULT_IMAGE_EMBEDDING_DIM,
        table_name: None,
    }
}

pub fn graph_node_contract() -> EmbeddingContract {
    EmbeddingContract {
        role: EmbeddingRole::GraphNode,
        vector_space: EmbeddingVectorSpace::GraphNodeText384,
        model_id: embedding_v4_contract().model_id.to_string(),
        dimension: DEFAULT_TEXT_EMBEDDING_DIM,
        table_name: Some("graph_nodes".to_string()),
    }
}

pub fn upsert_embedding_manifest(raw_evidence: &str, manifest: &EmbeddingManifest) -> String {
    let mut value = parse_raw_evidence_object(raw_evidence);
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            EMBEDDING_MANIFEST_KEY.to_string(),
            serde_json::to_value(manifest).unwrap_or_else(|_| serde_json::json!({})),
        );
    }
    value.to_string()
}

pub fn read_embedding_manifest(raw_evidence: &str) -> Option<EmbeddingManifest> {
    let value: serde_json::Value = serde_json::from_str(raw_evidence).ok()?;
    serde_json::from_value(value.get(EMBEDDING_MANIFEST_KEY)?.clone()).ok()
}

pub fn search_embedding_provenance(raw_evidence: &str) -> Option<SearchEmbeddingProvenance> {
    let manifest = read_embedding_manifest(raw_evidence)?;
    search_embedding_provenance_from_manifest(&manifest)
}

pub fn search_embedding_provenance_from_metadata(
    metadata: &serde_json::Value,
) -> Option<SearchEmbeddingProvenance> {
    let manifest = serde_json::from_value(metadata.get(EMBEDDING_MANIFEST_KEY)?.clone()).ok()?;
    search_embedding_provenance_from_manifest(&manifest)
}

fn search_embedding_provenance_from_manifest(
    manifest: &EmbeddingManifest,
) -> Option<SearchEmbeddingProvenance> {
    let mut roles = Vec::new();
    for role in manifest_roles(manifest) {
        let status = effective_role_status(manifest, role);
        let source = manifest
            .source_hashes
            .iter()
            .find(|hash| hash.role == role && hash.ordinal.is_none())
            .or_else(|| manifest.source_hashes.iter().find(|hash| hash.role == role));
        let contract = manifest
            .contracts
            .iter()
            .find(|contract| contract.role == role);
        let reason = manifest
            .statuses
            .iter()
            .rev()
            .find(|entry| entry.role == role && entry.reason.is_some())
            .and_then(|entry| entry.reason.clone());
        roles.push(SearchEmbeddingRoleProvenance {
            role,
            status,
            vector_space: contract.map(|contract| contract.vector_space),
            model_id: contract.map(|contract| contract.model_id.clone()),
            dimension: contract.map(|contract| contract.dimension),
            source_hash: source.map(|hash| hash.sha256.clone()),
            source_char_count: source.map(|hash| hash.char_count).unwrap_or_default(),
            reason,
        });
    }
    let status_labels = roles
        .iter()
        .filter(|role| role.status != EmbeddingStatus::Ready)
        .map(|role| {
            format!(
                "embedding:{}:{}",
                role_label(role.role),
                status_label(role.status)
            )
        })
        .collect();
    Some(SearchEmbeddingProvenance {
        document_version: manifest.document_version,
        roles,
        visual_semantic_source: manifest.visual_semantic_source,
        status_labels,
    })
}

pub fn embedding_retrieval_adjustment(
    provenance: Option<&SearchEmbeddingRoleProvenance>,
    role: EmbeddingRole,
) -> EmbeddingRetrievalAdjustment {
    let status = provenance
        .map(|provenance| provenance.status)
        .unwrap_or(EmbeddingStatus::Ready);
    let score_multiplier = match status {
        EmbeddingStatus::Ready => 1.0,
        EmbeddingStatus::LegacyUnverified => 0.97,
        EmbeddingStatus::StaleSourceText => 0.74,
        EmbeddingStatus::DimensionMismatch => 0.50,
        EmbeddingStatus::Unavailable => 0.45,
        EmbeddingStatus::ZeroVectorFallback => 0.45,
    };
    let reason_labels = if score_multiplier < 1.0 {
        vec![format!(
            "embedding:{}:{}",
            role_label(role),
            status_label(status)
        )]
    } else {
        Vec::new()
    };
    EmbeddingRetrievalAdjustment {
        role,
        status,
        score_multiplier,
        reason_labels,
    }
}

pub fn flag_embedding_text_mismatch(
    raw_evidence: &str,
    document: &MemoryEmbeddingDocument,
) -> String {
    let mut manifest = read_embedding_manifest(raw_evidence).unwrap_or_else(|| {
        build_embedding_manifest(
            document,
            EmbeddingStatus::LegacyUnverified,
            EmbeddingStatus::LegacyUnverified,
            VisualSemanticSource::Unknown,
        )
    });
    manifest.statuses.push(EmbeddingRoleStatus {
        role: EmbeddingRole::Primary,
        status: EmbeddingStatus::StaleSourceText,
        reason: Some("embedding_text differs from canonical document primary_text".to_string()),
    });
    upsert_embedding_manifest(raw_evidence, &manifest)
}

pub fn compose_graph_node_embedding_text(
    node: &GraphNode,
    source_memory: Option<&MemoryRecord>,
) -> String {
    let mut segments = Vec::new();
    segments.push(format!(
        "node_type: {}",
        graph_node_type_label(node.node_type)
    ));
    segments.push(format!("label: {}", node.label.trim()));

    if let Some(memory) = source_memory {
        push_segment(&mut segments, "project", &memory.project);
        push_segment(&mut segments, "topic", &memory.topic);
        push_segment(&mut segments, "activity", &memory.activity_type);
        push_segment(&mut segments, "context", &memory.memory_context);
        push_segment(
            &mut segments,
            "what_happened",
            &memory.insight_what_happened,
        );
        push_segment(&mut segments, "why_mattered", &memory.insight_why_mattered);
        push_joined(&mut segments, "entities", &memory.entities);
        push_joined(&mut segments, "files", &memory.files_touched);
        push_joined(&mut segments, "decisions", &memory.decisions);
        push_joined(&mut segments, "errors", &memory.errors);
        push_joined(&mut segments, "todos", &memory.todos);
    }

    trim_chars(&segments.join("\n"), GRAPH_NODE_TEXT_MAX_CHARS)
}

pub fn annotate_graph_node_embedding(
    node: &mut GraphNode,
    status: EmbeddingStatus,
    source_text: &str,
    reason: Option<String>,
) {
    let mut metadata = node.metadata.clone();
    if !metadata.is_object() {
        metadata = serde_json::json!({});
    }
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert(
            EMBEDDING_MANIFEST_KEY.to_string(),
            serde_json::json!({
                "version": 1,
                "document_version": EMBEDDING_DOCUMENT_VERSION,
                "source_hashes": [source_hash(EmbeddingRole::GraphNode, None, source_text)],
                "contracts": [graph_node_contract()],
                "statuses": [{
                    "role": EmbeddingRole::GraphNode,
                    "status": status,
                    "reason": reason,
                }],
                "visual_semantic_source": VisualSemanticSource::None,
            }),
        );
    }
    node.metadata = metadata;
}

pub fn infer_visual_semantic_source(record: &MemoryRecord) -> VisualSemanticSource {
    let source = record.source_type.to_ascii_lowercase();
    let branch = record.synthesis_branch.to_ascii_lowercase();
    let enrichment = record.enrichment_status.to_ascii_lowercase();
    let model_hint = record.raw_evidence.to_ascii_lowercase();
    if enrichment.contains("visual_semantics_failed")
        || model_hint.contains("visual_semantics_failed")
    {
        VisualSemanticSource::VisualSemanticsFailed
    } else if branch.contains("llm_ocr_grounded") || model_hint.contains("llm_ocr_grounded") {
        VisualSemanticSource::LlmOcrGrounded
    } else if branch.contains("visual_metadata_fallback")
        || enrichment.contains("visual_metadata_fallback")
        || model_hint.contains("clip_metadata_fallback")
    {
        VisualSemanticSource::ClipMetadataFallback
    } else if branch.contains("vlm") || model_hint.contains("pixel_vlm_or_ocr_grounded") {
        VisualSemanticSource::PixelVlmOrOcrGrounded
    } else if branch.contains("ocr_only") || model_hint.contains("ocr_only") {
        VisualSemanticSource::OcrOnly
    } else if source.contains("visual") || source.contains("image") || source.contains("glasses") {
        VisualSemanticSource::ClipImageEmbedding
    } else if source.contains("screen") || source.contains("browser") {
        VisualSemanticSource::TextCapture
    } else {
        VisualSemanticSource::None
    }
}

pub fn image_embedding_status(image_embedding: &[f32]) -> EmbeddingStatus {
    if image_embedding.len() != DEFAULT_IMAGE_EMBEDDING_DIM {
        EmbeddingStatus::DimensionMismatch
    } else if image_embedding.iter().all(|value| *value == 0.0) {
        EmbeddingStatus::ZeroVectorFallback
    } else {
        EmbeddingStatus::Ready
    }
}

pub fn text_embedding_status(embedding: &[f32]) -> EmbeddingStatus {
    if embedding.len() != DEFAULT_TEXT_EMBEDDING_DIM {
        EmbeddingStatus::DimensionMismatch
    } else if embedding.iter().all(|value| *value == 0.0) {
        EmbeddingStatus::Unavailable
    } else {
        EmbeddingStatus::Ready
    }
}

fn chunk_source_text(record: &MemoryRecord) -> String {
    for candidate in [
        record.clean_text.as_str(),
        record.memory_context.as_str(),
        record.embedding_text.as_str(),
        record.snippet.as_str(),
        record.text.as_str(),
    ] {
        let normalized = normalize_document_text(candidate);
        if !normalized.is_empty() {
            return trim_chars(&normalized, CHUNK_SOURCE_MAX_CHARS);
        }
    }
    String::new()
}

fn visual_semantic_text(record: &MemoryRecord) -> Option<String> {
    match infer_visual_semantic_source(record) {
        VisualSemanticSource::None | VisualSemanticSource::TextCapture => None,
        _ => {
            let text = normalize_document_text(&record.memory_context);
            if text.is_empty() {
                None
            } else {
                Some(trim_chars(&text, PRIMARY_TEXT_MAX_CHARS))
            }
        }
    }
}

fn parse_raw_evidence_object(raw_evidence: &str) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(raw_evidence) {
        Ok(value) if value.is_object() => value,
        _ => serde_json::json!({}),
    }
}

fn manifest_roles(manifest: &EmbeddingManifest) -> Vec<EmbeddingRole> {
    let mut roles = Vec::new();
    for hash in &manifest.source_hashes {
        push_unique_role(&mut roles, hash.role);
    }
    for contract in &manifest.contracts {
        push_unique_role(&mut roles, contract.role);
    }
    for status in &manifest.statuses {
        push_unique_role(&mut roles, status.role);
    }
    roles.sort_by_key(|role| role_sort_key(*role));
    roles
}

fn push_unique_role(roles: &mut Vec<EmbeddingRole>, role: EmbeddingRole) {
    if !roles.contains(&role) {
        roles.push(role);
    }
}

fn effective_role_status(manifest: &EmbeddingManifest, role: EmbeddingRole) -> EmbeddingStatus {
    let mut status = None;
    for entry in manifest.statuses.iter().filter(|entry| entry.role == role) {
        if status
            .map(|current| status_severity(entry.status) > status_severity(current))
            .unwrap_or(true)
        {
            status = Some(entry.status);
        }
    }
    status.unwrap_or(EmbeddingStatus::Ready)
}

fn role_sort_key(role: EmbeddingRole) -> u8 {
    match role {
        EmbeddingRole::Primary => 0,
        EmbeddingRole::Snippet => 1,
        EmbeddingRole::Support => 2,
        EmbeddingRole::Chunk => 3,
        EmbeddingRole::Image => 4,
        EmbeddingRole::GraphNode => 5,
    }
}

fn status_severity(status: EmbeddingStatus) -> u8 {
    match status {
        EmbeddingStatus::Ready => 0,
        EmbeddingStatus::LegacyUnverified => 1,
        EmbeddingStatus::ZeroVectorFallback => 2,
        EmbeddingStatus::Unavailable => 3,
        EmbeddingStatus::StaleSourceText => 4,
        EmbeddingStatus::DimensionMismatch => 5,
    }
}

fn role_label(role: EmbeddingRole) -> &'static str {
    match role {
        EmbeddingRole::Primary => "primary",
        EmbeddingRole::Snippet => "snippet",
        EmbeddingRole::Support => "support",
        EmbeddingRole::Chunk => "chunk",
        EmbeddingRole::Image => "image",
        EmbeddingRole::GraphNode => "graph_node",
    }
}

fn status_label(status: EmbeddingStatus) -> &'static str {
    match status {
        EmbeddingStatus::Ready => "ready",
        EmbeddingStatus::ZeroVectorFallback => "zero_vector_fallback",
        EmbeddingStatus::Unavailable => "unavailable",
        EmbeddingStatus::StaleSourceText => "stale_source_text",
        EmbeddingStatus::DimensionMismatch => "dimension_mismatch",
        EmbeddingStatus::LegacyUnverified => "legacy_unverified",
    }
}

fn push_segment(out: &mut Vec<String>, label: &str, value: &str) {
    let trimmed = normalize_document_text(value);
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
        return;
    }
    out.push(format!("{label}: {trimmed}"));
}

fn push_joined(out: &mut Vec<String>, label: &str, values: &[String]) {
    let joined = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join(", ");
    push_segment(out, label, &joined);
}

fn graph_node_type_label(node_type: GraphNodeType) -> &'static str {
    node_type.to_str()
}

fn normalize_document_text(raw: &str) -> String {
    raw.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn trim_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn rich_record() -> MemoryRecord {
        let mut record = MemoryRecord::default();
        record.id = "memory-1".to_string();
        record.app_name = "Editor".to_string();
        record.window_title = "Search ranking".to_string();
        record.memory_context =
            "The search ranking work connected vector recall with graph evidence.".to_string();
        record.user_intent = "Improve retrieval quality".to_string();
        record.project = "Memory search".to_string();
        record.topic = "Graph retrieval".to_string();
        record.activity_type = "implementation".to_string();
        record.entities = vec!["LanceDB".to_string(), "graph route".to_string()];
        record.files_touched = vec!["src/search/retrieval.rs".to_string()];
        record.decisions = vec!["Keep BGE as explicit reindex".to_string()];
        record.errors = vec!["weak OCR keyword hit".to_string()];
        record.todos = vec!["add eval fixture".to_string()];
        record.search_aliases = vec!["agentic graph search".to_string()];
        record.clean_text = "RAW_OCR_BLOB_THAT_SHOULD_NOT_BE_PRIMARY".to_string();
        record
    }

    #[test]
    fn primary_document_prefers_structured_context_over_raw_ocr() {
        let doc = compose_memory_embedding_document(&rich_record(), None);
        assert!(doc.primary_text.contains("context:"));
        assert!(doc.primary_text.contains("Graph retrieval"));
        assert!(!doc.primary_text.contains("RAW_OCR_BLOB"));
        assert!(doc.chunk_source_text.contains("RAW_OCR_BLOB"));
    }

    #[test]
    fn manifest_round_trips_inside_raw_evidence_without_dropping_existing_fields() {
        let record = rich_record();
        let doc = compose_memory_embedding_document(&record, None);
        let manifest = build_embedding_manifest(
            &doc,
            EmbeddingStatus::Ready,
            EmbeddingStatus::ZeroVectorFallback,
            VisualSemanticSource::TextCapture,
        );
        let raw = upsert_embedding_manifest(r#"{"source_kind":"screen"}"#, &manifest);
        assert!(raw.contains("source_kind"));
        let parsed = read_embedding_manifest(&raw).expect("manifest");
        assert_eq!(parsed.document_version, EMBEDDING_DOCUMENT_VERSION);
        assert!(parsed
            .source_hashes
            .iter()
            .any(|hash| hash.role == EmbeddingRole::Primary));
    }

    #[test]
    fn graph_node_text_is_structured_and_not_raw_ocr() {
        let record = rich_record();
        let node = GraphNode {
            id: Uuid::new_v4(),
            node_type: GraphNodeType::Decision,
            label: "Keep BGE explicit".to_string(),
            confidence: 0.8,
            source_memory_ids: vec![record.id.clone()],
            embedding: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            stale: false,
            metadata: serde_json::json!({}),
        };
        let text = compose_graph_node_embedding_text(&node, Some(&record));
        assert!(text.contains("node_type: Decision"));
        assert!(text.contains("Keep BGE explicit"));
        assert!(text.contains("Memory search"));
        assert!(!text.contains("RAW_OCR_BLOB"));
    }

    #[test]
    fn vector_contracts_keep_text_bge_graph_and_image_spaces_separate() {
        assert_eq!(minilm_contract(EmbeddingRole::Primary).dimension, 384);
        assert_eq!(bge_v5_contract_for(EmbeddingRole::Chunk).dimension, 1024);
        assert_eq!(graph_node_contract().dimension, 384);
        assert_eq!(clip_image_contract().dimension, 512);
        assert_ne!(
            bge_v5_contract_for(EmbeddingRole::Chunk).vector_space,
            minilm_contract(EmbeddingRole::Primary).vector_space
        );
    }

    #[test]
    fn search_provenance_reports_stale_primary_and_score_adjustment() {
        let record = rich_record();
        let doc = compose_memory_embedding_document(&record, None);
        let raw = flag_embedding_text_mismatch(r#"{"source_kind":"legacy"}"#, &doc);

        let provenance = search_embedding_provenance(&raw).expect("search provenance");
        let primary = provenance
            .role(EmbeddingRole::Primary)
            .expect("primary role provenance");
        assert_eq!(primary.status, EmbeddingStatus::StaleSourceText);

        let adjustment = embedding_retrieval_adjustment(
            provenance.role(EmbeddingRole::Primary),
            EmbeddingRole::Primary,
        );
        assert!(adjustment.score_multiplier < 0.8);
        assert!(adjustment
            .reason_labels
            .iter()
            .any(|label| label.contains("stale_source_text")));
    }
}
