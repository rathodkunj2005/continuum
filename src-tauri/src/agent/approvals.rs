use crate::agent::actions::AgentActionStatus;
use crate::agent::audit::get_agent_action_by_id;

/// Returns true if the action has been approved (or already ran/succeeded).
pub fn is_action_approved(app_data_dir: &std::path::Path, action_id: &str) -> Result<bool, String> {
    match get_agent_action_by_id(app_data_dir, action_id)? {
        Some(action) => Ok(matches!(
            action.status,
            AgentActionStatus::Approved | AgentActionStatus::Running | AgentActionStatus::Succeeded
        )),
        None => Err(format!("Action not found: {}", action_id)),
    }
}
