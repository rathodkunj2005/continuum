use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptionHint {
    Default,
    VoiceCommand,
}

impl TranscriptionHint {
    fn env_value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::VoiceCommand => "voice-command",
        }
    }

    fn sidecar_flag(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::VoiceCommand => Some("--voice-command"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SpeechModelKind {
    WhisperBaseEn,
    Orpheus3B,
}

#[derive(Debug, Clone, Copy)]
struct SpeechModelDefinition {
    id: &'static str,
    folder: &'static str,
    filename: &'static str,
    download_url: &'static str,
}

const WHISPER_MODEL: SpeechModelDefinition = SpeechModelDefinition {
    id: "whisper-small-ggml",
    folder: "whisper-small",
    filename: "ggml-small.bin",
    download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
};

const ORPHEUS_MODEL: SpeechModelDefinition = SpeechModelDefinition {
    id: "orpheus-3b-0.1-ft-q4-k-m",
    folder: "orpheus-3b-0.1-ft",
    filename: "orpheus-3b-0.1-ft-Q4_K_M.gguf",
    download_url:
        "https://huggingface.co/unsloth/orpheus-3b-0.1-ft-GGUF/resolve/main/orpheus-3b-0.1-ft-Q4_K_M.gguf",
};

static SPEECH_DOWNLOAD_LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
static SPEECH_BOOTSTRAP_LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();

fn download_lock() -> &'static AsyncMutex<()> {
    SPEECH_DOWNLOAD_LOCK.get_or_init(|| AsyncMutex::new(()))
}

fn bootstrap_lock() -> &'static AsyncMutex<()> {
    SPEECH_BOOTSTRAP_LOCK.get_or_init(|| AsyncMutex::new(()))
}

fn definition(kind: SpeechModelKind) -> &'static SpeechModelDefinition {
    match kind {
        SpeechModelKind::WhisperBaseEn => &WHISPER_MODEL,
        SpeechModelKind::Orpheus3B => &ORPHEUS_MODEL,
    }
}

pub fn speech_models_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("speech_models")
}

fn model_dir(app_data_dir: &Path, kind: SpeechModelKind) -> PathBuf {
    speech_models_dir(app_data_dir).join(definition(kind).folder)
}

pub fn model_path(app_data_dir: &Path, kind: SpeechModelKind) -> PathBuf {
    model_dir(app_data_dir, kind).join(definition(kind).filename)
}

fn partial_model_path(app_data_dir: &Path, kind: SpeechModelKind) -> PathBuf {
    model_dir(app_data_dir, kind).join(format!("{}.partial", definition(kind).filename))
}

pub fn voice_cache_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("voice")
}

pub fn make_voice_input_path(app_data_dir: &Path, extension: &str) -> PathBuf {
    voice_cache_dir(app_data_dir).join("input").join(format!(
        "voice-input-{}.{}",
        Uuid::new_v4(),
        extension
    ))
}

pub fn make_tts_output_path(app_data_dir: &Path) -> PathBuf {
    voice_cache_dir(app_data_dir)
        .join("tts")
        .join(format!("speech-{}.wav", Uuid::new_v4()))
}

pub async fn ensure_model_downloaded(
    app_data_dir: &Path,
    kind: SpeechModelKind,
) -> Result<PathBuf, String> {
    let _guard = download_lock().lock().await;
    let final_path = model_path(app_data_dir, kind);
    if final_path.exists() {
        return Ok(final_path);
    }

    let model_dir = model_dir(app_data_dir, kind);
    std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;
    let partial_path = partial_model_path(app_data_dir, kind);
    let definition = definition(kind);

    tracing::info!(
        "Downloading speech model {} to {:?}",
        definition.id,
        final_path
    );

    download_with_resume(
        definition.download_url,
        &partial_path,
        &final_path,
        definition.id,
    )
    .await?;

    Ok(final_path)
}

