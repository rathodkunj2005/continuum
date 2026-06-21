//! Descriptor derivation: turn a locally-synthesized [`MemoryRecord`] into the
//! compact `{ app, topic, concept, error_type }` shape the `agent-sync` Edge
//! Function ingests.
//!
//! The reference Electron agent runs a vision model per frame to produce this
//! descriptor. Continuum already synthesizes a richer record on-device (OCR +
//! VLM/LLM into `display_summary`, `topic`, `memory_context`, …), so we derive
//! the descriptor from that existing output instead of running a second model.
//!
//! The `content_hash` mirrors the server's `descriptorFingerprint` (lower-cased,
//! whitespace-collapsed `app|topic|concept|error_type`) so the desktop's L1
//! dedup and the Edge Function's pre-embedding dedup agree on "same observation".

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::MemoryRecord;

/// Sentinel the capture pipeline writes when a field is unknown.
const UNKNOWN: &str = "unknown";

/// The graph-node payload sent to `agent-sync`. Field names match the Edge
/// Function's `Descriptor` interface exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    pub app: String,
    pub topic: String,
    pub concept: String,
    /// `null` on the wire when there is no error context.
    #[serde(default)]
    pub error_type: Option<String>,
}

impl Descriptor {
    /// Stable fingerprint for "the same observation": case- and whitespace-
    /// insensitive over the semantic fields. Matches `descriptorFingerprint`
    /// in `supabase/functions/_shared/ingest.ts`.
    pub fn fingerprint(&self) -> String {
        [
            self.app.as_str(),
            self.topic.as_str(),
            self.concept.as_str(),
            self.error_type.as_deref().unwrap_or(""),
        ]
        .iter()
        .map(|s| normalize(s))
        .collect::<Vec<_>>()
        .join("|")
    }

    /// SHA-256 of the fingerprint — the dedup key used across the pipeline.
    pub fn content_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.fingerprint().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Derive a descriptor from a synthesized memory record. Returns `None`
    /// when the record carries no shareable semantic content (e.g. a metadata-
    /// only or visual-failed row) so we never push empty nodes to the graph.
    pub fn from_memory_record(record: &MemoryRecord) -> Option<Self> {
        let app = clean(&record.app_name).unwrap_or_else(|| "Unknown App".to_string());

        // Topic: prefer the synthesized `topic`, fall back to the window title.
        let topic = meaningful(&record.topic)
            .or_else(|| clean(&record.window_title))
            .unwrap_or_else(|| "General".to_string());

        // Concept: the human-readable one-liner. Prefer the reviewed summary,
        // then the snippet, then a trimmed slice of the internal context.
        let concept = meaningful(&record.display_summary)
            .or_else(|| meaningful(&record.snippet))
            .or_else(|| meaningful(&record.memory_context).map(|c| truncate(&c, 240)))?;

        // Error context, if the synthesis surfaced a blocker.
        let error_type = record
            .blockers
            .iter()
            .find_map(|b| clean(b))
            .map(|b| truncate(&b, 160));

        Some(Self {
            app,
            topic,
            concept,
            error_type,
        })
    }
}

/// Lower-case, trim, and collapse internal whitespace to single spaces.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Trimmed value, or `None` if empty after trimming.
fn clean(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Like [`clean`] but also rejects the `"unknown"` sentinel the pipeline uses
/// for unresolved enum/topic fields.
fn meaningful(s: &str) -> Option<String> {
    clean(s).filter(|t| !t.eq_ignore_ascii_case(UNKNOWN))
}

/// Truncate on a char boundary, appending an ellipsis when shortened.
fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_record() -> MemoryRecord {
        let mut r = MemoryRecord {
            id: "m1".to_string(),
            app_name: "Visual Studio Code".to_string(),
            window_title: "main.rs — continuum".to_string(),
            ..Default::default()
        };
        r.topic = "rust capture pipeline".to_string();
        r.display_summary = "Editing the capture flush loop to add cloud sync".to_string();
        r
    }

    #[test]
    fn derives_full_descriptor() {
        let d = Descriptor::from_memory_record(&base_record()).expect("descriptor");
        assert_eq!(d.app, "Visual Studio Code");
        assert_eq!(d.topic, "rust capture pipeline");
        assert_eq!(d.concept, "Editing the capture flush loop to add cloud sync");
        assert_eq!(d.error_type, None);
    }

    #[test]
    fn falls_back_topic_to_window_title() {
        let mut r = base_record();
        r.topic = "unknown".to_string();
        let d = Descriptor::from_memory_record(&r).expect("descriptor");
        assert_eq!(d.topic, "main.rs — continuum");
    }

    #[test]
    fn concept_falls_back_to_snippet_then_context() {
        let mut r = base_record();
        r.display_summary = "  ".to_string();
        r.snippet = "Reviewing PR feedback".to_string();
        let d = Descriptor::from_memory_record(&r).expect("descriptor");
        assert_eq!(d.concept, "Reviewing PR feedback");
    }

    #[test]
    fn returns_none_without_semantic_content() {
        let mut r = base_record();
        r.display_summary = "".to_string();
        r.snippet = "".to_string();
        r.memory_context = "".to_string();
        assert!(Descriptor::from_memory_record(&r).is_none());
    }

    #[test]
    fn surfaces_first_blocker_as_error_type() {
        let mut r = base_record();
        r.blockers = vec!["".to_string(), "cargo build fails: E0599".to_string()];
        let d = Descriptor::from_memory_record(&r).expect("descriptor");
        assert_eq!(d.error_type.as_deref(), Some("cargo build fails: E0599"));
    }

    #[test]
    fn fingerprint_is_case_and_whitespace_insensitive() {
        let a = Descriptor {
            app: "VS Code".to_string(),
            topic: "Rust  Pipeline".to_string(),
            concept: "Editing".to_string(),
            error_type: None,
        };
        let b = Descriptor {
            app: "vs code".to_string(),
            topic: "rust pipeline".to_string(),
            concept: "editing".to_string(),
            error_type: Some("".to_string()),
        };
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn content_hash_changes_with_content() {
        let a = Descriptor {
            app: "VS Code".to_string(),
            topic: "Rust".to_string(),
            concept: "Editing".to_string(),
            error_type: None,
        };
        let mut b = a.clone();
        b.concept = "Debugging".to_string();
        assert_ne!(a.content_hash(), b.content_hash());
    }
}
