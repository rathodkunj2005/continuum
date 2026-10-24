//! EmbeddingGemma 2 through `fndr-eg2-helper`, a child process with its own
//! pinned llama.cpp (`scripts/eg2/build-llama.sh`). Qwen keeps the llama.cpp in
//! `llama-cpp-2`; the two never share a binary, and a Metal fault in the
//! embedder cannot take the app down. One JSON line per request on stdin, one
//! per reply on stdout.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use serde_json::{json, Value};

const HELPER_NAME: &str = "fndr-eg2-helper";

pub(super) struct GemmaHelper {
    helper: PathBuf,
    model: PathBuf,
    dimension: usize,
    running: Mutex<Option<Running>>,
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl GemmaHelper {
    /// Starts the helper and waits for its ready line, which reports the model's
    /// vector size; a size other than `dimension` is an error.
    pub(super) fn start(model: &Path, dimension: usize) -> Result<Self, String> {
        let helper = resolve_helper().ok_or_else(|| {
            format!(
                "{HELPER_NAME} not found. Build it with scripts/eg2/build-helper.sh or set FNDR_EG2_HELPER."
            )
        })?;
        let running = spawn(&helper, model, dimension)?;
        Ok(Self {
            helper,
            model: model.to_path_buf(),
            dimension,
            running: Mutex::new(Some(running)),
        })
    }

    /// Embeds already prompted texts. A helper that died is restarted once.
    pub(super) fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let mut guard = self
            .running
            .lock()
            .map_err(|e| format!("{HELPER_NAME} lock poisoned: {e}"))?;
        for attempt in 0..2 {
            if guard.is_none() {
                *guard = Some(spawn(&self.helper, &self.model, self.dimension)?);
            }
            let running = guard.as_mut().expect("spawned above");
            match running.request(texts) {
                Ok(vectors) => return Ok(vectors),
                Err(Failure::Request(error)) => return Err(error),
                Err(Failure::Process(error)) => {
                    *guard = None;
                    if attempt == 1 {
                        return Err(error);
                    }
                    tracing::warn!("{HELPER_NAME} failed, restarting: {error}");
                }
            }
        }
        unreachable!("the second attempt returns")
    }
}

enum Failure {
    /// The helper answered that it could not embed this input.
    Request(String),
    /// The helper is gone or spoke nonsense.
    Process(String),
}

impl Running {
    fn request(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, Failure> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({ "id": id, "op": "embed_texts", "texts": texts });
        writeln!(self.stdin, "{line}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| Failure::Process(format!("{HELPER_NAME} write failed: {e}")))?;
        let mut reply = String::new();
        let read = self
            .stdout
            .read_line(&mut reply)
            .map_err(|e| Failure::Process(format!("{HELPER_NAME} read failed: {e}")))?;
        if read == 0 {
            return Err(Failure::Process(format!("{HELPER_NAME} exited")));
        }
        let reply: Value = serde_json::from_str(&reply)
            .map_err(|e| Failure::Process(format!("{HELPER_NAME} sent bad JSON: {e}")))?;
        if reply.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(Failure::Process(format!(
                "{HELPER_NAME} answered a different request"
            )));
        }
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            let error = reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(Failure::Request(format!("{HELPER_NAME}: {error}")));
        }
        let vectors = reply
            .get("vectors")
            .and_then(Value::as_array)
            .ok_or_else(|| Failure::Process(format!("{HELPER_NAME} sent no vectors")))?
            .iter()
            .map(|vector| {
                vector
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                            .collect::<Vec<_>>()
                    })
                    .ok_or_else(|| Failure::Process(format!("{HELPER_NAME} sent a bad vector")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if vectors.len() != texts.len() {
            return Err(Failure::Process(format!(
                "{HELPER_NAME} returned {} vectors for {} texts",
                vectors.len(),
                texts.len()
            )));
        }
        Ok(vectors)
    }
}

fn spawn(helper: &Path, model: &Path, dimension: usize) -> Result<Running, String> {
    let mut child = Command::new(helper)
        .arg("--text")
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", helper.display()))?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return Err(format!("{HELPER_NAME} has no pipes"));
    };
    // Owned from here on, so every error below kills the child.
    let mut running = Running {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        next_id: 0,
    };
    let mut line = String::new();
    running
        .stdout
        .read_line(&mut line)
        .map_err(|e| format!("{HELPER_NAME} startup read failed: {e}"))?;
    let ready: Value = serde_json::from_str(&line)
        .map_err(|_| format!("{HELPER_NAME} did not report ready: {line:?}"))?;
    if ready.get("ready").and_then(Value::as_bool) != Some(true) {
        let error = ready
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(format!("{HELPER_NAME} could not load the model: {error}"));
    }
    let actual = ready.get("dimension").and_then(Value::as_u64).unwrap_or(0) as usize;
    if actual != dimension {
        return Err(format!(
            "{HELPER_NAME} reports {actual}-d vectors but the FNDR contract expects {dimension}-d"
        ));
    }
    Ok(running)
}

/// Where the helper may be, best first: the override, beside the executable (Tauri
/// drops the target suffix from an external binary), then a development build.
fn resolve_helper() -> Option<PathBuf> {
    helper_candidates(
        std::env::var_os("FNDR_EG2_HELPER").map(PathBuf::from),
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        Path::new(env!("CARGO_MANIFEST_DIR")),
        "aarch64-apple-darwin",
    )
    .into_iter()
    .find(|candidate| candidate.is_file())
}

fn helper_candidates(
    override_path: Option<PathBuf>,
    executable_dir: Option<PathBuf>,
    manifest_dir: &Path,
    target: &str,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.extend(override_path);
    if let Some(dir) = executable_dir {
        candidates.push(dir.join(HELPER_NAME));
    }
    candidates.push(
        manifest_dir
            .join("binaries")
            .join(format!("{HELPER_NAME}-{target}")),
    );
    candidates.push(
        manifest_dir
            .join("helpers/fndr-eg2/target/release")
            .join(HELPER_NAME),
    );
    candidates
}
