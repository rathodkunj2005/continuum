//! Hermes bridge, gateway, and agent task Tauri commands.

use crate::context_runtime;
use crate::http_util::{llm_http_client, local_service_client, post_json_response};
use crate::search::MemoryCard;
use crate::AppState;
use chrono::{TimeZone, Timelike};
use parking_lot::Mutex as AgentMutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, OnceLock as AgentOnceLock};
use std::time::UNIX_EPOCH;
use tauri::{Manager, State};
use tokio::time::{Duration, Instant};

use super::common::{strip_internal_fndr_results, truncate_chars};
use super::search::{memory_card_from_result, refine_memory_card_titles};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub is_running: bool,
    pub task_title: Option<String>,
    pub last_message: Option<String>,
    pub status: String, // "idle" | "running" | "completed" | "error"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesAppContext {
    pub app_name: String,
    pub memory_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesMemoryDigest {
    pub title: String,
    pub app_name: String,
    pub summary: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesBridgeStatus {
    pub installed: bool,
    pub configured: bool,
    pub setup_complete: bool,
    pub gateway_running: bool,
    pub api_server_ready: bool,
    pub version: Option<String>,
    pub bundled_repo_available: bool,
    pub runtime_source: Option<String>,
    pub provider_kind: Option<String>,
    pub model_name: Option<String>,
    pub base_url: Option<String>,
    pub api_url: String,
    pub gateway_dir: String,
    pub home_dir: String,
    pub context_path: String,
    pub context_ready: bool,
    pub last_synced_at: Option<i64>,
    pub fndr_local_model_id: Option<String>,
    pub ollama_installed: bool,
    pub ollama_reachable: bool,
    pub ollama_models: Vec<String>,
    pub ollama_base_url: String,
    pub codex_cli_installed: bool,
    pub codex_logged_in: bool,
    pub codex_auth_path: String,
    pub profile_name: Option<String>,
    pub focus_task: Option<String>,
    pub recent_memory_count: u32,
    pub open_task_count: u32,
    /// True when Ollama is reachable and configured — chat works without Hermes CLI.
    pub direct_ollama_ready: bool,
    pub top_apps: Vec<HermesAppContext>,
    pub recent_memories: Vec<HermesMemoryDigest>,
    pub last_error: Option<String>,
    pub install_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesSetupPayload {
    pub provider_kind: String,
    pub model_name: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesChatReply {
    pub response_id: String,
    pub conversation_id: String,
    pub content: String,
}

static AGENT_PROCESS: AgentOnceLock<AgentMutex<Option<Child>>> = AgentOnceLock::new();
static AGENT_STATUS: AgentOnceLock<AgentMutex<AgentStatus>> = AgentOnceLock::new();

fn get_agent_process() -> &'static AgentMutex<Option<Child>> {
    AGENT_PROCESS.get_or_init(|| AgentMutex::new(None))
}

fn get_agent_status_store() -> &'static AgentMutex<AgentStatus> {
    AGENT_STATUS.get_or_init(|| {
        AgentMutex::new(AgentStatus {
            is_running: false,
            task_title: None,
            last_message: None,
            status: "idle".to_string(),
        })
    })
}

#[derive(Debug, Deserialize)]
struct HermesOnboardingProfile {
    display_name: Option<String>,
    model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermesSetupRecord {
    provider_kind: String,
    model_name: String,
    #[serde(default)]
    base_url: Option<String>,
}

static HERMES_GATEWAY_PROCESS: AgentOnceLock<AgentMutex<Option<Child>>> = AgentOnceLock::new();
static HERMES_GATEWAY_ERROR: AgentOnceLock<AgentMutex<Option<String>>> = AgentOnceLock::new();

// Local service endpoints. Keep host/port/path declarations together so the
// Hermes gateway and Ollama probe URLs can be updated in one place rather than
// scattered as literal strings throughout this module.
const HERMES_API_HOST: &str = "127.0.0.1";
const HERMES_API_PORT: u16 = 8742;
const OLLAMA_HOME_URL: &str = "http://127.0.0.1:11434";
const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";
const OLLAMA_API_TAGS_URL: &str = "http://127.0.0.1:11434/api/tags";

fn hermes_gateway_dir(state: &AppState) -> PathBuf {
    state.app_data_dir.join("hermes-gateway")
}

fn hermes_home_dir(state: &AppState) -> PathBuf {
    state.app_data_dir.join("hermes-home")
}

fn hermes_context_path(state: &AppState) -> PathBuf {
    hermes_gateway_dir(state).join("FNDR_CONTEXT.md")
}

fn hermes_project_context_path(state: &AppState) -> PathBuf {
    hermes_gateway_dir(state).join(".hermes.md")
}

fn hermes_gateway_readme_path(state: &AppState) -> PathBuf {
    hermes_gateway_dir(state).join("README.md")
}

fn hermes_env_path(state: &AppState) -> PathBuf {
    hermes_home_dir(state).join(".env")
}

fn hermes_config_path(state: &AppState) -> PathBuf {
    hermes_home_dir(state).join("config.yaml")
}

fn hermes_setup_record_path(state: &AppState) -> PathBuf {
    hermes_home_dir(state).join("fndr_setup.json")
}

fn hermes_soul_path(state: &AppState) -> PathBuf {
    hermes_home_dir(state).join("SOUL.md")
}

fn hermes_api_url() -> String {
    format!("http://{HERMES_API_HOST}:{HERMES_API_PORT}")
}

fn get_hermes_gateway_process() -> &'static AgentMutex<Option<Child>> {
    HERMES_GATEWAY_PROCESS.get_or_init(|| AgentMutex::new(None))
}

fn get_hermes_gateway_error_store() -> &'static AgentMutex<Option<String>> {
    HERMES_GATEWAY_ERROR.get_or_init(|| AgentMutex::new(None))
}

