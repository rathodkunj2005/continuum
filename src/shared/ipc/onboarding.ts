import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ── Onboarding state ──────────────────────────────────────────────────────

export type OnboardingStep =
    | "welcome"
    | "biometrics"
    | "privacy_promise"
    | "permissions"
    | "model_download"
    | "indexing_started"
    | "complete";

export interface OnboardingState {
    step: OnboardingStep;
    biometric_enabled: boolean;
    screen_permission: boolean;
    accessibility_permission: boolean;
    model_downloaded: boolean;
    model_id: string | null;
    display_name: string | null;
}

export async function getOnboardingState(): Promise<OnboardingState> {
    return invoke<OnboardingState>("get_onboarding_state");
}

export async function saveOnboardingState(state: OnboardingState): Promise<void> {
    return invoke("save_onboarding_state", { state });
}

// ── Biometrics ────────────────────────────────────────────────────────────

export async function requestBiometricAuth(reason: string): Promise<boolean> {
    return invoke<boolean>("request_biometric_auth", { reason });
}

// ── Permissions ───────────────────────────────────────────────────────────

export interface PermissionsStatus {
    screen_recording: boolean;
    accessibility: boolean;
    microphone: boolean;
}

export async function checkPermissions(): Promise<PermissionsStatus> {
    return invoke<PermissionsStatus>("check_permissions");
}

export async function openSystemSettings(pane: "screen-recording" | "accessibility" | "microphone"): Promise<void> {
    return invoke("open_system_settings", { pane });
}

// ── Models ────────────────────────────────────────────────────────────────

export interface ModelInfo {
    id: string;
    name: string;
    description: string;
    size_bytes: number;
    size_label: string;
    quality_label: string;
    speed_label: string;
    ram_gb: number;
    recommended: boolean;
    filename: string;
    download_url: string;
}

export async function listAvailableModels(): Promise<ModelInfo[]> {
    return invoke<ModelInfo[]>("list_available_models");
}

export async function downloadModel(modelId: string, downloadUrl: string, filename: string): Promise<void> {
    return invoke("download_model", { modelId, downloadUrl, filename });
}

export type ModelDownloadState =
    | "idle"
    | "preparing"
    | "downloading"
    | "finalizing"
    | "completed"
    | "failed";

export async function checkModelExists(filename: string): Promise<boolean> {
    return invoke<boolean>("check_model_exists", { filename });
}

export interface DownloadProgress {
    model_id: string;
    bytes_downloaded: number;
    total_bytes: number;
    percent: number;
    done: boolean;
    error: string | null;
}

export interface ModelDownloadStatus {
    state: ModelDownloadState;
    model_id: string | null;
    filename: string | null;
    download_url: string | null;
    destination_path: string | null;
    temp_path: string | null;
    bytes_downloaded: number;
    total_bytes: number;
    percent: number;
    done: boolean;
    error: string | null;
    logs: string[];
    updated_at_ms: number;
}

export interface AiRuntimeStatus {
    ai_model_available: boolean;
    ai_model_loaded: boolean;
    vlm_loaded: boolean;
    loaded_model_id: string | null;
    loaded_model_path: string | null;
}

export function onDownloadProgress(handler: (p: DownloadProgress) => void): Promise<() => void> {
    return listen<DownloadProgress>("model-download-progress", (event) => {
        handler(event.payload);
    });
}

export function onDownloadStatus(handler: (status: ModelDownloadStatus) => void): Promise<() => void> {
    return listen<ModelDownloadStatus>("model-download-status", (event) => {
        handler(event.payload);
    });
}

export function onDownloadLog(handler: (msg: string) => void): Promise<() => void> {
    return listen<string>("model-download-log", (event) => {
        handler(event.payload);
    });
}

export async function getModelDownloadStatus(): Promise<ModelDownloadStatus> {
    return invoke<ModelDownloadStatus>("get_model_download_status");
}

export async function refreshAiModels(): Promise<AiRuntimeStatus> {
    return invoke<AiRuntimeStatus>("refresh_ai_models");
}

export async function deleteAiModel(filename: string): Promise<void> {
    return invoke("delete_ai_model", { filename });
}

/**
 * Persist the user's preferred local LLM model and (if the GGUF is on disk)
 * swap the live engine immediately. Returns `true` when the engine was
 * reloaded, `false` when the choice was saved but the file is not on disk
 * yet (the next download or app restart will pick it up).
 *
 * The VLM tier (`vlm_model_size` in config.toml) continues to gate Qwen3-VL
 * 1B vs 4B independently of this choice.
 */
export async function setPreferredInferenceModel(modelId: string): Promise<boolean> {
    return invoke<boolean>("set_preferred_inference_model", { modelId });
}
