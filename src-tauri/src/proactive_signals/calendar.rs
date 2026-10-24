//! The calendar on this Mac as a source of meetings for meeting prep
//! (ADR 029). Read-only. An event title stays on this Mac: it is matched
//! against Resume threads and may appear in FNDR's own toast. Meeting prep
//! does not store, log or send event text to a model or host. A separate,
//! explicitly started calendar import may store selected events locally.

use super::meeting_prep::{prep_due, UpcomingMeeting, GRACE_MS};
use super::signal_allowed;
use serde::Serialize;

/// Events starting within this long are read.
pub const LOOKAHEAD_MS: i64 = 30 * 60_000;

/// A brief covers events starting within this long...
pub const BRIEF_AHEAD_MS: i64 = 24 * 60 * 60_000;
/// ...or that started this long ago.
pub const BRIEF_STARTED_MS: i64 = 15 * 60_000;

/// Whether FNDR may read the calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarStatus {
    NotDetermined,
    Restricted,
    Denied,
    Granted,
}

impl CalendarStatus {
    /// From `EKAuthorizationStatus`: 0 not determined, 1 restricted, 2 denied,
    /// 3 full access (`authorized` before macOS 14), 4 write only. Write-only
    /// access cannot read, so it counts as denied, as does any value a later
    /// macOS adds.
    pub fn from_raw(raw: i64) -> Self {
        match raw {
            0 => Self::NotDetermined,
            1 => Self::Restricted,
            3 => Self::Granted,
            _ => Self::Denied,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarInfo {
    pub id: String,
    pub name: String,
    /// A subscribed or birthday calendar: someone else's events.
    pub subscribed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarEvent {
    /// The EventKit identifier, shared by every occurrence of a repeating event.
    pub id: String,
    pub calendar_id: String,
    pub title: Option<String>,
    pub starts_at: i64,
    pub ends_at: i64,
    pub attendees: Vec<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub conference_url: Option<String>,
    pub all_day: bool,
    /// Declined by the person, or cancelled by the organizer.
    pub declined: bool,
}

/// The calendar FNDR reads. EventKit on a Mac; a fake in tests.
pub trait CalendarSource {
    fn status(&self) -> CalendarStatus;
    /// Asks macOS for read access. The system prompt appears only while the
    /// status is not determined; otherwise this returns the status as is.
    fn request_access(&self) -> CalendarStatus;
    fn calendars(&self) -> Result<Vec<CalendarInfo>, String>;
    fn events(
        &self,
        calendar_ids: &[String],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<CalendarEvent>, String>;
}

/// The calendars read: those named in settings, or else every calendar the
/// person did not subscribe to.
pub fn calendars_to_read(calendars: &[CalendarInfo], selected: &[String]) -> Vec<String> {
    calendars
        .iter()
        .filter(|calendar| {
            if selected.is_empty() {
                !calendar.subscribed
            } else {
                selected.contains(&calendar.id)
            }
        })
        .map(|calendar| calendar.id.clone())
        .collect()
}

/// Events worth preparing for, each with an id unique to its occurrence:
/// titled, timed, not declined, from a calendar being read, and starting
/// within the lookahead or started within the grace period.
pub fn upcoming_meetings(
    events: Vec<CalendarEvent>,
    calendar_ids: &[String],
    now_ms: i64,
) -> Vec<(String, UpcomingMeeting)> {
    events
        .into_iter()
        .filter(|event| !event.all_day && !event.declined)
        .filter(|event| calendar_ids.contains(&event.calendar_id))
        .filter(|event| (now_ms - GRACE_MS..=now_ms + LOOKAHEAD_MS).contains(&event.starts_at))
        .filter_map(|event| {
            let title = event.title.as_deref().map(str::trim).unwrap_or_default();
            if title.is_empty() {
                return None;
            }
            Some((
                format!("{}@{}", event.id, event.starts_at),
                UpcomingMeeting {
                    title: title.to_string(),
                    starts_at: event.starts_at,
                },
            ))
        })
        .collect()
}

/// The meeting a brief is for: the earliest titled, timed, accepted event
/// from a calendar being read that starts within a day or began in the last
/// quarter hour.
pub fn next_event(
    events: Vec<CalendarEvent>,
    calendar_ids: &[String],
    now_ms: i64,
) -> Option<CalendarEvent> {
    events
        .into_iter()
        .filter(|event| !event.all_day && !event.declined)
        .filter(|event| calendar_ids.contains(&event.calendar_id))
        .filter(|event| (now_ms - BRIEF_STARTED_MS..=now_ms + BRIEF_AHEAD_MS).contains(&event.starts_at))
        .filter(|event| event.title.as_deref().is_some_and(|t| !t.trim().is_empty()))
        .min_by_key(|event| event.starts_at)
}

/// The calendar meetings prep is due for now. Nothing is read when the
/// switch is off, in Private Mode, or without access; a failed read is
/// logged without any event text and gives nothing.
pub fn due_meetings(
    source: &dyn CalendarSource,
    enabled: bool,
    incognito: bool,
    selected: &[String],
    now_ms: i64,
) -> Vec<(String, UpcomingMeeting)> {
    if !signal_allowed(enabled, incognito) || source.status() != CalendarStatus::Granted {
        return Vec::new();
    }
    let read = source.calendars().and_then(|calendars| {
        let ids = calendars_to_read(&calendars, selected);
        if ids.is_empty() {
            return Ok((ids, Vec::new()));
        }
        let events = source.events(&ids, now_ms - GRACE_MS, now_ms + LOOKAHEAD_MS)?;
        Ok((ids, events))
    });
    match read {
        Ok((ids, events)) => upcoming_meetings(events, &ids, now_ms)
            .into_iter()
            .filter(|(_, meeting)| prep_due(meeting, now_ms))
            .collect(),
        Err(error) => {
            tracing::warn!(error = %error, "proactive_signals:calendar_read_failed");
            Vec::new()
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::Cell;

    pub const NOW: i64 = 1_800_000_000_000;
    pub const MIN: i64 = 60_000;

    pub fn event(id: &str, calendar: &str, title: &str, starts_in_min: i64) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            calendar_id: calendar.into(),
            title: Some(title.into()),
            starts_at: NOW + starts_in_min * MIN,
            ends_at: NOW + (starts_in_min + 30) * MIN,
            attendees: Vec::new(),
            location: None,
            notes: None,
            conference_url: None,
            all_day: false,
            declined: false,
        }
    }

    #[test]
    fn the_brief_is_for_the_earliest_accepted_timed_event_within_a_day() {
        let work = vec!["work".to_string()];
        let mut declined = event("d", "work", "Declined", 5);
        declined.declined = true;
        let mut all_day = event("a", "work", "Offsite", 1);
        all_day.all_day = true;
        let events = vec![
            event("late", "work", "Later", 120),
            declined,
            all_day,
            event("far", "work", "Tomorrow+", 25 * 60),
            event("other", "home", "Dentist", 10),
            event("soon", "work", "Design review", 45),
            event("over", "work", "Long over", -30),
        ];
        let picked = next_event(events, &work, NOW).expect("an event");
        assert_eq!(picked.id, "soon");
        assert!(next_event(Vec::new(), &work, NOW).is_none());
        let started = vec![event("on", "work", "Just began", -10)];
        assert_eq!(next_event(started, &work, NOW).unwrap().id, "on");
    }

    /// A calendar on a fake Mac. It returns every event it holds, whatever
    /// calendars are asked for, so the filtering under test is FNDR's own.
    pub struct FakeCalendar {
        pub status: CalendarStatus,
        pub calendars: Vec<CalendarInfo>,
        pub events: Vec<CalendarEvent>,
        pub reads: Cell<usize>,
        pub prompts: Cell<usize>,
    }

    impl FakeCalendar {
        pub fn granted(events: Vec<CalendarEvent>) -> Self {
            Self {
                status: CalendarStatus::Granted,
                calendars: vec![
                    CalendarInfo {
                        id: "work".into(),
                        name: "Work".into(),
                        subscribed: false,
                    },
                    CalendarInfo {
                        id: "home".into(),
                        name: "Home".into(),
                        subscribed: false,
                    },
                    CalendarInfo {
                        id: "holidays".into(),
                        name: "Holidays".into(),
                        subscribed: true,
                    },
                ],
                events,
                reads: Cell::new(0),
                prompts: Cell::new(0),
            }
        }
    }

    impl CalendarSource for FakeCalendar {
        fn status(&self) -> CalendarStatus {
            self.status
        }
        fn request_access(&self) -> CalendarStatus {
            self.prompts.set(self.prompts.get() + 1);
            self.status
        }
        fn calendars(&self) -> Result<Vec<CalendarInfo>, String> {
            Ok(self.calendars.clone())
        }
        fn events(&self, _: &[String], _: i64, _: i64) -> Result<Vec<CalendarEvent>, String> {
            self.reads.set(self.reads.get() + 1);
            Ok(self.events.clone())
        }
    }

    fn titles(meetings: &[(String, UpcomingMeeting)]) -> Vec<&str> {
        meetings.iter().map(|(_, m)| m.title.as_str()).collect()
    }

    #[test]
    fn every_permission_state_maps_to_a_typed_status() {
        assert_eq!(CalendarStatus::from_raw(0), CalendarStatus::NotDetermined);
        assert_eq!(CalendarStatus::from_raw(1), CalendarStatus::Restricted);
        assert_eq!(CalendarStatus::from_raw(2), CalendarStatus::Denied);
        assert_eq!(CalendarStatus::from_raw(3), CalendarStatus::Granted);
        assert_eq!(
            CalendarStatus::from_raw(4),
            CalendarStatus::Denied,
            "write-only access cannot read"
        );
        assert_eq!(CalendarStatus::from_raw(-1), CalendarStatus::Denied);
        assert_eq!(
            serde_json::to_string(&CalendarStatus::NotDetermined).unwrap(),
            "\"not_determined\""
        );
    }

    #[test]
    fn by_default_every_calendar_but_subscribed_ones_is_read() {
        let fake = FakeCalendar::granted(Vec::new());
        assert_eq!(
            calendars_to_read(&fake.calendars, &[]),
            vec!["work", "home"]
        );
        assert_eq!(
            calendars_to_read(&fake.calendars, &["holidays".into(), "gone".into()]),
            vec!["holidays"],
            "a calendar picked in settings is read even when subscribed"
        );
    }

    #[test]
    fn declined_all_day_untitled_and_far_events_are_skipped() {
        let mut declined = event("d", "work", "Budget review", 10);
        declined.declined = true;
        let mut all_day = event("a", "work", "Offsite planning", 0);
        all_day.all_day = true;
        let mut no_title = event("n", "work", "", 10);
        no_title.title = None;
        let events = vec![
            event("k", "work", "Platform hiring sync", 10),
            declined,
            all_day,
            no_title,
            event("blank", "work", "   ", 10),
            event("far", "work", "Quarterly planning", 45),
            event("past", "work", "Standup", -10),
            event("other", "holidays", "Bank holiday", 10),
            event("h", "home", "Dentist", 29),
        ];
        let ids = vec!["work".to_string(), "home".to_string()];
        let meetings = upcoming_meetings(events, &ids, NOW);
        assert_eq!(titles(&meetings), vec!["Platform hiring sync", "Dentist"]);
        assert_eq!(meetings[0].0, format!("k@{}", NOW + 10 * MIN));
    }

    #[test]
    fn each_occurrence_of_a_repeating_event_has_its_own_id() {
        let ids = vec!["work".to_string()];
        let meetings = upcoming_meetings(
            vec![
                event("r", "work", "Standup", 2),
                event("r", "work", "Standup", 20),
            ],
            &ids,
            NOW,
        );
        assert_eq!(meetings.len(), 2);
        assert_ne!(meetings[0].0, meetings[1].0);
    }

    #[test]
    fn only_meetings_about_to_start_are_due_across_calendars() {
        let fake = FakeCalendar::granted(vec![
            event("w", "work", "Hiring sync", 3),
            event("h", "home", "School pickup", 4),
            event("s", "holidays", "Bank holiday", 3),
            event("later", "work", "Design review", 20),
        ]);
        let due = due_meetings(&fake, true, false, &[], NOW);
        assert_eq!(titles(&due), vec!["Hiring sync", "School pickup"]);

        let only_home = due_meetings(&fake, true, false, &["home".into()], NOW);
        assert_eq!(titles(&only_home), vec!["School pickup"]);
    }

    #[test]
    fn nothing_is_read_when_off_in_private_mode_or_without_access() {
        let fake = FakeCalendar::granted(vec![event("w", "work", "Hiring sync", 3)]);
        assert!(due_meetings(&fake, false, false, &[], NOW).is_empty());
        assert!(due_meetings(&fake, true, true, &[], NOW).is_empty());
        assert_eq!(fake.reads.get(), 0, "off or Private Mode reads nothing");

        for status in [
            CalendarStatus::NotDetermined,
            CalendarStatus::Denied,
            CalendarStatus::Restricted,
        ] {
            let mut fake = FakeCalendar::granted(vec![event("w", "work", "Hiring sync", 3)]);
            fake.status = status;
            assert!(due_meetings(&fake, true, false, &[], NOW).is_empty());
            assert_eq!(fake.reads.get(), 0);
            assert_eq!(fake.prompts.get(), 0, "the background never prompts");
        }
    }
}