fn read_hermes_profile_name(state: &AppState) -> Option<String> {
    let path = state.app_data_dir.join("onboarding.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<HermesOnboardingProfile>(&raw)
        .ok()?
        .display_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn read_fndr_local_model_id(state: &AppState) -> Option<String> {
    let path = state.app_data_dir.join("onboarding.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<HermesOnboardingProfile>(&raw)
        .ok()?
        .model_id
        .map(|model_id| model_id.trim().to_string())
        .filter(|model_id| !model_id.is_empty())
}

#[derive(Debug, Clone)]
enum HermesLauncher {
    Bundled { python: PathBuf, script: PathBuf },
    System { executable: PathBuf },
}

impl HermesLauncher {
    fn command(&self) -> Command {
        match self {
            Self::Bundled { python, script } => {
                let mut command = Command::new(python);
                command.arg(script);
                command
            }
            Self::System { executable } => Command::new(executable),
        }
    }
}

#[derive(Debug, Clone)]
struct HermesRuntimeStatus {
    installed: bool,
    version: Option<String>,
    launcher: Option<HermesLauncher>,
    bundled_repo_path: Option<PathBuf>,
    runtime_source: Option<String>,
}

impl HermesRuntimeStatus {
    fn bundled_repo_available(&self) -> bool {
        self.bundled_repo_path.is_some()
    }
}

fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn hermes_runtime_root(state: &AppState) -> PathBuf {
    state.app_data_dir.join("hermes-runtime")
}

fn hermes_runtime_bin_dir(state: &AppState) -> PathBuf {
    hermes_runtime_root(state).join("bin")
}

fn hermes_runtime_python_dir(state: &AppState) -> PathBuf {
    hermes_runtime_root(state).join("python")
}

fn hermes_runtime_venv_dir(state: &AppState) -> PathBuf {
    hermes_runtime_root(state).join("venv")
}

fn hermes_runtime_python_path(state: &AppState) -> PathBuf {
    hermes_runtime_venv_dir(state).join("bin").join("python3")
}

fn hermes_uv_path(state: &AppState) -> PathBuf {
    hermes_runtime_bin_dir(state).join("uv")
}

fn is_hermes_repo(path: &Path) -> bool {
    path.join("pyproject.toml").exists() && path.join("hermes").exists()
}

fn resolve_bundled_hermes_repo() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(value) = std::env::var_os("FNDR_HERMES_REPO") {
        candidates.push(PathBuf::from(value));
    }

    let manifest_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hermes-agent");
    candidates.push(manifest_candidate);

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("hermes-agent"));
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("../Resources/hermes-agent"));
            for ancestor in exe_dir.ancestors().take(6) {
                candidates.push(ancestor.join("hermes-agent"));
            }
        }
    }

    candidates
        .into_iter()
        .find(|candidate| is_hermes_repo(candidate))
}

fn read_hermes_repo_version(repo_root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(repo_root.join("pyproject.toml")).ok()?;
    let value = raw.parse::<toml::Value>().ok()?;
    value
        .get("project")
        .and_then(|project| project.get("version"))
        .and_then(|version| version.as_str())
        .map(str::to_string)
}

fn existing_executable_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn find_existing_executable(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn common_executable_candidates(name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(home) = user_home_dir() {
        candidates.push(home.join(".local/bin").join(name));
        candidates.push(home.join(".cargo/bin").join(name));
        candidates.push(home.join(".npm-global/bin").join(name));
    }

    candidates.push(PathBuf::from("/opt/homebrew/bin").join(name));
    candidates.push(PathBuf::from("/usr/local/bin").join(name));
    candidates
}

fn detect_system_hermes_executable() -> Option<PathBuf> {
    existing_executable_path("hermes")
        .or_else(|| find_existing_executable(common_executable_candidates("hermes")))
}

fn detect_uv_executable(state: &AppState) -> Option<PathBuf> {
    let bundled_uv = hermes_uv_path(state);
    if bundled_uv.exists() {
        return Some(bundled_uv);
    }

    existing_executable_path("uv")
        .or_else(|| find_existing_executable(common_executable_candidates("uv")))
}

fn detect_ollama_executable() -> Option<PathBuf> {
    let mut candidates = common_executable_candidates("ollama");
    candidates.push(PathBuf::from(
        "/Applications/Ollama.app/Contents/Resources/ollama",
    ));
    existing_executable_path("ollama").or_else(|| find_existing_executable(candidates))
}

fn detect_codex_executable() -> Option<PathBuf> {
    existing_executable_path("codex")
        .or_else(|| find_existing_executable(common_executable_candidates("codex")))
}

fn version_from_output(output: &std::process::Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stdout.is_empty() {
        stderr.lines().next().map(str::to_string)
    } else {
        stdout.lines().next().map(str::to_string)
    }
}

fn command_failure_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "No diagnostic output was returned.".to_string()
    }
}

fn configure_uv_command(command: &mut Command, state: &AppState) -> Result<(), String> {
    let runtime_root = hermes_runtime_root(state);
    let xdg_cache_home = runtime_root.join("xdg-cache");
    let xdg_data_home = runtime_root.join("xdg-data");
    let xdg_config_home = runtime_root.join("xdg-config");
    let tool_dir = runtime_root.join("tools");
    let tool_bin_dir = hermes_runtime_bin_dir(state);
    let python_dir = hermes_runtime_python_dir(state);
    let project_env = hermes_runtime_venv_dir(state);

    for dir in [
        &runtime_root,
        &xdg_cache_home,
        &xdg_data_home,
        &xdg_config_home,
        &tool_dir,
        &tool_bin_dir,
        &python_dir,
    ] {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }

    command
        .env("XDG_CACHE_HOME", xdg_cache_home)
        .env("XDG_DATA_HOME", xdg_data_home)
        .env("XDG_CONFIG_HOME", xdg_config_home)
        .env("UV_TOOL_DIR", tool_dir)
        .env("UV_TOOL_BIN_DIR", tool_bin_dir)
        .env("UV_PYTHON_INSTALL_DIR", python_dir)
        .env("UV_PROJECT_ENVIRONMENT", project_env)
        .env("UV_NO_PROGRESS", "1");

    Ok(())
}

fn ensure_uv_available(state: &AppState) -> Result<PathBuf, String> {
    if let Some(path) = detect_uv_executable(state) {
        return Ok(path);
    }

    let install_dir = hermes_runtime_bin_dir(state);
    std::fs::create_dir_all(&install_dir).map_err(|e| e.to_string())?;

    let output = Command::new("sh")
        .arg("-lc")
        .arg("curl -LsSf https://astral.sh/uv/install.sh | sh")
        .env("UV_UNMANAGED_INSTALL", &install_dir)
        .env("UV_NO_MODIFY_PATH", "1")
        .output()
        .map_err(|e| format!("Failed to install uv for the bundled Hermes runtime: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "FNDR could not prepare its private Hermes runtime because uv failed to install. {}",
            command_failure_detail(&output)
        ));
    }

    detect_uv_executable(state).ok_or_else(|| {
        "uv installed successfully, but FNDR could not locate the resulting binary.".to_string()
    })
}

