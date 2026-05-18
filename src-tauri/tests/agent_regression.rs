#[cfg(test)]
mod agent_regression {
    use fndr_lib::agent::{build_agent_context_pack, AgentContextRequest, AgentMode};
    use fndr_lib::AppState;
    use std::sync::Arc;
    use tokio::runtime::Runtime;

    fn setup_test_state() -> Arc<AppState> {
        // Create AppState with tempfile dir
        // (Actual setup depends on AppState constructor)
        todo!("Implement test AppState factory")
    }

    #[ignore = "requires AppState factory - implement in later task"]
    #[test]
    fn agent_context_pack_is_deterministic() {
        let rt = Runtime::new().unwrap();
        let state = setup_test_state();
        let req = AgentContextRequest {
            user_goal: "test".to_string(),
            mode: AgentMode::Ask,
            ..Default::default()
        };

        let pack1 = rt
            .block_on(build_agent_context_pack(&state, req.clone()))
            .unwrap();
        let pack2 = rt.block_on(build_agent_context_pack(&state, req)).unwrap();

        assert_eq!(
            pack1.task_id, pack2.task_id,
            "Context pack must be deterministic"
        );
    }

    #[ignore = "requires AppState factory - implement in later task"]
    #[test]
    fn ask_mode_has_no_proposed_actions() {
        // Verify Ask mode doesn't generate AgentAction proposals
        todo!()
    }

    #[ignore = "requires AppState factory - implement in later task"]
    #[test]
    fn plan_mode_proposes_but_does_not_execute() {
        // Verify Plan mode returns proposed_actions but status is not Executed
        todo!()
    }

    #[ignore = "requires AppState factory - implement in later task"]
    #[test]
    fn audit_record_is_created_for_every_run() {
        // Verify every agent.run call appends to audit JSONL
        todo!()
    }

    #[ignore = "requires AppState factory - implement in later task"]
    #[test]
    fn policy_defaults_deny_dangerous_actions() {
        // Verify Act mode policy blocks rm, sudo, git push, etc.
        todo!()
    }
}

#[cfg(test)]
mod action_policy {
    use fndr_lib::agent::actions::{policy_for_action, AgentActionKind};
    use fndr_lib::agent::policy::{AgentMode, RiskLevel};

    #[test]
    fn ask_mode_blocks_all_actions() {
        let decision =
            policy_for_action(&AgentActionKind::OpenUrl, &RiskLevel::Low, &AgentMode::Ask);
        assert!(!decision.allowed, "Ask mode must block all actions");
        assert!(decision.blocked_because.is_some());
    }

    #[test]
    fn plan_mode_proposes_but_requires_approval() {
        let decision =
            policy_for_action(&AgentActionKind::OpenUrl, &RiskLevel::Low, &AgentMode::Plan);
        assert!(decision.allowed, "Plan mode should allow proposals");
        assert!(
            decision.requires_approval,
            "Plan mode requires approval to execute"
        );
    }

    #[test]
    fn high_risk_command_is_blocked_in_act_mode() {
        let decision = policy_for_action(
            &AgentActionKind::RunReadOnlyCommand,
            &RiskLevel::High,
            &AgentMode::Act,
        );
        assert!(!decision.allowed, "High-risk commands must be blocked");
    }

    #[test]
    fn low_risk_open_url_allowed_without_approval() {
        let decision =
            policy_for_action(&AgentActionKind::OpenUrl, &RiskLevel::Low, &AgentMode::Act);
        assert!(decision.allowed);
        assert!(!decision.requires_approval);
    }

    #[test]
    fn unsupported_action_always_blocked() {
        let decision = policy_for_action(
            &AgentActionKind::Unsupported,
            &RiskLevel::Low,
            &AgentMode::Act,
        );
        assert!(!decision.allowed);
        assert!(decision.blocked_because.is_some());
    }
}