async fn download_with_resume(
    url: &str,
    partial_path: &Path,
    final_path: &Path,
    label: &str,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .user_agent("Continuum/1.0")
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client for {}: {}", label, e))?;

    let resume_from = partial_path.metadata().map(|meta| meta.len()).unwrap_or(0);
    let mut request = client.get(url);
    if resume_from > 0 {
        request = request.header("Range", format!("bytes={}-", resume_from));
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("Failed downloading {}: {}", label, e))?;
    let status_code = response.status().as_u16();
    if !response.status().is_success() && status_code != 206 {
        let body_preview = response.text().await.unwrap_or_default().replace('\n', " ");
        return Err(format!(
            "Failed downloading {}: {} {}",
            label,
            status_code,
            body_preview.chars().take(200).collect::<String>()
        ));
    }

    let resume_from = if resume_from > 0 && status_code == 200 {
        let _ = tokio::fs::remove_file(partial_path).await;
        0
    } else {
        resume_from
    };

    let raw_file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resume_from > 0)
        .truncate(resume_from == 0)
        .open(partial_path)
        .await
        .map_err(|e| format!("Failed opening partial file for {}: {}", label, e))?;
    let mut file = tokio::io::BufWriter::new(raw_file);

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream failed for {}: {}", label, e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed writing {} partial: {}", label, e))?;
    }

    file.flush()
        .await
        .map_err(|e| format!("Failed flushing {} partial: {}", label, e))?;
    drop(file);

    tokio::fs::rename(partial_path, final_path)
        .await
        .map_err(|e| format!("Failed finalizing {}: {}", label, e))?;

    Ok(())
}

pub fn resolve_sidecar(script_name: &str) -> Option<PathBuf> {
    let packaged = std::env::current_exe().ok().and_then(|exe| {
        exe.parent()
            .map(|dir| dir.join("../Resources/sidecar").join(script_name))
    });
    if let Some(path) = packaged {
        if path.exists() {
            return Some(path);
        }
    }

    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("sidecar")
        .join(script_name);
    dev.exists().then_some(dev)
}