fn prepare_vendored_hermes_runtime(state: &AppState) -> Result<(), String> {
    let repo_root = resolve_bundled_hermes_repo().ok_or_else(|| {
        "FNDR could not find the vendored hermes-agent clone. Expected a bundled `hermes-agent/` directory."
            .to_string()
    })?;
    let uv = ensure_uv_available(state)?;
    let venv_dir = hermes_runtime_venv_dir(state);

    let mut venv_command = Command::new(&uv);
    venv_command
        .arg("venv")
        .arg(&venv_dir)
        .arg("--python")
        .arg("3.11");
    configure_uv_command(&mut venv_command, state)?;
    let venv_output = venv_command
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Failed to create the FNDR Hermes environment: {e}"))?;
    if !venv_output.status.success() {
        return Err(format!(
            "FNDR could not create the bundled Hermes environment. {}",
            command_failure_detail(&venv_output)
        ));
    }

    let mut sync_command = Command::new(&uv);
    sync_command.arg("sync").arg("--locked");
    for extra in ["messaging", "pty", "honcho", "mcp", "acp"] {
        sync_command.arg("--extra").arg(extra);
    }
    configure_uv_command(&mut sync_command, state)?;
    let sync_output = sync_command
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Failed to install Hermes dependencies for FNDR: {e}"))?;
    if !sync_output.status.success() {
        return Err(format!(
            "FNDR could not finish installing Hermes dependencies. {}",
            command_failure_detail(&sync_output)
        ));
    }

    if !hermes_runtime_python_path(state).exists() {
        return Err(
            "FNDR prepared the Hermes runtime, but the private Python interpreter is missing."
                .to_string(),
        );
    }

    Ok(())
}

fn detect_hermes_runtime(state: &AppState) -> HermesRuntimeStatus {
    let bundled_repo_path = resolve_bundled_hermes_repo();
    let bundled_version = bundled_repo_path
        .as_deref()
        .and_then(read_hermes_repo_version);

    if let Some(repo_root) = bundled_repo_path.clone() {
        let python = hermes_runtime_python_path(state);
        let script = repo_root.join("hermes");
        if python.exists() && script.exists() {
            return HermesRuntimeStatus {
                installed: true,
                version: bundled_version.clone(),
                launcher: Some(HermesLauncher::Bundled { python, script }),
                bundled_repo_path: Some(repo_root),
                runtime_source: Some("bundled".to_string()),
            };
        }
    }

    if let Some(executable) = detect_system_hermes_executable() {
        let mut command = Command::new(&executable);
        let output = command.arg("--version").output();
        if let Ok(output) = output {
            if output.status.success() {
                return HermesRuntimeStatus {
                    installed: true,
                    version: version_from_output(&output).or(bundled_version.clone()),
                    launcher: Some(HermesLauncher::System { executable }),
                    bundled_repo_path,
                    runtime_source: Some("system".to_string()),
                };
            }
        }
    }

    HermesRuntimeStatus {
        installed: false,
        version: bundled_version,
        launcher: None,
        bundled_repo_path,
        runtime_source: None,
    }
}

fn detect_ollama_installation() -> bool {
    detect_ollama_executable()
        .and_then(|executable| {
            Command::new(executable)
                .arg("--version")
                .output()
                .ok()
                .filter(|output| output.status.success())
        })
        .is_some()
}

fn parse_ollama_list_output(output: &str) -> Vec<String> {
    let mut models = output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed
                .split_whitespace()
                .next()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.eq_ignore_ascii_case("name"))
        })
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

async fn detect_ollama_state() -> (bool, bool, Vec<String>) {
    let installed = detect_ollama_installation();
    let mut reachable = false;
    let mut models: Vec<String> = Vec::new();

    if let Ok(client) = local_service_client() {
        if let Ok(response) = client.get(OLLAMA_API_TAGS_URL).send().await {
            if response.status().is_success() {
                reachable = true;
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    models = json
                        .get("models")
                        .and_then(|value| value.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|item| item.get("name").and_then(|value| value.as_str()))
                        .map(str::to_string)
                        .collect();
                }
            }
        }
    }

    if models.is_empty() && installed {
        if let Some(ollama) = detect_ollama_executable() {
            if let Ok(output) = Command::new(ollama).arg("list").output() {
                if output.status.success() {
                    reachable = true;
                    models = parse_ollama_list_output(&String::from_utf8_lossy(&output.stdout));
                }
            }
        }
    }

    models.sort();
    models.dedup();
    (installed, reachable, models)
}

