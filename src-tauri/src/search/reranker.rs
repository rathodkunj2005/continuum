use crate::storage::SearchResult;

use super::query_processor::{normalize_text, QueryContext};

const VECTOR_WEIGHT: f32 = 0.7;
const COVERAGE_WEIGHT: f32 = 0.3;
const HARD_COVERAGE_THRESHOLD: f32 = 0.15;

#[derive(Debug, Clone, Default)]
pub struct RerankStats {
    pub excluded_for_coverage: usize,
}

pub fn rerank_results(
    query_context: &QueryContext,
    results: Vec<SearchResult>,
) -> (Vec<SearchResult>, RerankStats) {
    if results.is_empty() {
        return (Vec::new(), RerankStats::default());
    }

    let mut stats = RerankStats::default();
    let mut reranked = Vec::with_capacity(results.len());

    for mut result in results {
        let coverage = anchor_coverage_score(query_context, &result);
        result.anchor_coverage_score = coverage;

        if !query_context.anchor_terms.is_empty() && coverage < HARD_COVERAGE_THRESHOLD {
            stats.excluded_for_coverage += 1;
            continue;
        }

        let vector_similarity = result.score.clamp(0.0, 1.0);
        result.score =
            (vector_similarity * VECTOR_WEIGHT + coverage * COVERAGE_WEIGHT).clamp(0.0, 1.0);
        reranked.push(result);
    }

    reranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    (reranked, stats)
}

pub fn anchor_coverage_score(query_context: &QueryContext, result: &SearchResult) -> f32 {
    if query_context.anchor_terms.is_empty() {
        return 1.0;
    }

    let summary = if result.display_summary.trim().is_empty() {
        &result.snippet
    } else {
        &result.display_summary
    };
    let merged_text = normalize_text(&format!(
        "{} {} {} {} {} {}",
        result.window_title,
        summary,
        result.snippet,
        result.clean_text,
        result.extracted_entities.join(" "),
        result.url.clone().unwrap_or_default(),
    ));

    if merged_text.is_empty() {
        return 0.0;
    }

    let mut matched = 0usize;
    for term in &query_context.anchor_terms {
        let term_norm = normalize_text(term);
        if term_norm.is_empty() {
            continue;
        }
        if merged_text.contains(&term_norm) {
            matched += 1;
        }
    }

    let mut score = matched as f32 / query_context.anchor_terms.len() as f32;
    if !query_context.normalized_query.is_empty()
        && merged_text.contains(&query_context.normalized_query)
    {
        score = (score + 0.12).min(1.0);
    }
    score.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(title: &str, summary: &str) -> SearchResult {
        SearchResult {
            id: "1".to_string(),
            window_title: title.to_string(),
            snippet: summary.to_string(),
            display_summary: summary.to_string(),
            clean_text: summary.to_string(),
            extracted_entities: Vec::new(),
            score: 0.8,
            ..Default::default()
        }
    }

    #[test]
    fn excludes_low_anchor_coverage_results() {
        let query = QueryContext::from_query("cricket");
        let (results, stats) = rerank_results(
            &query,
            vec![
                result("IPL Highlights", "Watched cricket highlights"),
                result("Rust Docs", "Debugged Rust compiler issues"),
            ],
        );

        assert_eq!(stats.excluded_for_coverage, 1);
        assert_eq!(results.len(), 1);
    }
}