pub fn python_for_sidecar() -> Option<PathBuf> {
    let venv_dir = speech_venv_dir()?;
    if cfg!(target_os = "windows") {
        let candidate = venv_dir.join("Scripts").join("python");
        if candidate.exists() {
            return Some(candidate);
        }
    } else {
        let candidate = venv_dir.join("bin").join("python3");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn pip_for_venv(venv_dir: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        venv_dir.join("Scripts").join("pip")
    } else {
        venv_dir.join("bin").join("pip")
    }
}

fn speech_venv_dir() -> Option<PathBuf> {
    dirs::document_dir().map(|root| root.join("Continuum Speech").join("venv"))
}

/// Probe for a usable Python 3 interpreter, preferring versions ≤ 3.13
/// (whisper-cpp-python has no 3.14+ wheels). Checks Homebrew paths first
/// because macOS Dock-launched apps don't inherit the user's shell PATH.
fn find_python3() -> Option<PathBuf> {
    // Ordered list of (binary name, full path to try)
    let candidates: &[&str] = &[
        "/opt/homebrew/bin/python3.13",
        "/opt/homebrew/bin/python3.12",
        "/opt/homebrew/bin/python3.11",
        "/opt/homebrew/bin/python3.10",
        "/usr/local/bin/python3.13",
        "/usr/local/bin/python3.12",
        "/usr/local/bin/python3.11",
        "/usr/local/bin/python3.10",
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
        "/usr/bin/python3",
        "python3",
    ];
    for &path in candidates {
        let candidate = PathBuf::from(path);
        if Command::new(&candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(candidate);
        }
    }
    None
}

/// Try to run whisper via the `whisper-cli` binary (from `brew install whisper-cpp`).
/// Returns the transcript on success, or None if the binary is unavailable / fails.
fn try_whisper_cli_binary(model_path: &Path, audio_path: &Path) -> Option<String> {
    // whisper-cpp installs as `whisper-cli` on Homebrew
    let cli_paths: &[&str] = &[
        "/opt/homebrew/bin/whisper-cli",
        "/usr/local/bin/whisper-cli",
        "whisper-cli",
    ];
    for &cli in cli_paths {
        let output = Command::new(cli)
            .args([
                "-m",
                &model_path.to_string_lossy(),
                "-f",
                &audio_path.to_string_lossy(),
                "-l",
                "en",
                "--no-timestamps",
                "-nt",
            ])
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn python_imports_ok(python: &Path, imports: &str) -> bool {
    Command::new(python)
        .args(["-c", imports])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn whisper_imports_ok(python: &Path) -> bool {
    python_imports_ok(python, "import whisper_cpp_python")
}

fn orpheus_imports_ok(python: &Path) -> bool {
    python_imports_ok(
        python,
        "import llama_cpp, numpy, huggingface_hub, onnxruntime",
    )
}

fn llama_cpp_extra_index() -> &'static str {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "https://abetlen.github.io/llama-cpp-python/whl/metal"
    } else {
        "https://abetlen.github.io/llama-cpp-python/whl/cpu"
    }
}

fn ensure_venv_ready() -> Result<PathBuf, String> {
    let Some(venv_dir) = speech_venv_dir() else {
        return Err("Could not determine Documents directory for Continuum Speech".to_string());
    };

    let python3 = find_python3().ok_or_else(|| {
        "python3 (≤3.13) is required for speech features. Install it with: brew install python@3.13".to_string()
    })?;

    if !venv_dir.exists() {
        if let Some(parent) = venv_dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let status = Command::new(&python3)
            .args(["-m", "venv", &venv_dir.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed creating Continuum Speech venv: {}", e))?;
        if !status.success() {
            return Err("Failed creating Continuum Speech venv".to_string());
        }
    }

    let pip = pip_for_venv(&venv_dir);
    if !pip.exists() {
        return Err(format!("Pip binary missing at {:?}", pip));
    }

    let upgrade = Command::new(&pip)
        .args(["install", "--upgrade", "pip", "setuptools", "wheel"])
        .status()
        .map_err(|e| format!("Failed upgrading speech pip: {}", e))?;
    if !upgrade.success() {
        return Err("Failed upgrading speech pip toolchain".to_string());
    }

    Ok(venv_dir)
}

fn ensure_whisper_backend_blocking() -> Result<(), String> {
    if let Some(python) = python_for_sidecar() {
        if whisper_imports_ok(&python) {
            return Ok(());
        }
    }

    let venv_dir = ensure_venv_ready()?;
    let pip = pip_for_venv(&venv_dir);

    let whisper = Command::new(&pip)
        .env(
            "CMAKE_ARGS",
            "-DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DWHISPER_METAL=1",
        )
        .args(["install", "whisper-cpp-python"])
        .status()
        .map_err(|e| format!("Failed installing whisper-cpp-python: {}", e))?;
    if !whisper.success() {
        return Err("Failed installing whisper-cpp-python".to_string());
    }

    // Workaround: whisper-cpp-python on MacOS expects .so but often builds .dylib
    let Some(python) = python_for_sidecar() else {
        return Err("Speech venv was created, but python was not found".to_string());
    };

    let patch_script = "
import sys, os
site_packages = [p for p in sys.path if 'site-packages' in p]
if site_packages:
    dylib = os.path.join(site_packages[0], 'whisper_cpp_python', 'libwhisper.dylib')
    so = os.path.join(site_packages[0], 'whisper_cpp_python', 'libwhisper.so')
    if os.path.exists(dylib) and not os.path.exists(so):
        import shutil
        shutil.copy(dylib, so)
";
    let _ = Command::new(&python).args(["-c", patch_script]).status();

    if !whisper_imports_ok(&python) {
        return Err("Whisper backend is still unavailable after install".to_string());
    }

    Ok(())
}

fn ensure_orpheus_backend_blocking() -> Result<(), String> {
    if let Some(python) = python_for_sidecar() {
        if orpheus_imports_ok(&python) {
            return Ok(());
        }
    }

    let venv_dir = ensure_venv_ready()?;
    let pip = pip_for_venv(&venv_dir);

    let llama = Command::new(&pip)
        .args([
            "install",
            "llama-cpp-python",
            "--extra-index-url",
            llama_cpp_extra_index(),
        ])
        .status()
        .map_err(|e| format!("Failed installing llama-cpp-python: {}", e))?;
    if !llama.success() {
        return Err("Failed installing llama-cpp-python".to_string());
    }

    let deps = Command::new(&pip)
        .args(["install", "huggingface_hub", "numpy", "onnxruntime"])
        .status()
        .map_err(|e| format!("Failed installing Orpheus dependencies: {}", e))?;
    if !deps.success() {
        return Err("Failed installing Orpheus dependencies".to_string());
    }

    let Some(python) = python_for_sidecar() else {
        return Err("Speech venv was created, but python was not found".to_string());
    };
    if !orpheus_imports_ok(&python) {
        return Err("Orpheus backend dependencies are still unavailable after install".to_string());
    }

    Ok(())
}

pub async fn ensure_whisper_backend() -> Result<(), String> {
    let _guard = bootstrap_lock().lock().await;
    tokio::task::spawn_blocking(ensure_whisper_backend_blocking)
        .await
        .map_err(|e| e.to_string())?
}

pub async fn ensure_orpheus_backend() -> Result<(), String> {
    let _guard = bootstrap_lock().lock().await;
    tokio::task::spawn_blocking(ensure_orpheus_backend_blocking)
        .await
        .map_err(|e| e.to_string())?
}

fn extension_from_mime(mime_type: Option<&str>) -> &'static str {
    let mime = mime_type.unwrap_or_default().to_ascii_lowercase();
    if mime.contains("wav") {
        "wav"
    } else if mime.contains("webm") {
        "webm"
    } else if mime.contains("ogg") {
        "ogg"
    } else if mime.contains("mp4") || mime.contains("m4a") || mime.contains("aac") {
        "m4a"
    } else if mime.contains("mpeg") || mime.contains("mp3") {
        "mp3"
    } else {
        "wav"
    }
}

fn normalize_transcript_text(raw: &str) -> String {
    let mut cleaned_tokens = Vec::new();

    for token in raw.split_whitespace() {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }

        let marker = trimmed
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '[' | ']' | '(' | ')' | '{' | '}' | '<' | '>' | '"' | '\''
                )
            })
            .to_ascii_lowercase();

        if matches!(
            marker.as_str(),
            "music"
                | "applause"
                | "laughing"
                | "laughter"
                | "noise"
                | "silence"
                | "inaudible"
                | "background"
                | "ambient"
        ) {
            continue;
        }

        cleaned_tokens.push(trimmed.to_string());
    }

    cleaned_tokens.join(" ").trim().to_string()
}