fn codex_home_dir() -> PathBuf {
    if let Some(value) = std::env::var_os("CODEX_HOME") {
        return PathBuf::from(value);
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

fn codex_auth_path() -> PathBuf {
    codex_home_dir().join("auth.json")
}

fn detect_codex_state() -> (bool, bool, PathBuf) {
    let auth_path = codex_auth_path();
    let cli_installed = detect_codex_executable()
        .and_then(|executable| {
            Command::new(executable)
                .arg("--help")
                .output()
                .ok()
                .filter(|output| output.status.success())
        })
        .is_some();

    let logged_in = std::fs::read_to_string(&auth_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .map(|json| {
            json.get("OPENAI_API_KEY")
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.trim().is_empty())
                || json
                    .get("tokens")
                    .and_then(|value| value.as_object())
                    .is_some_and(|tokens| !tokens.is_empty())
                || json
                    .get("tokens")
                    .and_then(|value| value.as_array())
                    .is_some_and(|tokens| !tokens.is_empty())
        })
        .unwrap_or(false);

    (cli_installed, logged_in, auth_path)
}

fn read_hermes_setup_record(state: &AppState) -> Option<HermesSetupRecord> {
    let raw = std::fs::read_to_string(hermes_setup_record_path(state)).ok()?;
    serde_json::from_str::<HermesSetupRecord>(&raw).ok()
}

fn persist_hermes_setup_files(state: &AppState, setup: &HermesSetupPayload) -> Result<(), String> {
    let home_dir = hermes_home_dir(state);
    std::fs::create_dir_all(&home_dir).map_err(|e| e.to_string())?;
    let api_key = setup
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let record = HermesSetupRecord {
        provider_kind: setup.provider_kind.trim().to_string(),
        model_name: setup.model_name.trim().to_string(),
        base_url: setup
            .base_url
            .as_ref()
            .map(|value| value.trim().to_string()),
    };

    let config_yaml = match record.provider_kind.as_str() {
        "ollama" => {
            let base_url = record
                .base_url
                .clone()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| OLLAMA_BASE_URL.to_string());
            format!(
                "model:\n  provider: custom\n  default: {}\n  base_url: {}\n  context_length: 32768\n",
                toml::to_string(&record.model_name)
                    .map_err(|e| e.to_string())?
                    .trim(),
                toml::to_string(&base_url)
                    .map_err(|e| e.to_string())?
                    .trim(),
            )
        }
        "codex" => format!(
            "model:\n  provider: codex\n  default: {}\n",
            toml::to_string(&record.model_name)
                .map_err(|e| e.to_string())?
                .trim(),
        ),
        "custom" => {
            let base_url = record
                .base_url
                .clone()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "A base URL is required for a custom endpoint.".to_string())?;
            format!(
                "model:\n  provider: custom\n  default: {}\n  base_url: {}\n",
                toml::to_string(&record.model_name)
                    .map_err(|e| e.to_string())?
                    .trim(),
                toml::to_string(&base_url)
                    .map_err(|e| e.to_string())?
                    .trim(),
            )
        }
        _ => format!(
            "model:\n  provider: {}\n  default: {}\n",
            record.provider_kind,
            toml::to_string(&record.model_name)
                .map_err(|e| e.to_string())?
                .trim(),
        ),
    };

    let mut env_lines = vec![
        "API_SERVER_ENABLED=true".to_string(),
        format!("API_SERVER_HOST={HERMES_API_HOST}"),
        format!("API_SERVER_PORT={HERMES_API_PORT}"),
        format!("API_SERVER_KEY={}", uuid::Uuid::new_v4()),
        "API_SERVER_MODEL_NAME=hermes-agent".to_string(),
    ];

    match record.provider_kind.as_str() {
        "custom" | "ollama" => {
            if let Some(api_key) = api_key {
                env_lines.push(format!("OPENAI_API_KEY={api_key}"));
            }
        }
        "openrouter" => {
            if let Some(api_key) = api_key {
                env_lines.push(format!("OPENROUTER_API_KEY={api_key}"));
            }
        }
        _ => {}
    }

    let soul_md = r#"# FNDR Agent Identity

You are the native FNDR agent experience, powered by Hermes under the hood.

- Present yourself as FNDR's built-in agent unless the user asks how you are implemented.
- FNDR is the user's trusted interface and source of truth for personal context.
- Treat FNDR-provided memory, tasks, and focus context as private and read-only.
- Ask before destructive actions, external sends, purchases, or credential changes.
- Prefer helping with recall, planning, drafting, research, and safe computer-use assistance.
"#;

    let record_json = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    std::fs::write(hermes_config_path(state), config_yaml).map_err(|e| e.to_string())?;
    std::fs::write(hermes_env_path(state), env_lines.join("\n") + "\n")
        .map_err(|e| e.to_string())?;
    std::fs::write(hermes_soul_path(state), soul_md).map_err(|e| e.to_string())?;
    std::fs::write(hermes_setup_record_path(state), record_json).map_err(|e| e.to_string())?;
    Ok(())
}

