//! Today's calendar: days and times on this PC's clock, events as Graph gives them, where each goes on the timeline,
//! and the few things the panel can do with them.

use super::graph::{Graph, GraphError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

// ---- Days ----

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar.
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a number of days since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + i64::from(month <= 2)) as i32, month, day)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Day {
    pub fn today() -> Day {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let local = crate::platform::local_time(now);
        Day { year: local.year, month: local.month, day: local.day }
    }

    pub fn number(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    pub fn offset(self, days: i64) -> Day {
        let (year, month, day) = civil_from_days(self.number() + days);
        Day { year, month, day }
    }

    /// 0 for Monday to 6 for Sunday.
    pub fn weekday(self) -> u32 {
        (self.number() + 3).rem_euclid(7) as u32
    }
}

/// A moment on this PC's clock.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct When {
    pub day: Day,
    /// Minutes since midnight.
    pub minute: u32,
}

impl When {
    /// Minutes from the start of `day`, which can be below zero or past a day's end.
    pub fn minutes_from(self, day: Day) -> i64 {
        (self.day.number() - day.number()) * 1440 + i64::from(self.minute)
    }
}

/// "2026-11-13T09:30:00.0000000", with or without seconds, fractions or a Z: the date and the minute of the day.
fn parse_stamp(text: &str) -> Option<When> {
    let (date, time) = text.trim().split_once('T')?;
    let mut d = date.split('-');
    let day = Day { year: d.next()?.parse().ok()?, month: d.next()?.parse().ok()?, day: d.next()?.parse().ok()? };
    let mut t = time.trim_end_matches('Z').split(':');
    let hour: u32 = t.next()?.parse().ok()?;
    let minute: u32 = t.next()?.get(..2)?.parse().ok()?;
    ((1..=12).contains(&day.month) && (1..=31).contains(&day.day) && hour < 24 && minute < 60).then_some(When { day, minute: hour * 60 + minute })
}

/// A UTC time as Graph writes it, in seconds since 1970.
pub fn utc_secs(text: &str) -> Option<u64> {
    let when = parse_stamp(text)?;
    u64::try_from(when.day.number() * 86_400 + i64::from(when.minute) * 60).ok()
}

