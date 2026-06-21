//! Cluster Q&A — desktop client for the `query-synthesize` Edge Function.
//!
//! The desktop asks the same endpoint the web app uses, authenticated with the
//! signed-in user's JWT (the function runs `verify_jwt = true`). The Edge
//! Function embeds the query, retrieves + reranks cluster nodes, expands the
//! subgraph, and returns a Claude-synthesized, citation-aware answer.
//!
//! The server response is `{ answer, subgraph: { nodes, edges } }`; we flatten
//! the subgraph nodes into [`Citation`]s for the ambient panel.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cloud::{CloudConfig, CloudSession};

/// A teammate node that informed the answer (rendered under the answer text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    /// Teammate display name (falls back to a short id server-side).
    pub user: String,
    pub concept: String,
    pub app: String,
    pub topic: String,
    /// ISO-8601 capture time, if present.
    pub timestamp: Option<String>,
    /// Source node id.
    pub node_id: String,
}

/// A grounded answer plus the nodes it cited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusterAnswer {
    pub answer: String,
    pub citations: Vec<Citation>,
    pub node_ids: Vec<String>,
}

// ── Wire shape of the Edge Function response ───────────────────────────────

#[derive(Debug, Deserialize)]
struct QuerySynthResponse {
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    subgraph: Subgraph,
    /// Present on a handled error (e.g. bad request) instead of an answer.
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Subgraph {
    #[serde(default)]
    nodes: Vec<SubgraphNode>,
}

#[derive(Debug, Deserialize)]
struct SubgraphNode {
    #[serde(default)]
    id: String,
    #[serde(default)]
    teammate: String,
    #[serde(default)]
    concept: String,
    #[serde(default)]
    app: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    created_at: Option<String>,
}

/// Map the Edge Function payload into a [`ClusterAnswer`]. Pure (no IO) so the
/// mapping is unit-tested against captured fixtures.
fn parse_answer(resp: QuerySynthResponse) -> Result<ClusterAnswer, String> {
    if let Some(err) = resp.error {
        return Err(err);
    }
    let answer = resp
        .answer
        .unwrap_or_else(|| "No answer was returned.".to_string());
    let node_ids: Vec<String> = resp.subgraph.nodes.iter().map(|n| n.id.clone()).collect();
    let citations = resp
        .subgraph
        .nodes
        .into_iter()
        .map(|n| Citation {
            user: n.teammate,
            concept: n.concept,
            app: n.app,
            topic: n.topic,
            timestamp: n.created_at,
            node_id: n.id,
        })
        .collect();
    Ok(ClusterAnswer {
        answer,
        citations,
        node_ids,
    })
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Continuum/1.0")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))
}

/// Ask the cluster a question. `cluster_id` must be the user's joined cluster.
pub async fn query_cluster(
    cfg: &CloudConfig,
    session: &CloudSession,
    cluster_id: &str,
    query: &str,
) -> Result<ClusterAnswer, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Ask a question first.".to_string());
    }
    let client = http_client()?;
    let res = client
        .post(cfg.function_url("query-synthesize"))
        .header("apikey", &cfg.anon_key)
        .bearer_auth(&session.access_token)
        .json(&serde_json::json!({ "query": query, "cluster_id": cluster_id }))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach the cluster: {e}"))?;

    let status = res.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("Rate limit reached — try again in a moment.".to_string());
    }
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Query failed (HTTP {status}): {text}"));
    }
    let body: QuerySynthResponse = res
        .json()
        .await
        .map_err(|e| format!("Unexpected query response: {e}"))?;
    parse_answer(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_response_shape() {
        let json = serde_json::json!({
            "answer": "According to [Luke@14:32] the parser was rewritten.",
            "subgraph": {
                "nodes": [
                    {
                        "id": "n1",
                        "label": "parser rewrite",
                        "concept": "parser rewrite",
                        "app": "VS Code",
                        "topic": "compiler",
                        "teammate": "Luke",
                        "created_at": "2026-06-20T14:32:00Z"
                    }
                ],
                "edges": [
                    { "source": "n1", "target": "n2", "type": "BUILDS_ON" }
                ]
            }
        });
        let resp: QuerySynthResponse = serde_json::from_value(json).unwrap();
        let answer = parse_answer(resp).unwrap();
        assert!(answer.answer.contains("parser was rewritten"));
        assert_eq!(answer.node_ids, vec!["n1".to_string()]);
        assert_eq!(answer.citations.len(), 1);
        let c = &answer.citations[0];
        assert_eq!(c.user, "Luke");
        assert_eq!(c.concept, "parser rewrite");
        assert_eq!(c.app, "VS Code");
        assert_eq!(c.timestamp.as_deref(), Some("2026-06-20T14:32:00Z"));
        assert_eq!(c.node_id, "n1");
    }

    #[test]
    fn handles_empty_subgraph() {
        let json = serde_json::json!({
            "answer": "Nothing relevant has been captured for this question yet.",
            "subgraph": { "nodes": [], "edges": [] }
        });
        let resp: QuerySynthResponse = serde_json::from_value(json).unwrap();
        let answer = parse_answer(resp).unwrap();
        assert!(answer.citations.is_empty());
        assert!(answer.node_ids.is_empty());
        assert!(answer.answer.contains("Nothing relevant"));
    }

    #[test]
    fn surfaces_server_error() {
        let json = serde_json::json!({ "error": "missing query or cluster_id" });
        let resp: QuerySynthResponse = serde_json::from_value(json).unwrap();
        assert_eq!(
            parse_answer(resp).unwrap_err(),
            "missing query or cluster_id"
        );
    }
}
