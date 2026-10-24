//! Meeting prep from the calendar (ADR 029): its switch and macOS calendar
//! access. The calendar is read on this Mac only; see
//! `proactive_signals::calendar`.

use crate::context_runtime::{retrieve_search_results, RetrieveRequest};
use crate::proactive_signals::calendar::{
    calendars_to_read, next_event, CalendarSource, CalendarStatus, BRIEF_AHEAD_MS,
    BRIEF_STARTED_MS,
};
use crate::proactive_signals::eventkit::EventKitCalendar;
use crate::proactive_signals::meeting_prep::related_thread;
use crate::AppState;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::State;

const CALENDAR_PRIVACY_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarPrepState {
    pub enabled: bool,
    pub status: CalendarStatus,
}

/// Turning the switch on asks macOS for access (the system prompt appears
/// only the first time) and stays on only when access is granted. Turning it
/// off asks for nothing.
fn switch_calendar_prep(source: &dyn CalendarSource, requested: bool) -> CalendarPrepState {
    let status = if requested {
        source.request_access()
    } else {
        source.status()
    };
    CalendarPrepState {
        enabled: requested && status == CalendarStatus::Granted,
        status,
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn calendar_meeting_prep_state(
    state: State<'_, Arc<AppState>>,
) -> Result<CalendarPrepState, String> {
    let status = blocking(|| EventKitCalendar.status()).await?;
    Ok(CalendarPrepState {
        enabled: state
            .inner()
            .config
            .read()
            .proactive_signals
            .calendar_meeting_prep,
        status,
    })
}

#[tauri::command]
pub async fn set_calendar_meeting_prep(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
) -> Result<CalendarPrepState, String> {
    let next = blocking(move || switch_calendar_prep(&EventKitCalendar, enabled)).await?;
    let mut config = state.inner().config.write();
    let previous = config.proactive_signals.calendar_meeting_prep;
    config.proactive_signals.calendar_meeting_prep = next.enabled;
    if let Err(err) = config.save() {
        config.proactive_signals.calendar_meeting_prep = previous;
        return Err(err.to_string());
    }
    Ok(next)
}

const BRIEF_MEMORIES: usize = 6;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefMemory {
    pub id: String,
    pub title: String,
    pub app_name: String,
    pub timestamp: i64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefThread {
    pub key: String,
    pub title: String,
}

/// Why a brief is empty, or that it holds a meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefState {
    /// The calendar switch is off.
    Off,
    /// The switch is on but macOS has not granted calendar access.
    NoAccess,
    /// Private Mode: nothing is read.
    Private,
    /// No meeting in the next day.
    NoMeeting,
    Ready,
}

/// The next meeting and what FNDR remembers that bears on it. The event
/// title goes only to this Mac's own search and to the window that asked.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingBrief {
    pub state: BriefState,
    pub title: Option<String>,
    pub starts_at: Option<i64>,
    pub strong_match: bool,
    pub thread: Option<BriefThread>,
    pub memories: Vec<BriefMemory>,
}

impl MeetingBrief {
    fn empty(state: BriefState) -> Self {
        Self {
            state,
            title: None,
            starts_at: None,
            strong_match: false,
            thread: None,
            memories: Vec::new(),
        }
    }
}

#[tauri::command]
pub async fn next_meeting_brief(state: State<'_, Arc<AppState>>) -> Result<MeetingBrief, String> {
    let app = state.inner();
    if app.is_incognito.load(Ordering::SeqCst) {
        return Ok(MeetingBrief::empty(BriefState::Private));
    }
    let (enabled, selected) = {
        let config = app.config.read();
        (
            config.proactive_signals.calendar_meeting_prep,
            config.proactive_signals.calendar_ids.clone(),
        )
    };
    if !enabled {
        return Ok(MeetingBrief::empty(BriefState::Off));
    }
    let now_ms = chrono::Utc::now().timestamp_millis();
    let event = blocking(move || -> Result<_, String> {
        let source = EventKitCalendar;
        if source.status() != CalendarStatus::Granted {
            return Ok(None);
        }
        let calendars = source.calendars()?;
        let ids = calendars_to_read(&calendars, &selected);
        if ids.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let events = source.events(&ids, now_ms - BRIEF_STARTED_MS, now_ms + BRIEF_AHEAD_MS)?;
        Ok(Some(next_event(events, &ids, now_ms).into_iter().collect()))
    })
    .await??;
    let Some(events) = event else {
        return Ok(MeetingBrief::empty(BriefState::NoAccess));
    };
    let Some(event) = events.into_iter().next() else {
        return Ok(MeetingBrief::empty(BriefState::NoMeeting));
    };
    let title = event.title.clone().unwrap_or_default().trim().to_string();

    let request = RetrieveRequest {
        query: title.clone(),
        time: None,
        app: None,
        limit: BRIEF_MEMORIES,
    };
    let (retrieved, results) = retrieve_search_results(app, &request)
        .await
        .unwrap_or_default();
    let thread = related_thread(app, &title).await.map(|thread| BriefThread {
        key: thread.key,
        title: thread.title,
    });
    let memories = if retrieved.strong_match {
        results
            .into_iter()
            .take(BRIEF_MEMORIES)
            .map(|hit| BriefMemory {
                title: if hit.window_title.trim().is_empty() {
                    hit.app_name.clone()
                } else {
                    hit.window_title.clone()
                },
                id: hit.id,
                app_name: hit.app_name,
                timestamp: hit.timestamp,
                snippet: if hit.display_summary.trim().is_empty() {
                    hit.snippet
                } else {
                    hit.display_summary
                },
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(MeetingBrief {
        state: BriefState::Ready,
        title: Some(title),
        starts_at: Some(event.starts_at),
        strong_match: retrieved.strong_match,
        thread,
        memories,
    })
}

/// Opens System Settings at Privacy and Security, Calendars.
#[tauri::command]
pub async fn open_calendar_privacy_settings() -> Result<(), String> {
    tokio::process::Command::new("open")
        .arg(CALENDAR_PRIVACY_URL)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proactive_signals::calendar::tests::FakeCalendar;

    #[test]
    fn turning_on_asks_for_access_and_stays_on_only_when_granted() {
        let fake = FakeCalendar::granted(Vec::new());
        assert_eq!(
            switch_calendar_prep(&fake, true),
            CalendarPrepState {
                enabled: true,
                status: CalendarStatus::Granted
            }
        );
        assert_eq!(fake.prompts.get(), 1);

        for status in [
            CalendarStatus::Denied,
            CalendarStatus::Restricted,
            CalendarStatus::NotDetermined,
        ] {
            let mut fake = FakeCalendar::granted(Vec::new());
            fake.status = status;
            assert_eq!(
                switch_calendar_prep(&fake, true),
                CalendarPrepState {
                    enabled: false,
                    status
                }
            );
        }
    }

    #[test]
    fn turning_off_asks_for_nothing() {
        let fake = FakeCalendar::granted(Vec::new());
        assert!(!switch_calendar_prep(&fake, false).enabled);
        assert_eq!(fake.prompts.get(), 0);
    }
}