fn read_hermes_api_key(state: &AppState) -> Option<String> {
    let env_contents = std::fs::read_to_string(hermes_env_path(state)).ok()?;
    env_contents.lines().find_map(|line| {
        let value = line.strip_prefix("API_SERVER_KEY=")?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn update_hermes_gateway_runtime() -> (bool, Option<String>) {
    let mut process_guard = get_hermes_gateway_process().lock();
    if let Some(child) = process_guard.as_mut() {
        match child.try_wait() {
            Ok(Some(status)) => {
                let message = if status.success() {
                    "Hermes gateway exited.".to_string()
                } else {
                    format!("Hermes gateway exited with status {status}.")
                };
                *get_hermes_gateway_error_store().lock() = Some(message.clone());
                *process_guard = None;
                (false, Some(message))
            }
            Ok(None) => (true, get_hermes_gateway_error_store().lock().clone()),
            Err(err) => {
                let message = format!("Failed to inspect Hermes gateway: {err}");
                *get_hermes_gateway_error_store().lock() = Some(message.clone());
                *process_guard = None;
                (false, Some(message))
            }
        }
    } else {
        (false, get_hermes_gateway_error_store().lock().clone())
    }
}

async fn hermes_api_ready() -> bool {
    let Ok(client) = local_service_client() else {
        return false;
    };
    match client
        .get(format!("{}/health", hermes_api_url()))
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

fn file_modified_at_ms(path: &PathBuf) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(duration.as_millis().min(i64::MAX as u128) as i64)
}

fn format_hermes_timestamp(timestamp_ms: i64) -> String {
    chrono::Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .map(|dt| dt.format("%b %d, %Y at %I:%M %p").to_string())
        .unwrap_or_else(|| "Unknown time".to_string())
}

async fn build_hermes_bridge_status(state: &AppState) -> Result<HermesBridgeStatus, String> {
    let context_path = hermes_context_path(state);
    let home_dir = hermes_home_dir(state);
    let recent_results = state
        .store
        .list_recent_results(18, None)
        .await
        .map_err(|e| e.to_string())?;
    let mut recent_memories: Vec<MemoryCard> = strip_internal_fndr_results(recent_results)
        .into_iter()
        .map(memory_card_from_result)
        .collect();
    refine_memory_card_titles(&mut recent_memories);

    let mut app_counts: HashMap<String, usize> = HashMap::new();
    for memory in &recent_memories {
        *app_counts.entry(memory.app_name.clone()).or_insert(0) += 1;
    }

    let mut top_apps: Vec<HermesAppContext> = app_counts
        .into_iter()
        .map(|(app_name, memory_count)| HermesAppContext {
            app_name,
            memory_count: memory_count as u32,
        })
        .collect();
    top_apps.sort_by(|left, right| {
        right
            .memory_count
            .cmp(&left.memory_count)
            .then_with(|| left.app_name.cmp(&right.app_name))
    });
    top_apps.truncate(6);

    let recent_memories = recent_memories
        .into_iter()
        .take(6)
        .map(|memory| HermesMemoryDigest {
            title: memory.title,
            app_name: memory.app_name,
            summary: truncate_chars(&memory.summary, 180),
            timestamp: memory.timestamp,
        })
        .collect::<Vec<_>>();

    let open_task_count = state
        .store
        .list_tasks()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|task| !task.is_completed && !task.is_dismissed)
        .count() as u32;

    let runtime = detect_hermes_runtime(state);
    let (ollama_installed, ollama_reachable, ollama_models) = detect_ollama_state().await;
    let (codex_cli_installed, codex_logged_in, codex_auth_path) = detect_codex_state();
    let setup = read_hermes_setup_record(state);
    let configured = setup.is_some();
    let (gateway_running, last_error) = update_hermes_gateway_runtime();
    let api_server_ready = if gateway_running {
        hermes_api_ready().await
    } else {
        false
    };

    // Direct Ollama mode: provider configured as ollama + Ollama reachable + has models.
    // This works without the Hermes CLI being installed at all.
    let direct_ollama_ready = setup
        .as_ref()
        .map(|s| s.provider_kind == "ollama")
        .unwrap_or(false)
        && ollama_reachable
        && !ollama_models.is_empty();

    let bundled_repo_available = runtime.bundled_repo_available();
    let runtime_source = runtime.runtime_source.clone();
    let version = runtime.version.clone();

    Ok(HermesBridgeStatus {
        installed: runtime.installed,
        configured,
        setup_complete: configured && (runtime.installed || direct_ollama_ready),
        gateway_running,
        api_server_ready,
        direct_ollama_ready,
        version,
        bundled_repo_available,
        runtime_source,
        provider_kind: setup.as_ref().map(|value| value.provider_kind.clone()),
        model_name: setup.as_ref().map(|value| value.model_name.clone()),
        base_url: setup.as_ref().and_then(|value| {
            if value.provider_kind == "ollama" {
                Some(
                    value
                        .base_url
                        .clone()
                        .unwrap_or_else(|| OLLAMA_BASE_URL.to_string()),
                )
            } else {
                value.base_url.clone()
            }
        }),
        api_url: hermes_api_url(),
        gateway_dir: hermes_gateway_dir(state).display().to_string(),
        home_dir: home_dir.display().to_string(),
        context_path: context_path.display().to_string(),
        context_ready: context_path.exists(),
        last_synced_at: file_modified_at_ms(&context_path),
        fndr_local_model_id: read_fndr_local_model_id(state),
        ollama_installed,
        ollama_reachable,
        ollama_models,
        ollama_base_url: OLLAMA_BASE_URL.to_string(),
        codex_cli_installed,
        codex_logged_in,
        codex_auth_path: codex_auth_path.display().to_string(),
        profile_name: read_hermes_profile_name(state),
        focus_task: state.focus_task.read().clone(),
        recent_memory_count: recent_memories.len() as u32,
        open_task_count,
        top_apps,
        recent_memories,
        last_error,
        install_command: if bundled_repo_available {
            "Prepare the bundled Hermes runtime inside FNDR.".to_string()
        } else {
            "curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash".to_string()
        },
    })
}

fn render_hermes_context_markdown(status: &HermesBridgeStatus) -> String {
    let profile_line = status
        .profile_name
        .as_deref()
        .map(|name| format!("- Preferred name: {name}"))
        .unwrap_or_else(|| "- Preferred name: not set in FNDR onboarding".to_string());
    let focus_line = status
        .focus_task
        .as_deref()
        .map(|task| format!("- Focus task: {task}"))
        .unwrap_or_else(|| "- Focus task: none currently pinned in FNDR".to_string());

    let app_lines = if status.top_apps.is_empty() {
        "- No recent app clusters captured yet.".to_string()
    } else {
        status
            .top_apps
            .iter()
            .map(|app| format!("- {} ({})", app.app_name, app.memory_count))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let memory_lines = if status.recent_memories.is_empty() {
        "- No recent memories are available yet.".to_string()
    } else {
        status
            .recent_memories
            .iter()
            .map(|memory| {
                format!(
                    "- {} [{}] {}\n  {}\n  {}",
                    memory.title,
                    memory.app_name,
                    format_hermes_timestamp(memory.timestamp),
                    memory.summary,
                    "Treat this as private user context from FNDR."
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# FNDR Hermes Gateway\n\n\
This workspace is generated by FNDR and should feel like part of FNDR, not a separate product.\n\n\
## Operating mode\n\n\
- FNDR is the source of truth for personal context.\n\
- Use the FNDR snapshot below to help the user with recall, planning, drafting, research, and safe computer-use support.\n\
- Be grounded: if the FNDR snapshot is missing or stale, ask the user to refresh it in FNDR instead of guessing.\n\
- Operate as FNDR's built-in agent experience unless the user asks about implementation details.\n\
- Ask for approval before sending messages, making purchases, changing credentials, or doing irreversible actions.\n\
- Treat FNDR context as read-only and privacy-sensitive.\n\n\
## FNDR snapshot\n\n\
{profile_line}\n\
{focus_line}\n\
- Open FNDR tasks: {}\n\
- Recent memory cards included: {}\n\n\
## Recent apps from FNDR\n\n\
{app_lines}\n\n\
## Recent memories from FNDR\n\n\
{memory_lines}\n",
        status.open_task_count, status.recent_memory_count
    )
}

fn render_hermes_gateway_readme(status: &HermesBridgeStatus) -> String {
    format!(
        "# FNDR Hermes Gateway\n\n\
FNDR generated this workspace so Hermes can operate with FNDR-curated context.\n\n\
Files:\n\
- `.hermes.md` keeps the FNDR-native operating instructions and latest snapshot.\n\
- `FNDR_CONTEXT.md` mirrors the same snapshot in a user-readable file.\n\n\
If the snapshot feels stale, refresh it from FNDR's Agent page.\n\n\
Gateway directory: {}\n",
        status.gateway_dir
    )
}

async fn sync_hermes_bridge_files(state: &AppState) -> Result<HermesBridgeStatus, String> {
    let mut status = build_hermes_bridge_status(state).await?;
    let gateway_dir = hermes_gateway_dir(state);
    std::fs::create_dir_all(&gateway_dir).map_err(|e| e.to_string())?;

    let context_markdown = match context_runtime::build_context_pack(
        state,
        context_runtime::ContextRequest {
            query: String::new(),
            agent_type: "chat_agent".to_string(),
            budget_tokens: 1200,
            session_id: Some("hermes-bridge".to_string()),
            active_files: Vec::new(),
            project: None,
        },
    )
    .await
    {
        Ok(pack) => {
            let profile_line = status
                .profile_name
                .as_deref()
                .map(|name| format!("- Preferred name: {name}"))
                .unwrap_or_else(|| "- Preferred name: not set in FNDR onboarding".to_string());
            let focus_line = status
                .focus_task
                .as_deref()
                .map(|task| format!("- Focus task: {task}"))
                .unwrap_or_else(|| "- Focus task: none currently pinned in FNDR".to_string());

            format!(
                "# FNDR Hermes Gateway\n\n\
This workspace is generated by FNDR and should feel like part of FNDR, not a separate product.\n\n\
{profile_line}\n\
{focus_line}\n\n\
{}",
                context_runtime::render_pack_markdown(&pack)
            )
        }
        Err(err) => {
            tracing::warn!("Falling back to legacy Hermes context snapshot: {}", err);
            render_hermes_context_markdown(&status)
        }
    };
    std::fs::write(hermes_project_context_path(state), &context_markdown)
        .map_err(|e| e.to_string())?;
    std::fs::write(hermes_context_path(state), &context_markdown).map_err(|e| e.to_string())?;
    std::fs::write(
        hermes_gateway_readme_path(state),
        render_hermes_gateway_readme(&status),
    )
    .map_err(|e| e.to_string())?;

    status.context_ready = true;
    status.last_synced_at = file_modified_at_ms(&hermes_context_path(state));
    Ok(status)
}

async fn wait_for_hermes_api(timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if hermes_api_ready().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

fn validate_hermes_gateway_prerequisites(status: &HermesBridgeStatus) -> Result<(), String> {
    if !status.installed {
        return Err(if status.bundled_repo_available {
            "FNDR has a bundled Hermes clone, but the private runtime is not prepared yet. Click Enable Agent in the FNDR Agent panel first."
                .to_string()
        } else {
            "Hermes is not installed yet.".to_string()
        });
    }
    if !status.configured {
        return Err("Finish FNDR Agent setup before starting the runtime.".to_string());
    }
    if status.provider_kind.as_deref() == Some("ollama") {
        if !status.ollama_installed {
            return Err(
                "Install Ollama on this Mac before starting the FNDR agent in Ollama mode."
                    .to_string(),
            );
        }
        if !status.ollama_reachable {
            return Err(format!(
                "FNDR could not reach Ollama at {OLLAMA_HOME_URL}. Open Ollama or run `ollama serve`, then try again."
            ));
        }
    }
    if status.provider_kind.as_deref() == Some("codex") && !status.codex_logged_in {
        return Err(
            "FNDR could not find an active Codex login for the agent runtime. Sign in to Codex on this Mac first."
                .to_string(),
        );
    }
    Ok(())
}

async fn ensure_hermes_gateway_ready(
    state: &AppState,
    timeout_ms: u64,
) -> Result<HermesBridgeStatus, String> {
    let status = sync_hermes_bridge_files(state).await?;
    validate_hermes_gateway_prerequisites(&status)?;

    if status.api_server_ready {
        return Ok(status);
    }

    let (running, _) = update_hermes_gateway_runtime();
    if !running {
        let launcher = detect_hermes_runtime(state)
            .launcher
            .ok_or_else(|| "FNDR could not resolve a Hermes runtime to launch.".to_string())?;
        let mut command = launcher.command();
        command
            .arg("gateway")
            .env("HERMES_HOME", hermes_home_dir(state))
            .current_dir(hermes_gateway_dir(state))
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if status.codex_logged_in {
            command.env("CODEX_HOME", codex_home_dir());
        }

        let child = command
            .spawn()
            .map_err(|e| format!("Failed to start Hermes gateway: {e}"))?;

        *get_hermes_gateway_process().lock() = Some(child);
        *get_hermes_gateway_error_store().lock() = None;
    }

    if !wait_for_hermes_api(timeout_ms).await {
        let message =
            "Hermes gateway started, but the local API server did not come online in time."
                .to_string();
        *get_hermes_gateway_error_store().lock() = Some(message.clone());
        return Err(message);
    }

    let ready_status = build_hermes_bridge_status(state).await?;
    if ready_status.api_server_ready {
        Ok(ready_status)
    } else {
        Err(ready_status
            .last_error
            .clone()
            .unwrap_or_else(|| "Hermes gateway is still unavailable.".to_string()))
    }
}

#[tauri::command]
pub async fn get_hermes_bridge_status(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    build_hermes_bridge_status(state.inner()).await
}

#[tauri::command]
pub async fn sync_hermes_bridge_context(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    sync_hermes_bridge_files(state.inner()).await
}

#[tauri::command]
pub async fn install_hermes_bridge(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    let runtime = detect_hermes_runtime(state.inner());
    if runtime.installed {
        return build_hermes_bridge_status(state.inner()).await;
    }

    if runtime.bundled_repo_available() {
        prepare_vendored_hermes_runtime(state.inner())?;
    } else {
        let install_command = "curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash";
        let output = Command::new("sh")
            .arg("-lc")
            .arg(install_command)
            .output()
            .map_err(|e| format!("Failed to run Hermes installer: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "Hermes install failed. {}",
                command_failure_detail(&output)
            ));
        }
    }

    build_hermes_bridge_status(state.inner()).await
}

#[tauri::command]
pub async fn save_hermes_setup(
    state: State<'_, Arc<AppState>>,
    payload: HermesSetupPayload,
) -> Result<HermesBridgeStatus, String> {
    let provider_kind = payload.provider_kind.trim();
    let model_name = payload.model_name.trim();
    if model_name.is_empty() {
        return Err("Choose a model name for the FNDR agent.".to_string());
    }

    let api_key = payload
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match provider_kind {
        "openrouter" => {
            if api_key.is_none() {
                return Err(
                    "An OpenRouter API key is required to finish FNDR Agent setup.".to_string(),
                );
            }
        }
        "custom" => {
            let has_base_url = payload
                .base_url
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            if !has_base_url {
                return Err("A base URL is required for a custom endpoint.".to_string());
            }
        }
        "ollama" => {
            let (ollama_installed, _, _) = detect_ollama_state().await;
            if !ollama_installed {
                return Err(
                    "FNDR could not find Ollama on this Mac. Install Ollama first, then return to the Agent page."
                        .to_string(),
                );
            }
        }
        "codex" => {
            let (_, codex_logged_in, _) = detect_codex_state();
            if !codex_logged_in {
                return Err(
                    "FNDR could not find a local Codex login yet. Sign in to Codex on this Mac first, then choose Codex again."
                        .to_string(),
                );
            }
        }
        _ => {
            return Err(
                "FNDR currently supports agent setup via Ollama, Codex OAuth, OpenRouter, or a custom endpoint."
                    .to_string(),
            );
        }
    }

    persist_hermes_setup_files(state.inner(), &payload)?;
    {
        let mut process_guard = get_hermes_gateway_process().lock();
        if let Some(child) = process_guard.as_mut() {
            let _ = child.kill();
        }
        *process_guard = None;
    }
    *get_hermes_gateway_error_store().lock() = None;
    sync_hermes_bridge_files(state.inner()).await
}

#[tauri::command]
pub async fn start_hermes_gateway(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    ensure_hermes_gateway_ready(state.inner(), 12_000).await
}

#[tauri::command]
pub async fn stop_hermes_gateway(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    // Drop guards before the .await — parking_lot MutexGuard is !Send.
    {
        let mut process_guard = get_hermes_gateway_process().lock();
        if let Some(child) = process_guard.as_mut() {
            let _ = child.kill();
        }
        *process_guard = None;
        *get_hermes_gateway_error_store().lock() = None;
    }
    build_hermes_bridge_status(state.inner()).await
}

/// Direct chat with an Ollama model — no Hermes CLI required.
/// Works with any OpenAI-compatible base URL (Ollama's /v1 endpoint).
#[tauri::command]
pub async fn send_direct_chat(
    state: State<'_, Arc<AppState>>,
    messages: Vec<serde_json::Value>,
    input: String,
) -> Result<String, String> {
    let _ = sync_hermes_bridge_files(state.inner()).await?;
    let setup = read_hermes_setup_record(state.inner())
        .ok_or_else(|| "Configure a provider in FNDR's Agent page first.".to_string())?;

    let base_url = if setup.provider_kind == "ollama" {
        setup
            .base_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| OLLAMA_BASE_URL.to_string())
    } else {
        return Err(
            "Direct chat is only available for Ollama. Use the Hermes gateway for other providers."
                .to_string(),
        );
    };

    // Build a system message with FNDR context
    let context_path = hermes_context_path(state.inner());
    let system_content = if context_path.exists() {
        std::fs::read_to_string(&context_path)
            .unwrap_or_else(|_| "You are a helpful assistant embedded in FNDR.".to_string())
    } else {
        "You are a helpful assistant embedded in FNDR, a privacy-first local memory app. Help the user with recall, planning, drafting, and research using context they provide.".to_string()
    };

    let mut all_messages: Vec<serde_json::Value> =
        vec![serde_json::json!({ "role": "system", "content": system_content })];
    all_messages.extend(messages);
    all_messages.push(serde_json::json!({ "role": "user", "content": input.trim() }));

    let request = serde_json::json!({
        "model": setup.model_name,
        "messages": all_messages,
        "stream": false,
    });

    let client = llm_http_client().map_err(|e| format!("HTTP client: {e}"))?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let (status_code, json) = post_json_response(&client, &url, &request, None)
        .await
        .map_err(|e| format!("Could not reach Ollama at {base_url}: {e}"))?;

    if !status_code.is_success() {
        return Err(json
            .get("error")
            .and_then(|v| v.get("message").or(Some(v)))
            .and_then(|v| v.as_str())
            .unwrap_or("Ollama request failed.")
            .to_string());
    }

    let content = json
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(content)
}

#[tauri::command]
pub async fn send_hermes_message(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    input: String,
) -> Result<HermesChatReply, String> {
    let status = ensure_hermes_gateway_ready(state.inner(), 12_000).await?;

    let api_key = read_hermes_api_key(state.inner())
        .ok_or_else(|| "FNDR could not read the Hermes API server key.".to_string())?;
    let input = input.trim();
    if input.is_empty() {
        return Err("Message cannot be empty.".to_string());
    }

    let instructions = "You are the native FNDR agent experience, powered by Hermes under the hood. Use FNDR's context files and private snapshot to help with planning, recall, drafting, research, and safe computer-use support. Ask before destructive actions, external messages, purchases, or credential changes.";
    let request_body = serde_json::json!({
        "model": "hermes-agent",
        "input": input,
        "conversation": conversation_id,
        "store": true,
        "instructions": instructions
    });

    let client = llm_http_client().map_err(|e| format!("HTTP client: {e}"))?;
    let url = format!("{}/v1/responses", status.api_url.trim_end_matches('/'));
    let (status_code, json) =
        post_json_response(&client, &url, &request_body, Some(api_key.as_str()))
            .await
            .map_err(|e| format!("Failed to reach the Hermes API server: {e}"))?;

    if !status_code.is_success() {
        return Err(json
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_str())
            .or_else(|| json.get("detail").and_then(|value| value.as_str()))
            .unwrap_or("Hermes API request failed.")
            .to_string());
    }

    let response_id = json
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    let content = json
        .get("output")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(|value| value.as_str()) == Some("message") {
                        item.get("content")
                            .and_then(|value| value.as_array())
                            .map(|parts| {
                                parts
                                    .iter()
                                    .filter_map(|part| {
                                        part.get("text").and_then(|value| value.as_str())
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                    } else {
                        None
                    }
                })
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| {
            "Hermes completed the turn, but no assistant text was returned.".to_string()
        });

    Ok(HermesChatReply {
        response_id,
        conversation_id,
        content,
    })
}

/// Start the agent to execute a task
#[tauri::command]
pub async fn start_agent_task(
    task_title: String,
    context_urls: Option<Vec<String>>,
    context_notes: Option<Vec<String>>,
) -> Result<AgentStatus, String> {
    let mut process_guard = get_agent_process().lock();

    // Kill existing process if any
    if let Some(ref mut child) = *process_guard {
        let _ = child.kill();
    }

    // Find the agent runner script
    let sidecar_path = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("No parent dir")?
        .join("../Resources/sidecars/agent_runner.py");

    let script_path = if sidecar_path.exists() {
        sidecar_path
    } else {
        // Fallback for development
        std::path::PathBuf::from("src-tauri/sidecars/agent_runner.py")
    };

    // Find the python executable in the virtual environment
    let venv_python = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("No parent dir")?
        .join("../.venv/bin/python3");

    let python_exe = if venv_python.exists() {
        venv_python
    } else {
        // Fallback for development (assuming project root relative to execution)
        std::path::PathBuf::from(".venv/bin/python3")
    };

    let mut task_prompt = task_title.clone();
    if let Some(urls) = context_urls {
        if !urls.is_empty() {
            let url_context = urls
                .into_iter()
                .take(6)
                .map(|u| format!("- {}", u))
                .collect::<Vec<_>>()
                .join("\n");
            task_prompt.push_str("\n\nGround-truth URLs from memory graph:\n");
            task_prompt.push_str(&url_context);
        }
    }
    if let Some(notes) = context_notes {
        if !notes.is_empty() {
            task_prompt.push_str("\n\nMemory graph notes:\n");
            task_prompt.push_str(
                &notes
                    .into_iter()
                    .take(5)
                    .map(|n| format!("- {}", n))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }

    // Start the agent process
    let child = Command::new(python_exe)
        .arg(&script_path)
        .arg(&task_prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start agent: {}", e))?;

    *process_guard = Some(child);

    // Update status
    let mut status = get_agent_status_store().lock();
    *status = AgentStatus {
        is_running: true,
        task_title: Some(task_title),
        last_message: Some("Agent started...".to_string()),
        status: "running".to_string(),
    };

    Ok(status.clone())
}

/// Get current agent status
#[tauri::command]
pub async fn get_agent_status() -> Result<AgentStatus, String> {
    let mut process_guard = get_agent_process().lock();
    let mut status = get_agent_status_store().lock();

    if let Some(ref mut child) = *process_guard {
        // Check if process is still running
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                status.is_running = false;
                status.status = if exit_status.success() {
                    "completed".to_string()
                } else {
                    "error".to_string()
                };
            }
            Ok(None) => {
                // Still running, try to read output
                status.is_running = true;
            }
            Err(e) => {
                status.is_running = false;
                status.status = "error".to_string();
                status.last_message = Some(format!("Error: {}", e));
            }
        }
    }

    Ok(status.clone())
}

/// Stop the agent
#[tauri::command]
pub async fn stop_agent() -> Result<AgentStatus, String> {
    let mut process_guard = get_agent_process().lock();

    if let Some(ref mut child) = *process_guard {
        let _ = child.kill();
    }
    *process_guard = None;

    let mut status = get_agent_status_store().lock();
    *status = AgentStatus {
        is_running: false,
        task_title: status.task_title.clone(),
        last_message: Some("Agent stopped by user".to_string()),
        status: "idle".to_string(),
    };

    Ok(status.clone())
}

/// Generate a smart daily briefing paragraph using the local LLM.
/// `mode`: "morning" (actionable: what to focus on) or "evening" (recap + tomorrow).
/// Defaults to time-of-day detection when None.
#[tauri::command]
pub async fn generate_daily_briefing(
    state: State<'_, Arc<AppState>>,
    mode: Option<String>,
) -> Result<String, String> {
    // Detect mode from local hour if not specified
    let resolved_mode = mode.unwrap_or_else(|| {
        let hour = chrono::Local::now().hour();
        if hour >= 17 {
            "evening".to_string()
        } else {
            "morning".to_string()
        }
    });

    // Fetch the most recent cards (today + a few recent ones for context)
    let limit = 10usize;
    let results = state
        .store
        .list_recent_results(limit, None)
        .await
        .map_err(|e| e.to_string())?;

    let mut cards: Vec<MemoryCard> = strip_internal_fndr_results(results)
        .into_iter()
        .map(memory_card_from_result)
        .collect();
    refine_memory_card_titles(&mut cards);

    if cards.is_empty() {
        return Ok(String::new());
    }

    // Build compact per-card lines for the LLM context
    let card_lines: Vec<String> = cards
        .iter()
        .take(8)
        .map(|c| format!("- [{}] {}: {}", c.app_name, c.title, c.summary))
        .collect();

    // Grab inference engine
    let engine = {
        let guard = state.inference.read();
        guard.as_ref().map(Arc::clone)
    };

    let Some(engine) = engine else {
        return Ok(String::new());
    };

    let briefing = engine
        .generate_daily_briefing(&card_lines, &resolved_mode)
        .await;
    Ok(briefing)
}

#[tauri::command]
pub fn get_fun_greeting(name: Option<String>) -> Result<String, String> {
    use rand::prelude::IndexedRandom;
    let base_name = name.unwrap_or_else(|| "there".to_string());

    let hour = chrono::Local::now().hour();

    let prefix = if (4..12).contains(&hour) {
        "Good Morning"
    } else if (12..16).contains(&hour) {
        "Good Afternoon"
    } else if (16..20).contains(&hour) {
        "Good Evening"
    } else {
        "Good Night"
    };

    let fun_suffixes = ["Ready to conquer the day?",
        "Let's dive into your memories.",
        "What are we exploring today?",
        "Time to make some magic happen.",
        "Welcome back to the matrix.",
        "Let's get productive.",
        "System fully operational."];

    let mut rng = rand::rng();
    let random_suffix = fun_suffixes.choose(&mut rng).unwrap_or(&"");

    Ok(format!("{}, {}! {}", prefix, base_name, random_suffix))
}

#[tauri::command]
pub async fn quick_setup_ollama(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    let (installed, reachable, models) = detect_ollama_state().await;
    if !installed {
        return Err("Ollama is not installed on this Mac.".to_string());
    }
    if !reachable {
        return Err(
            "FNDR could not reach Ollama. Make sure Ollama is running (`ollama serve`)."
                .to_string(),
        );
    }
    if models.is_empty() {
        return Err(
            "No Ollama models found. Pull a model first: `ollama pull llama3.2` or `ollama pull qwen2.5-coder`.".to_string(),
        );
    }

    let best_model = models
        .iter()
        .find(|m| {
            let l = m.to_lowercase();
            l.contains("llama3")
                || l.contains("llama-3")
                || l.contains("qwen2.5")
                || l.contains("mistral")
                || l.contains("gemma")
        })
        .or_else(|| models.first())
        .cloned()
        .unwrap_or_else(|| models[0].clone());

    let payload = HermesSetupPayload {
        provider_kind: "ollama".to_string(),
        model_name: best_model,
        api_key: None,
        base_url: Some(OLLAMA_BASE_URL.to_string()),
    };

    persist_hermes_setup_files(state.inner(), &payload)?;
    {
        let mut process_guard = get_hermes_gateway_process().lock();
        if let Some(child) = process_guard.as_mut() {
            let _ = child.kill();
        }
        *process_guard = None;
    }
    *get_hermes_gateway_error_store().lock() = None;
    sync_hermes_bridge_files(state.inner()).await
}