/// Seconds since 1970 as a UTC time for Graph: 2026-11-13T08:00:00Z.
pub fn iso_utc(secs: u64) -> String {
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rest = secs % 86_400;
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// A UTC moment on this PC's clock.
pub fn utc_to_local(utc: When) -> When {
    let secs = utc.day.number() * 86_400 + i64::from(utc.minute) * 60;
    let local = crate::platform::local_time(u64::try_from(secs).unwrap_or(0));
    When { day: Day { year: local.year, month: local.month, day: local.day }, minute: local.hour * 60 + local.minute }
}

/// The Windows name of this PC's time zone, such as "W. Europe Standard Time", which Graph understands.
pub fn windows_zone() -> String {
    use windows::Win32::System::Time::{DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation};
    let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
    unsafe { GetDynamicTimeZoneInformation(&mut info) };
    let len = info.TimeZoneKeyName.iter().position(|&c| c == 0).unwrap_or(info.TimeZoneKeyName.len());
    let name = String::from_utf16_lossy(&info.TimeZoneKeyName[..len]);
    if name.is_empty() { "UTC".into() } else { name }
}

// ---- Events ----

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Answer {
    None,
    Organizer,
    Accepted,
    Tentative,
    Declined,
    NotResponded,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub subject: String,
    pub start: When,
    pub end: When,
    pub all_day: bool,
    pub location: String,
    pub web_link: String,
    /// The online meeting to join, empty without one.
    pub join_url: String,
    pub answer: Answer,
    pub organizer: bool,
    pub cancelled: bool,
}

impl Event {
    /// Someone else's invitation, not answered yet.
    pub fn is_invitation(&self) -> bool {
        !self.organizer && !self.cancelled && matches!(self.answer, Answer::NotResponded | Answer::None)
    }
}

fn is_utc(zone: &str) -> bool {
    let zone = zone.trim().to_ascii_lowercase();
    zone == "utc" || zone == "tzone://microsoft/utc" || zone == "coordinated universal time" // i18n-ignore: time zone names
}

/// The events of a calendarView answer, asked for in `zone`. Times in that zone are this PC's clock already; one
/// Graph still gives in UTC goes through `to_local`. All-day events keep their dates as they are.
pub fn parse_events(json: &str, zone: &str, to_local: &dyn Fn(When) -> When) -> Vec<Event> {
    let value: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let mut events: Vec<Event> = value["value"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|e| {
                    let all_day = e["isAllDay"].as_bool().unwrap_or(false);
                    let moment = |part: &Value| -> Option<When> {
                        let when = parse_stamp(part["dateTime"].as_str()?)?;
                        let in_zone = part["timeZone"].as_str().unwrap_or(zone);
                        Some(if !all_day && is_utc(in_zone) && !is_utc(zone) { to_local(when) } else { when })
                    };
                    let (start, end) = (moment(&e["start"])?, moment(&e["end"])?);
                    let answer = match e["responseStatus"]["response"].as_str().unwrap_or("none") {
                        "organizer" => Answer::Organizer,
                        "accepted" => Answer::Accepted,
                        "tentativelyAccepted" => Answer::Tentative,
                        "declined" => Answer::Declined,
                        "notResponded" => Answer::NotResponded,
                        _ => Answer::None,
                    };
                    Some(Event {
                        id: e["id"].as_str().unwrap_or("").to_string(),
                        subject: e["subject"].as_str().unwrap_or("").trim().to_string(),
                        start,
                        end: end.max(start),
                        all_day,
                        location: e["location"]["displayName"].as_str().unwrap_or("").trim().to_string(),
                        web_link: e["webLink"].as_str().unwrap_or("").to_string(),
                        join_url: e["onlineMeeting"]["joinUrl"].as_str().unwrap_or("").to_string(),
                        organizer: e["isOrganizer"].as_bool().unwrap_or(false) || answer == Answer::Organizer,
                        cancelled: e["isCancelled"].as_bool().unwrap_or(false),
                        answer,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    events.sort_by_key(|e| (!e.all_day, e.start, e.end));
    events
}

/// All-day events on this day: from their first day up to, not including, the day they end.
pub fn all_day_on(events: &[Event], day: Day) -> Vec<usize> {
    events.iter().enumerate().filter(|(_, e)| e.all_day && e.start.day <= day && (day < e.end.day || e.start.day == day)).map(|(i, _)| i).collect()
}

/// Where an event goes on a day's timeline: minutes from midnight, and its column among the ones it overlaps.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    pub index: usize,
    pub top: u32,
    pub bottom: u32,
    pub column: usize,
    pub columns: usize,
}

/// Events too short to read get this much room: enough for their title on the panel's timeline.
const SHORTEST: u32 = 25;

/// The timed events on this day, side by side where they overlap.
pub fn place(events: &[Event], day: Day) -> Vec<Placed> {
    let mut placed: Vec<Placed> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.all_day)
        .filter_map(|(index, e)| {
            let top = e.start.minutes_from(day).clamp(0, 1440) as u32;
            let bottom = e.end.minutes_from(day).clamp(0, 1440) as u32;
            let starts_today = e.start.minutes_from(day) < 1440;
            let ends_after_midnight = e.end.minutes_from(day) > 0;
            (starts_today && (ends_after_midnight || e.start.day == day)).then(|| Placed { index, top: top.min(1440 - SHORTEST), bottom: bottom.max(top + SHORTEST).min(1440), column: 0, columns: 1 })
        })
        .collect();
    placed.sort_by_key(|p| (p.top, p.bottom));
    // Events that overlap, directly or through one another, share the width; each takes the first column free at its
    // start.
    let mut i = 0;
    while i < placed.len() {
        let mut end = placed[i].bottom;
        let mut j = i + 1;
        while j < placed.len() && placed[j].top < end {
            end = end.max(placed[j].bottom);
            j += 1;
        }
        let mut ends: Vec<u32> = Vec::new();
        for p in &mut placed[i..j] {
            let column = ends.iter().position(|e| *e <= p.top).unwrap_or(ends.len());
            if column == ends.len() {
                ends.push(p.bottom);
            } else {
                ends[column] = p.bottom;
            }
            p.column = column;
        }
        let columns = ends.len();
        placed[i..j].iter_mut().for_each(|p| p.columns = columns);
        i = j;
    }
    placed
}

// ---- Reading and writing ----

const FIELDS: &str = "id,subject,start,end,isAllDay,location,webLink,onlineMeeting,isOnlineMeeting,responseStatus,isOrganizer,isCancelled";

/// A day's events, from midnight to midnight on this PC's clock, given as UTC.
pub fn view_path(start_utc: u64, end_utc: u64) -> String {
    format!("/me/calendarView?startDateTime={}&endDateTime={}&$top=100&$orderby=start/dateTime&$select={FIELDS}", iso_utc(start_utc), iso_utc(end_utc))
}

/// Asks Graph for times in this zone, so they come as this PC's clock shows them.
pub fn zone_header(zone: &str) -> String {
    format!("outlook.timezone=\"{zone}\"")
}

/// The UTC moments a day on this PC's clock begins and ends.
pub fn day_bounds(day: Day) -> Option<(u64, u64)> {
    let next = day.offset(1);
    Some((crate::platform::local_midnight(day.year, day.month, day.day)?, crate::platform::local_midnight(next.year, next.month, next.day)?))
}

pub fn fetch_day(graph: &dyn Graph, bounds: (u64, u64), zone: &str, to_local: &dyn Fn(When) -> When) -> Result<Vec<Event>, GraphError> {
    let prefer = zone_header(zone);
    let json = graph.get(&view_path(bounds.0, bounds.1), &[("Prefer", prefer.as_str())])?; // i18n-ignore: an HTTP header
    Ok(parse_events(&json, zone, to_local))
}

/// Answers an invitation, and tells the organizer.
pub fn respond(graph: &dyn Graph, id: &str, answer: Answer) -> Result<(), GraphError> {
    let action = match answer {
        Answer::Accepted => "accept",
        Answer::Tentative => "tentativelyAccept",
        Answer::Declined => "decline",
        _ => return Ok(()),
    };
    graph.post(&format!("/me/events/{}/{action}", crate::search::percent_encode(id)), &json!({ "sendResponse": true }).to_string()).map(|_| ())
}

/// What a new event is sent as: its title, and its start and end on this PC's clock.
pub fn new_event_body(subject: &str, start: When, end: When, zone: &str) -> String {
    let stamp = |w: When| format!("{:04}-{:02}-{:02}T{:02}:{:02}:00", w.day.year, w.day.month, w.day.day, w.minute / 60, w.minute % 60);
    json!({
        "subject": subject.trim(),
        "start": { "dateTime": stamp(start), "timeZone": zone },
        "end": { "dateTime": stamp(end.max(start)), "timeZone": zone },
    })
    .to_string()
}

pub fn create(graph: &dyn Graph, subject: &str, start: When, end: When, zone: &str) -> Result<(), GraphError> {
    graph.post("/me/events", &new_event_body(subject, start, end, zone)).map(|_| ())
}

/// "9:30", "09:30" or "930": minutes since midnight.
pub fn parse_clock(text: &str) -> Option<u32> {
    let text = text.trim();
    // Only digits and one separator: anything else, such as a letter with an accent, would cut a character in two below.
    if !text.chars().all(|c| c.is_ascii_digit() || c == ':' || c == '.') {
        return None;
    }
    let (hour, minute) = match text.split_once([':', '.']) {
        Some((h, m)) => (h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?),
        None if text.len() > 2 => (text[..text.len() - 2].parse().ok()?, text[text.len() - 2..].parse().ok()?),
        None => (text.parse().ok()?, 0),
    };
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

// ---- The copy kept for next time ----

/// The last answer, so the panel shows something at once and while offline. Kept encrypted for this Windows account.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cached {
    pub day: Day,
    /// When it was read, in seconds since 1970.
    pub fetched: u64,
    pub events: Vec<Event>,
}

pub fn save_cache(path: &Path, cached: &Cached) {
    let Ok(text) = serde_json::to_vec(cached) else { return };
    if let Some(sealed) = super::auth::protect(&text) {
        let _ = std::fs::write(path, sealed);
    }
}

pub fn load_cache(path: &Path) -> Option<Cached> {
    let sealed = std::fs::read(path).ok()?;
    serde_json::from_slice(&super::auth::unprotect(&sealed)?).ok()
}
