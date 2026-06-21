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

/// Width of the shared cluster's embedding column (`VECTOR(1536)`).
pub const CLOUD_EMBED_DIM: usize = 1536;

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
}
