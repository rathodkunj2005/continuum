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

#[cfg(test)]
mod audit_persistence {
    use fndr_lib::agent::audit::{
        append_agent_audit_record, list_agent_audit_runs, AgentAuditRecord,
        AgentRunStatus,
    };
    use fndr_lib::agent::policy::AgentMode;

    fn make_record(run_id: &str, mode: AgentMode, status: AgentRunStatus) -> AgentAuditRecord {
        AgentAuditRecord {
            run_id: run_id.to_string(),
            user_goal: format!("goal for {run_id}"),
            mode,
            result_status: status,
            ..Default::default()
        }
    }

    #[test]
    fn audit_record_persistence() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let app_data_dir = dir.path();

        let record = make_record("run-persist-1", AgentMode::Ask, AgentRunStatus::Success);
        append_agent_audit_record(app_data_dir, &record).expect("append audit record");

        let rows = list_agent_audit_runs(app_data_dir, 10, None, None)
            .expect("list audit runs");

        assert_eq!(rows.len(), 1, "Expected exactly one audit record");
        assert_eq!(rows[0].run_id, "run-persist-1", "run_id must round-trip");
    }

    #[test]
    fn audit_filtering_by_mode() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let app_data_dir = dir.path();

        append_agent_audit_record(app_data_dir, &make_record("run-ask-1", AgentMode::Ask, AgentRunStatus::Success))
            .expect("append ask record 1");
        append_agent_audit_record(app_data_dir, &make_record("run-ask-2", AgentMode::Ask, AgentRunStatus::Partial))
            .expect("append ask record 2");
        append_agent_audit_record(app_data_dir, &make_record("run-plan-1", AgentMode::Plan, AgentRunStatus::Success))
            .expect("append plan record");

        let ask_rows = list_agent_audit_runs(app_data_dir, 100, Some(AgentMode::Ask), None)
            .expect("list ask audit runs");

        assert_eq!(ask_rows.len(), 2, "Filtering by Ask mode should return exactly 2 records");
        assert!(
            ask_rows.iter().all(|r| r.mode == AgentMode::Ask),
            "All returned records should have Ask mode"
        );
    }

    #[test]
    fn audit_filtering_by_status() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let app_data_dir = dir.path();

        append_agent_audit_record(app_data_dir, &make_record("run-ok-1", AgentMode::Ask, AgentRunStatus::Success))
            .expect("append success record 1");
        append_agent_audit_record(app_data_dir, &make_record("run-fail-1", AgentMode::Plan, AgentRunStatus::Failed))
            .expect("append failed record");
        append_agent_audit_record(app_data_dir, &make_record("run-ok-2", AgentMode::Ask, AgentRunStatus::Success))
            .expect("append success record 2");

        let failed_rows = list_agent_audit_runs(app_data_dir, 100, None, Some(AgentRunStatus::Failed))
            .expect("list failed audit runs");

        assert_eq!(failed_rows.len(), 1, "Filtering by Failed status should return exactly 1 record");
        assert_eq!(failed_rows[0].run_id, "run-fail-1", "Failed record run_id must match");
        assert_eq!(failed_rows[0].result_status, AgentRunStatus::Failed, "Status must be Failed");
    }
}
