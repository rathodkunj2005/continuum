//! Bounded [`reqwest::Client`] builders for localhost probes and LLM-style HTTP.
//! Core capture/search stay offline; this backs agent and provider status from `api::commands`.

use std::time::Duration;

const USER_AGENT: &str = "FNDR/1.0";

/// Health and tag-list probes to local services (Ollama, Hermes gateway).
pub fn local_service_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(6))
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
}

/// Chat/completions against Ollama or similar — long generation, bounded stall.
pub fn llm_http_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()
}

/// POST JSON, read JSON body; used by agent chat paths to keep error handling in one place.
pub async fn post_json_response(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
    bearer_token: Option<&str>,
) -> Result<(reqwest::StatusCode, serde_json::Value), String> {
    let mut req = client.post(url).json(body);
    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    let status = response.status();
    let json = response
        .json()
        .await
        .map_err(|e| format!("Unreadable JSON response: {e}"))?;
    Ok((status, json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients_build() {
        assert!(local_service_client().is_ok());
        assert!(llm_http_client().is_ok());
    }
}
