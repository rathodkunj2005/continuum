use super::actions::{ActionResult, AgentAction, AgentActionKind};
use std::time::Instant;

/// Commands that are safe to execute (read-only)
const SAFE_COMMANDS: &[&str] = &["pwd", "ls", "git", "cargo", "npm"];

/// Subcommand args that are blocked for specific commands
fn is_blocked(cmd: &str, args: &[&str]) -> Option<String> {
    let full = format!("{} {}", cmd, args.join(" "));

    // Block dangerous patterns in the full command string
    let blocked_patterns = [
        "rm",
        "sudo",
        "chmod",
        "chown",
        "curl",
        "wget",
        "npm install",
        "brew install",
        "cargo install",
        "git commit",
        "git push",
        "git reset",
        "git clean",
        "git rebase",
        "git checkout --",
    ];
    for pat in &blocked_patterns {
        if full.contains(pat) {
            return Some(format!("Blocked pattern: {}", pat));
        }
    }

    // Validate per-command subcommands
    match cmd {
        "git" => {
            let allowed = ["status", "diff", "log", "show", "blame", "branch"];
            if let Some(sub) = args.first() {
                if !allowed.contains(sub) {
                    return Some(format!("git subcommand not allowed: {}", sub));
                }
            }
        }
        "cargo" => {
            let allowed = ["check", "test"];
            if let Some(sub) = args.first() {
                if !allowed.contains(sub) {
                    return Some(format!("cargo subcommand not allowed: {}", sub));
                }
            }
        }
        "npm" => {
            if args.first() != Some(&"run") {
                return Some("Only 'npm run' is allowed".to_string());
            }
            let allowed_scripts = ["typecheck", "test"];
            if let Some(script) = args.get(1) {
                if !allowed_scripts.contains(script) {
                    return Some(format!("npm script not allowed: {}", script));
                }
            }
        }
        _ => {}
    }

    None
}

/// Validate a command before execution. Returns Ok(()) or Err(reason).
pub fn validate_command(cmd: &str, args: &[&str]) -> Result<(), String> {
    if !SAFE_COMMANDS.contains(&cmd) {
        return Err(format!("Command not in allowlist: {}", cmd));
    }
    if let Some(reason) = is_blocked(cmd, args) {
        return Err(reason);
    }
    Ok(())
}

/// Execute a validated safe command with a timeout. Returns ActionResult.
pub async fn execute_safe_command(
    cmd: &str,
    args: &[&str],
    timeout_secs: u64,
) -> Result<ActionResult, String> {
    use tokio::process::Command;
    use tokio::time::{timeout, Duration};

    validate_command(cmd, args)?;

    let start = Instant::now();

    let output_fut = Command::new(cmd).args(args).output();

    let output = timeout(Duration::from_secs(timeout_secs), output_fut)
        .await
        .map_err(|_| format!("Command timed out after {}s", timeout_secs))?
        .map_err(|e| format!("Failed to spawn command: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();
    let raw = if success { stdout } else { stderr };

    // Truncate at 5000 chars
    let truncated = if raw.len() > 5_000 {
        format!("{}…(truncated {} bytes)", &raw[..5_000], raw.len() - 5_000)
    } else {
        raw
    };

    Ok(ActionResult {
        success,
        output: truncated,
        error: if success {
            None
        } else {
            Some("Command exited with non-zero status".to_string())
        },
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

/// Execute an approved AgentAction. Only RunReadOnlyCommand is currently handled.
pub async fn execute_action(action: &AgentAction) -> Result<ActionResult, String> {
    match &action.kind {
        AgentActionKind::RunReadOnlyCommand => {
            let cmd = action
                .input
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'command' in action input")?;

            let args: Vec<String> = action
                .input
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            execute_safe_command(cmd, &arg_refs, 30).await
        }
        other => Err(format!("Action kind not yet executable: {:?}", other)),
    }
}