pub async fn transcribe_audio_bytes(
    app_data_dir: &Path,
    audio_bytes: &[u8],
    mime_type: Option<&str>,
) -> Result<String, String> {
    if audio_bytes.is_empty() {
        return Err("Cannot transcribe empty audio input".to_string());
    }

    let extension = extension_from_mime(mime_type);
    let input_path = make_voice_input_path(app_data_dir, extension);
    if let Some(parent) = input_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create voice input cache directory: {}", e))?;
    }

    tokio::fs::write(&input_path, audio_bytes)
        .await
        .map_err(|e| format!("Failed to persist voice input: {}", e))?;

    let result =
        transcribe_audio_file_with_hint(app_data_dir, &input_path, TranscriptionHint::VoiceCommand)
            .await;
    let _ = tokio::fs::remove_file(&input_path).await;
    result
}

pub async fn transcribe_audio_file(
    app_data_dir: &Path,
    audio_path: &Path,
) -> Result<String, String> {
    transcribe_audio_file_with_hint(app_data_dir, audio_path, TranscriptionHint::Default).await
}

pub async fn transcribe_audio_file_voice_command(
    app_data_dir: &Path,
    audio_path: &Path,
) -> Result<String, String> {
    transcribe_audio_file_with_hint(app_data_dir, audio_path, TranscriptionHint::VoiceCommand).await
}

