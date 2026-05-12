//! Meeting recorder Tauri commands.

use crate::meeting::{self, MeetingRecorderStatus, MeetingTranscript};
use crate::store::MeetingSession;

/// Get meeting recorder status
#[tauri::command]
pub async fn get_meeting_status() -> Result<MeetingRecorderStatus, String> {
    meeting::recorder_status()
}

/// Start a meeting recording session
#[tauri::command]
pub async fn start_meeting_recording(
    app: tauri::AppHandle,
    title: String,
    participants: Option<Vec<String>>,
    model: Option<String>,
) -> Result<MeetingRecorderStatus, String> {
    meeting::start_recording(Some(app), title, participants.unwrap_or_default(), model).await
}

/// Stop the active meeting recording session
#[tauri::command]
pub async fn stop_meeting_recording() -> Result<MeetingRecorderStatus, String> {
    meeting::stop_recording().await
}

/// List all local meeting sessions
#[tauri::command]
pub async fn list_meetings() -> Result<Vec<MeetingSession>, String> {
    meeting::list_meetings().await
}

/// Delete a local meeting session and its persisted artifacts
#[tauri::command]
pub async fn delete_meeting(meeting_id: String) -> Result<bool, String> {
    meeting::delete_meeting(&meeting_id).await
}

/// Get full transcript for a meeting
#[tauri::command]
pub async fn get_meeting_transcript(meeting_id: String) -> Result<MeetingTranscript, String> {
    meeting::get_meeting_transcript(&meeting_id).await
}

/// Re-run transcription on an existing meeting (useful after STT backend changes)
#[tauri::command]
pub async fn retranscribe_meeting(meeting_id: String) -> Result<(), String> {
    meeting::retranscribe_meeting(&meeting_id).await
}
