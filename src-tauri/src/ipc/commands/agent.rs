use crate::agent::audit::{
    append_feedback, explanation_from_audit, get_agent_audit_run as load_agent_audit_run,
    list_agent_audit_runs as load_agent_audit_runs,
    AgentAuditRecord, AgentRunStatus, ExplainRetrievalRequest, RateResultRequest,
    RetrievalExplanation,
};
use crate::agent::evals::{append_eval_draft, list_eval_drafts, propose_eval_from_audit};
use crate::agent::skills::{append_skill_draft, list_skill_drafts, propose_skill_from_audit};
use crate::agent::{
    self, AgentContextPack, AgentContextRequest, AgentEvalCase, AgentPrompt, AgentRunResponse,
    AgentSkillCandidate, AgentMode,
};
use crate::AppState;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn build_agent_context_pack(
    state: State<'_, Arc<AppState>>,
    request: AgentContextRequest,
) -> Result<AgentContextPack, String> {
    agent::build_agent_context_pack(state.inner(), request).await
}

#[tauri::command]
pub async fn run_agent_request(
    state: State<'_, Arc<AppState>>,
    request: AgentContextRequest,
) -> Result<AgentRunResponse, String> {
    agent::context::run_agent_request(state.inner(), request).await
}

#[tauri::command]
pub async fn list_agent_audit_runs(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
    mode: Option<AgentMode>,
    status: Option<AgentRunStatus>,
) -> Result<Vec<AgentAuditRecord>, String> {
    load_agent_audit_runs(
        state.inner().app_data_dir.as_path(),
        limit.unwrap_or(20),
        mode,
        status,
    )
}

#[tauri::command]
pub async fn get_agent_audit_run(
    state: State<'_, Arc<AppState>>,
    run_id: String,
) -> Result<Option<AgentAuditRecord>, String> {
    load_agent_audit_run(state.inner().app_data_dir.as_path(), &run_id)
}

#[tauri::command]
pub async fn explain_agent_retrieval(
    state: State<'_, Arc<AppState>>,
    request: ExplainRetrievalRequest,
) -> Result<RetrievalExplanation, String> {
    if let Some(run_id) = request.run_id.as_deref() {
        let record = load_agent_audit_run(state.inner().app_data_dir.as_path(), run_id)?
            .ok_or_else(|| format!("No agent audit run found for {run_id}"))?;
        return Ok(explanation_from_audit(&record));
    }

    let query = request
        .query
        .clone()
        .or_else(|| request.context_pack_id.clone())
        .unwrap_or_else(|| "recent agent context".to_string());
    let response = agent::context::run_agent_request(
        state.inner(),
        AgentContextRequest {
            user_goal: query,
            mode: AgentMode::Ask,
            project: request.project,
            budget_tokens: 900,
            ..Default::default()
        },
    )
    .await?;
    let record = load_agent_audit_run(state.inner().app_data_dir.as_path(), &response.run_id)?
        .ok_or_else(|| "Agent run was created but audit detail was unavailable".to_string())?;
    Ok(explanation_from_audit(&record))
}

#[tauri::command]
pub async fn rate_agent_result(
    state: State<'_, Arc<AppState>>,
    request: RateResultRequest,
) -> Result<crate::agent::audit::AgentRetrievalFeedback, String> {
    append_feedback(state.inner().app_data_dir.as_path(), request)
}

#[tauri::command]
pub async fn propose_skill_from_run(
    state: State<'_, Arc<AppState>>,
    run_id: String,
) -> Result<AgentSkillCandidate, String> {
    let record = load_agent_audit_run(state.inner().app_data_dir.as_path(), &run_id)?
        .ok_or_else(|| format!("No agent audit run found for {run_id}"))?;
    let draft = propose_skill_from_audit(&record)?;
    append_skill_draft(state.inner().app_data_dir.as_path(), &draft)?;
    Ok(draft)
}

#[tauri::command]
pub async fn list_agent_skill_drafts(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
) -> Result<Vec<AgentSkillCandidate>, String> {
    list_skill_drafts(state.inner().app_data_dir.as_path(), limit.unwrap_or(20))
}

#[tauri::command]
pub async fn propose_eval_from_run(
    state: State<'_, Arc<AppState>>,
    run_id: String,
) -> Result<AgentEvalCase, String> {
    let record = load_agent_audit_run(state.inner().app_data_dir.as_path(), &run_id)?
        .ok_or_else(|| format!("No agent audit run found for {run_id}"))?;
    let draft = propose_eval_from_audit(&record)?;
    append_eval_draft(state.inner().app_data_dir.as_path(), &draft)?;
    Ok(draft)
}

#[tauri::command]
pub async fn list_agent_eval_drafts(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
) -> Result<Vec<AgentEvalCase>, String> {
    list_eval_drafts(state.inner().app_data_dir.as_path(), limit.unwrap_or(20))
}

#[tauri::command]
pub async fn list_agent_prompts() -> Result<Vec<AgentPrompt>, String> {
    Ok(agent::list_agent_prompts())
}

#[tauri::command]
pub async fn get_agent_prompt(name: String) -> Result<Option<AgentPrompt>, String> {
    Ok(agent::get_agent_prompt(&name))
}
