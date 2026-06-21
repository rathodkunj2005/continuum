//! Cloud embedding adapter — reuse Continuum's on-device embedding instead of a
//! server-side OpenAI call.
//!
//! The shared cluster stores `semantic_nodes.embedding` as `VECTOR(1536)` (the
//! OpenAI `text-embedding-3-small` width). Continuum's local text embedding is
//! 384-d (MiniLM). We project the existing local vector into the cluster's
//! 1536-d space by zero-padding, which **preserves cosine similarity exactly**
//! (appended zeros add nothing to the dot product or the norm). So a cluster
//! whose clients all embed locally and pad the same way searches identically to
//! the native 384-d space — no OpenAI, no model key, and the vector still fits
//! the existing column.
//!
//! Caveat made explicit for the team: a padded local vector is **not**
//! comparable to an OpenAI-embedded one. For cross-person search the query path
//! (`query-synthesize`) must embed queries with the same local model + padding;
//! until then this is coherent only across desktops using this adapter.

use crate::cloud::descriptor::Descriptor;
use crate::embedding::Embedder;

/// Width of the shared cluster's embedding column (`VECTOR(1536)`).
pub const CLOUD_EMBED_DIM: usize = 1536;

/// The text embedded for a cloud node. Mirrors the server's `embedText`
/// (`supabase/functions/_shared/ingest.ts`): `app | topic | concept[ | error]`.
pub fn descriptor_embed_text(d: &Descriptor) -> String {
    let mut s = format!("{} | {} | {}", d.app, d.topic, d.concept);
    if let Some(err) = d.error_type.as_deref() {
        if !err.is_empty() {
            s.push_str(" | ");
            s.push_str(err);
        }
    }
    s
}

/// Embed a descriptor with a BGE-large (1024-d) embedder and project into the
/// cluster's 1536-d space. Returns `None` on any failure so the caller falls
/// back to the cheaper embedding already on the queued job.
pub fn embed_descriptor_bge(embedder: &Embedder, descriptor: &Descriptor) -> Option<Vec<f32>> {
    let text = descriptor_embed_text(descriptor);
    match embedder.embed_batch(&[text]) {
        Ok(mut vectors) if !vectors.is_empty() => {
            let raw = std::mem::take(&mut vectors[0]);
            if raw.is_empty() {
                None
            } else {
                Some(to_cloud_embedding(&raw))
            }
        }
        Ok(_) => None,
        Err(e) => {
            tracing::debug!(target: "continuum::cloud_sync", "BGE cloud embed failed: {e}");
            None
        }
    }
}

/// Project a local embedding into the cluster's [`CLOUD_EMBED_DIM`] space.
///
/// - shorter (e.g. 384-d MiniLM) → zero-padded to 1536 (cosine-preserving);
/// - exact width → returned unchanged;
/// - longer → truncated (does not preserve cosine; only hit if the local model
///   is wider than the cluster, which is not the case today);
/// - empty → empty (the caller omits the field so the server can fall back).
pub fn to_cloud_embedding(local: &[f32]) -> Vec<f32> {
    if local.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0.0_f32; CLOUD_EMBED_DIM];
    let n = local.len().min(CLOUD_EMBED_DIM);
    out[..n].copy_from_slice(&local[..n]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb)
    }

    #[test]
    fn pads_short_vector_to_cloud_dim() {
        let v = vec![0.5_f32, -0.5, 0.25];
        let out = to_cloud_embedding(&v);
        assert_eq!(out.len(), CLOUD_EMBED_DIM);
        assert_eq!(&out[..3], &[0.5, -0.5, 0.25]);
        assert!(out[3..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn padding_preserves_cosine_similarity() {
        let a = vec![0.2_f32, 0.4, -0.1, 0.9];
        let b = vec![0.1_f32, 0.5, -0.2, 0.7];
        let pa = to_cloud_embedding(&a);
        let pb = to_cloud_embedding(&b);
        let before = cosine(&a, &b);
        let after = cosine(&pa, &pb);
        assert!((before - after).abs() < 1e-6, "before={before} after={after}");
    }

    #[test]
    fn empty_stays_empty() {
        assert!(to_cloud_embedding(&[]).is_empty());
    }

    #[test]
    fn exact_width_unchanged() {
        let v = vec![0.1_f32; CLOUD_EMBED_DIM];
        assert_eq!(to_cloud_embedding(&v), v);
    }

    #[test]
    fn descriptor_text_matches_server_format() {
        let d = Descriptor {
            app: "VS Code".to_string(),
            topic: "rust".to_string(),
            concept: "editing".to_string(),
            error_type: None,
        };
        assert_eq!(descriptor_embed_text(&d), "VS Code | rust | editing");
        let d2 = Descriptor {
            error_type: Some("E0599".to_string()),
            ..d
        };
        assert_eq!(descriptor_embed_text(&d2), "VS Code | rust | editing | E0599");
    }
}