#[cfg(test)]
mod audit_persistence {
    use fndr_lib::agent::audit::{
        append_agent_audit_record, list_agent_audit_runs, AgentAuditRecord, AgentRunStatus,
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

        let rows = list_agent_audit_runs(app_data_dir, 10, None, None).expect("list audit runs");

        assert_eq!(rows.len(), 1, "Expected exactly one audit record");
        assert_eq!(rows[0].run_id, "run-persist-1", "run_id must round-trip");
    }

    #[test]
    fn audit_filtering_by_mode() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let app_data_dir = dir.path();

        append_agent_audit_record(
            app_data_dir,
            &make_record("run-ask-1", AgentMode::Ask, AgentRunStatus::Success),
        )
        .expect("append ask record 1");
        append_agent_audit_record(
            app_data_dir,
            &make_record("run-ask-2", AgentMode::Ask, AgentRunStatus::Partial),
        )
        .expect("append ask record 2");
        append_agent_audit_record(
            app_data_dir,
            &make_record("run-plan-1", AgentMode::Plan, AgentRunStatus::Success),
        )
        .expect("append plan record");

        let ask_rows = list_agent_audit_runs(app_data_dir, 100, Some(AgentMode::Ask), None)
            .expect("list ask audit runs");

        assert_eq!(
            ask_rows.len(),
            2,
            "Filtering by Ask mode should return exactly 2 records"
        );
        assert!(
            ask_rows.iter().all(|r| r.mode == AgentMode::Ask),
            "All returned records should have Ask mode"
        );
    }

    #[test]
    fn audit_filtering_by_status() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let app_data_dir = dir.path();

        append_agent_audit_record(
            app_data_dir,
            &make_record("run-ok-1", AgentMode::Ask, AgentRunStatus::Success),
        )
        .expect("append success record 1");
        append_agent_audit_record(
            app_data_dir,
            &make_record("run-fail-1", AgentMode::Plan, AgentRunStatus::Failed),
        )
        .expect("append failed record");
        append_agent_audit_record(
            app_data_dir,
            &make_record("run-ok-2", AgentMode::Ask, AgentRunStatus::Success),
        )
        .expect("append success record 2");

        let failed_rows =
            list_agent_audit_runs(app_data_dir, 100, None, Some(AgentRunStatus::Failed))
                .expect("list failed audit runs");

        assert_eq!(
            failed_rows.len(),
            1,
            "Filtering by Failed status should return exactly 1 record"
        );
        assert_eq!(
            failed_rows[0].run_id, "run-fail-1",
            "Failed record run_id must match"
        );
        assert_eq!(
            failed_rows[0].result_status,
            AgentRunStatus::Failed,
            "Status must be Failed"
        );
    }
}

#[cfg(test)]
mod command_validation {
    use fndr_lib::agent::validate_command;

    #[test]
    fn git_status_is_allowed() {
        assert!(validate_command("git", &["status"]).is_ok());
    }

    #[test]
    fn git_commit_is_blocked() {
        assert!(validate_command("git", &["commit", "-m", "hack"]).is_err());
    }

    #[test]
    fn rm_is_blocked() {
        assert!(validate_command("rm", &["-rf", "/"]).is_err());
    }

    #[test]
    fn sudo_is_blocked() {
        assert!(validate_command("sudo", &["ls"]).is_err());
    }

    #[test]
    fn cargo_check_is_allowed() {
        assert!(validate_command("cargo", &["check"]).is_ok());
    }

    #[test]
    fn cargo_install_is_blocked() {
        assert!(validate_command("cargo", &["install", "ripgrep"]).is_err());
    }

    #[test]
    fn npm_typecheck_is_allowed() {
        assert!(validate_command("npm", &["run", "typecheck"]).is_ok());
    }

    #[test]
    fn npm_install_is_blocked() {
        assert!(validate_command("npm", &["install"]).is_err());
    }

    #[test]
    fn unknown_command_is_blocked() {
        assert!(validate_command("curl", &["https://example.com"]).is_err());
    }
}