async fn transcribe_audio_file_with_hint(
    app_data_dir: &Path,
    audio_path: &Path,
    hint: TranscriptionHint,
) -> Result<String, String> {
    let model_path = ensure_model_downloaded(app_data_dir, SpeechModelKind::WhisperBaseEn).await?;

    // Fast path: use the whisper-cli binary (brew install whisper-cpp) — no Python needed.
    {
        let model = model_path.clone();
        let audio = audio_path.to_path_buf();
        if let Some(text) =
            tokio::task::spawn_blocking(move || try_whisper_cli_binary(&model, &audio))
                .await
                .ok()
                .flatten()
        {
            let cleaned = normalize_transcript_text(&text);
            if !cleaned.is_empty() {
                return Ok(cleaned);
            }
        }
    }

    ensure_whisper_backend().await?;

    if let Ok(custom_cmd) = std::env::var("CONTINUUM_WHISPER_GGUF_COMMAND") {
        let audio = audio_path.to_path_buf();
        let model = model_path.clone();
        let hint_value = hint.env_value().to_string();
        let output = tokio::task::spawn_blocking(move || {
            Command::new("sh")
                .arg("-c")
                .arg(custom_cmd)
                .env("CONTINUUM_AUDIO_PATH", audio.to_string_lossy().to_string())
                .env(
                    "CONTINUUM_WHISPER_MODEL_PATH",
                    model.to_string_lossy().to_string(),
                )
                .env("CONTINUUM_TRANSCRIBE_HINT", hint_value)
                .output()
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("CONTINUUM_WHISPER_GGUF_COMMAND failed to start: {}", e))?;

        if output.status.success() {
            let text = normalize_transcript_text(&String::from_utf8_lossy(&output.stdout));
            if !text.is_empty() {
                return Ok(text);
            }
        }
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let sidecar = resolve_sidecar("whisper_gguf_runner.py")
        .ok_or_else(|| "Could not locate whisper_gguf_runner.py".to_string())?;
    let python = python_for_sidecar()
        .or_else(find_python3)
        .ok_or_else(|| "No usable python3 found for Whisper transcription. Install with: brew install python@3.13".to_string())?;
    let audio = audio_path.to_path_buf();
    let model = model_path.clone();
    let sidecar_flag = hint.sidecar_flag().map(str::to_string);
    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(python);
        cmd.arg(sidecar)
            .arg(model.to_string_lossy().to_string())
            .arg(audio.to_string_lossy().to_string());
        if let Some(flag) = sidecar_flag {
            cmd.arg(flag);
        }
        cmd.output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Failed launching Whisper GGUF runner: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let text = normalize_transcript_text(&String::from_utf8_lossy(&output.stdout));
    if text.is_empty() {
        return Err("Whisper GGUF runner returned empty transcript".to_string());
    }
    Ok(text)
}

pub async fn synthesize_speech(
    app_data_dir: &Path,
    text: &str,
    voice_id: Option<&str>,
) -> Result<PathBuf, String> {
    if text.trim().is_empty() {
        return Err("Cannot synthesize empty text".to_string());
    }

    let model_path = ensure_model_downloaded(app_data_dir, SpeechModelKind::Orpheus3B).await?;
    ensure_orpheus_backend().await?;

    let output_path = make_tts_output_path(app_data_dir);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let voice = voice_id.unwrap_or("tara").to_string();

    if let Ok(custom_cmd) = std::env::var("CONTINUUM_ORPHEUS_COMMAND") {
        let model = model_path.clone();
        let output = output_path.clone();
        let text = text.to_string();
        let voice_for_cmd = voice.clone();
        let custom_output = tokio::task::spawn_blocking(move || {
            Command::new("sh")
                .arg("-c")
                .arg(custom_cmd)
                .env("CONTINUUM_TTS_MODEL_PATH", model.to_string_lossy().to_string())
                .env("CONTINUUM_TTS_OUTPUT_PATH", output.to_string_lossy().to_string())
                .env("CONTINUUM_TTS_TEXT", text)
                .env("CONTINUUM_TTS_VOICE", voice_for_cmd)
                .output()
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("CONTINUUM_ORPHEUS_COMMAND failed to start: {}", e))?;

        if !custom_output.status.success() {
            return Err(String::from_utf8_lossy(&custom_output.stderr)
                .trim()
                .to_string());
        }
        return Ok(output_path);
    }

    let sidecar = resolve_sidecar("orpheus_tts_runner.py")
        .ok_or_else(|| "Could not locate orpheus_tts_runner.py".to_string())?;
    let python = python_for_sidecar().unwrap_or_else(|| PathBuf::from("python3"));
    let model = model_path.clone();
    let output = output_path.clone();
    let text = text.to_string();

    let runner_output = tokio::task::spawn_blocking(move || {
        Command::new(python)
            .arg(sidecar)
            .arg(model.to_string_lossy().to_string())
            .arg(output.to_string_lossy().to_string())
            .arg(voice)
            .arg(text)
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Failed launching Orpheus TTS runner: {}", e))?;

    if !runner_output.status.success() {
        return Err(String::from_utf8_lossy(&runner_output.stderr)
            .trim()
            .to_string());
    }
    if !output_path.exists() {
        return Err("Orpheus TTS runner did not produce an output WAV".to_string());
    }

    Ok(output_path)
}
