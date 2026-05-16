#[cfg(test)]
mod agent_regression {
    use fndr_lib::agent::{
        AgentContextRequest, AgentMode, build_agent_context_pack,
    };
    use fndr_lib::AppState;
    use std::sync::Arc;
    use tokio::runtime::Runtime;

    fn setup_test_state() -> Arc<AppState> {
        // Create AppState with tempfile dir
        // (Actual setup depends on AppState constructor)
        todo!("Implement test AppState factory")
    }

    #[test]
    fn agent_context_pack_is_deterministic() {
        let rt = Runtime::new().unwrap();
        let state = setup_test_state();
        let req = AgentContextRequest {
            user_goal: "test".to_string(),
            mode: AgentMode::Ask,
            ..Default::default()
        };

        let pack1 = rt.block_on(build_agent_context_pack(&state, req.clone())).unwrap();
        let pack2 = rt.block_on(build_agent_context_pack(&state, req)).unwrap();

        assert_eq!(pack1.task_id, pack2.task_id, "Context pack must be deterministic");
    }

    #[test]
    fn ask_mode_has_no_proposed_actions() {
        // Verify Ask mode doesn't generate AgentAction proposals
        todo!()
    }

    #[test]
    fn plan_mode_proposes_but_does_not_execute() {
        // Verify Plan mode returns proposed_actions but status is not Executed
        todo!()
    }

    #[test]
    fn audit_record_is_created_for_every_run() {
        // Verify every agent.run call appends to audit JSONL
        todo!()
    }

    #[test]
    fn policy_defaults_deny_dangerous_actions() {
        // Verify Act mode policy blocks rm, sudo, git push, etc.
        todo!()
    }
}
