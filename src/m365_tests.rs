//! Microsoft 365 without signing in: Graph is a stand-in that answers with recorded JSON and remembers what it was
//! asked.

use super::auth::{self, SignInError};
use super::calendar::{self, Answer, Day, Event, When};
use super::graph::{self, Graph, GraphError, Tokens};
use super::*;
use crate::config::Config;
use crate::policy::{Policies, tests_raw};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Answers recorded for the paths they start with; remembers every request.
#[derive(Default)]
struct RecordedGraph {
    answers: Vec<(&'static str, Result<String, GraphError>)>,
    asked: Mutex<Vec<(String, Vec<(String, String)>)>>,
    posted: Mutex<Vec<(String, String)>>,
}

impl RecordedGraph {
    fn answering(answers: Vec<(&'static str, Result<String, GraphError>)>) -> RecordedGraph {
        RecordedGraph { answers, ..Default::default() }
    }
}

impl Graph for RecordedGraph {
    fn get(&self, path: &str, headers: &[(&str, &str)]) -> Result<String, GraphError> {
        self.asked.lock().unwrap().push((path.to_string(), headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()));
        self.answers.iter().find(|(start, _)| path.starts_with(start)).map(|(_, answer)| answer.clone()).unwrap_or(Err(GraphError::Refused(404, "not recorded".into())))
    }

    fn post(&self, path: &str, body: &str) -> Result<String, GraphError> {
        self.posted.lock().unwrap().push((path.to_string(), body.to_string()));
        Ok("{}".into())
    }
}

#[derive(Default)]
struct CountedTokens {
    forgotten: AtomicUsize,
}

impl Tokens for CountedTokens {
    fn token(&self) -> Result<String, GraphError> {
        Ok("token".into())
    }

    fn forget(&self) {
        self.forgotten.fetch_add(1, Ordering::SeqCst);
    }
}

fn signed_in_cfg() -> Config {
    Config { m365: true, m365_client_id: "11111111-2222-3333-4444-555555555555".into(), m365_account_id: "account-1".into(), roots: vec![], ..Config::default() }
}

fn temp_store(name: &str) -> Store {
    let dir = std::env::temp_dir().join(format!("startline-m365-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    Store { dir }
}

const DAY: Day = Day { year: 2026, month: 11, day: 13 };
const ZONE: &str = "W. Europe Standard Time";

/// A calendarView answer asked for in W. Europe Standard Time: one event in that zone, an all-day event over two
/// days, and one Graph still gave in UTC.
const CALENDAR_VIEW: &str = r#"{
  "@odata.context": "https://graph.microsoft.com/v1.0/$metadata#users('me')/calendarView",
  "value": [
    {
      "id": "AAMkAGI1",
      "subject": "Stand-up",
      "isAllDay": false,
      "isCancelled": false,
      "isOrganizer": false,
      "isOnlineMeeting": true,
      "start": { "dateTime": "2026-11-13T09:00:00.0000000", "timeZone": "W. Europe Standard Time" },
      "end": { "dateTime": "2026-11-13T09:15:00.0000000", "timeZone": "W. Europe Standard Time" },
      "location": { "displayName": "Microsoft Teams Meeting" },
      "webLink": "https://outlook.office365.com/owa/?itemid=AAMkAGI1&exvsurl=1&path=/calendar/item",
      "responseStatus": { "response": "accepted", "time": "2026-11-10T08:00:00Z" },
      "onlineMeeting": { "joinUrl": "https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0" }
    },
    {
      "id": "AAMkAGI2",
      "subject": "Team offsite",
      "isAllDay": true,
      "isCancelled": false,
      "isOrganizer": true,
      "start": { "dateTime": "2026-11-13T00:00:00.0000000", "timeZone": "UTC" },
      "end": { "dateTime": "2026-11-15T00:00:00.0000000", "timeZone": "UTC" },
      "location": { "displayName": "Lake house" },
      "webLink": "https://outlook.office365.com/owa/?itemid=AAMkAGI2",
      "responseStatus": { "response": "organizer" },
      "onlineMeeting": null
    },
    {
      "id": "AAMkAGI3",
      "subject": "Late call with Sydney",
      "isAllDay": false,
      "isOrganizer": false,
      "start": { "dateTime": "2026-11-13T21:30:00.0000000", "timeZone": "UTC" },
      "end": { "dateTime": "2026-11-13T22:30:00.0000000", "timeZone": "UTC" },
      "webLink": "https://outlook.office365.com/owa/?itemid=AAMkAGI3",
      "responseStatus": { "response": "notResponded" },
      "onlineMeeting": null
    }
  ]
}"#;

/// Central European winter time is UTC plus an hour.
fn winter(utc: When) -> When {
    let minutes = utc.day.number() * 1440 + i64::from(utc.minute) + 60;
    let (year, month, day) = calendar::civil_from_days(minutes.div_euclid(1440));
    When { day: Day { year, month, day }, minute: minutes.rem_euclid(1440) as u32 }
}

#[test]
fn events_are_read_with_all_day_ones_first_and_utc_ones_moved_to_this_clock() {
    let events = calendar::parse_events(CALENDAR_VIEW, ZONE, &winter);
    assert_eq!(events.len(), 3);
    let offsite = &events[0];
    assert!(offsite.all_day && offsite.organizer && offsite.subject == "Team offsite");
    assert_eq!((offsite.start, offsite.end), (When { day: DAY, minute: 0 }, When { day: DAY.offset(2), minute: 0 }), "all-day dates stay as they are");
    let standup = &events[1];
    assert_eq!((standup.start.minute, standup.end.minute), (9 * 60, 9 * 60 + 15), "already in the zone asked for");
    assert_eq!(standup.join_url, "https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0");
    assert_eq!(standup.answer, Answer::Accepted);
    assert!(!standup.is_invitation());
    let late = &events[2];
    assert_eq!((late.start.minute, late.end.minute), (22 * 60 + 30, 23 * 60 + 30), "UTC moved an hour on");
    assert!(late.is_invitation() && late.join_url.is_empty());
    assert_eq!(calendar::all_day_on(&events, DAY), [0]);
    assert_eq!(calendar::all_day_on(&events, DAY.offset(1)), [0], "the second day of the offsite");
    assert!(calendar::all_day_on(&events, DAY.offset(2)).is_empty(), "the day it ends isn't part of it");
    assert!(calendar::parse_events("not json", ZONE, &winter).is_empty());
}

#[test]
fn the_day_is_asked_for_in_this_pcs_time_zone() {
    let recorded = RecordedGraph::answering(vec![("/me/calendarView", Ok(CALENDAR_VIEW.into()))]);
    let start = calendar::days_from_civil(2026, 11, 12) as u64 * 86_400 + 23 * 3600;
    let events = calendar::fetch_day(&recorded, (start, start + 86_400), ZONE, &winter).unwrap();
    assert_eq!(events.len(), 3);
    let asked = recorded.asked.lock().unwrap();
    let (path, headers) = &asked[0];
    assert!(path.contains("startDateTime=2026-11-12T23:00:00Z&endDateTime=2026-11-13T23:00:00Z"), "{path}");
    assert!(path.contains("$select=") && path.contains("onlineMeeting"));
    assert_eq!(headers, &[("Prefer".to_string(), "outlook.timezone=\"W. Europe Standard Time\"".to_string())]);
}

fn event(start: (u32, u32), end: (u32, u32)) -> Event {
    Event {
        id: String::new(),
        subject: String::new(),
        start: When { day: DAY, minute: start.0 * 60 + start.1 },
        end: When { day: DAY, minute: end.0 * 60 + end.1 },
        all_day: false,
        location: String::new(),
        web_link: String::new(),
        join_url: String::new(),
        answer: Answer::Accepted,
        organizer: false,
        cancelled: false,
    }
}

#[test]
fn overlapping_events_share_the_width_and_short_ones_stay_readable() {
    let mut events = vec![event((9, 0), (10, 0)), event((9, 30), (10, 30)), event((10, 0), (11, 0)), event((14, 0), (14, 5))];
    let mut overnight = event((23, 0), (23, 0));
    overnight.start.day = DAY.offset(-1);
    overnight.end = When { day: DAY, minute: 60 };
    events.push(overnight);
    let placed = calendar::place(&events, DAY);
    let of = |index: usize| *placed.iter().find(|p| p.index == index).unwrap();
    assert_eq!((of(0).column, of(1).column, of(2).column), (0, 1, 0), "the third takes the column the first left");
    assert!([of(0), of(1), of(2)].iter().all(|p| p.columns == 2));
    assert_eq!((of(3).top, of(3).bottom, of(3).columns), (14 * 60, 14 * 60 + 20, 1), "five minutes get room to read");
    assert_eq!((of(4).top, of(4).bottom), (0, 60), "from yesterday, it starts at midnight");
    assert!(calendar::place(&events, DAY.offset(1)).is_empty());
}

#[test]
fn invitations_are_answered_and_new_events_written_in_this_zone() {
    let recorded = RecordedGraph::default();
    calendar::respond(&recorded, "AAMk=1", Answer::Tentative).unwrap();
    calendar::respond(&recorded, "AAMk=1", Answer::Declined).unwrap();
    calendar::create(&recorded, " Planning ", When { day: DAY, minute: 600 }, When { day: DAY, minute: 630 }, ZONE).unwrap();
    let posted = recorded.posted.lock().unwrap();
    assert_eq!(posted[0], ("/me/events/AAMk%3D1/tentativelyAccept".to_string(), r#"{"sendResponse":true}"#.to_string()));
    assert_eq!(posted[1].0, "/me/events/AAMk%3D1/decline");
    let body: serde_json::Value = serde_json::from_str(&posted[2].1).unwrap();
    assert_eq!(posted[2].0, "/me/events");
    assert_eq!(body, json!({ "subject": "Planning", "start": { "dateTime": "2026-11-13T10:00:00", "timeZone": ZONE }, "end": { "dateTime": "2026-11-13T10:30:00", "timeZone": ZONE } }));
    assert_eq!(calendar::parse_clock("9:30"), Some(570));
    assert_eq!(calendar::parse_clock("0930"), Some(570));
    assert_eq!(calendar::parse_clock("14"), Some(840));
    assert_eq!(calendar::parse_clock("25:00"), None);
    assert_eq!(calendar::parse_clock("é0"), None);
    assert_eq!(calendar::parse_clock("9h30"), None);
}

#[test]
fn dates_and_times_are_counted_right() {
    for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 11, 13), (2028, 12, 31)] {
        assert_eq!(calendar::civil_from_days(calendar::days_from_civil(y, m, d)), (y, m, d));
    }
    assert_eq!(calendar::days_from_civil(1970, 1, 1), 0);
    assert_eq!(DAY.weekday(), 4, "13 November 2026 is a Friday");
    assert_eq!(Day { year: 2026, month: 12, day: 31 }.offset(1), Day { year: 2027, month: 1, day: 1 });
    assert_eq!(Day { year: 2028, month: 3, day: 1 }.offset(-1), Day { year: 2028, month: 2, day: 29 });
    let secs = calendar::utc_secs("2026-11-12T08:15:00Z").unwrap();
    assert_eq!(calendar::iso_utc(secs), "2026-11-12T08:15:00Z");
}

#[test]
fn people_are_found_by_name_or_mail_and_rooms_without_an_address_left_out() {
    let path = graph::people_path("meg \"b");
    assert!(path.starts_with("/users?$search=%22displayName%3Ameg%20b%22%20OR%20%22mail%3Ameg%20b%22&"), "{path}");
    assert_eq!(graph::PEOPLE_HEADERS, [("ConsistencyLevel", "eventual")]);
    let recorded = r#"{
      "@odata.context": "https://graph.microsoft.com/v1.0/$metadata#users(displayName,jobTitle,mail,userPrincipalName)",
      "value": [
        { "displayName": "Megan Bowen", "jobTitle": "Marketing Manager", "mail": "MeganB@contoso.com", "userPrincipalName": "MeganB@contoso.com" },
        { "displayName": "Guest Writer", "jobTitle": null, "mail": "writer@fabrikam.com", "userPrincipalName": "" },
        { "displayName": "Room 1", "jobTitle": null, "mail": null, "userPrincipalName": null }
      ]
    }"#;
    let people = graph::parse_people(recorded);
    assert_eq!(people.len(), 2);
    assert_eq!(people[0], graph::Person { name: "Megan Bowen".into(), title: "Marketing Manager".into(), mail: "MeganB@contoso.com".into(), upn: "MeganB@contoso.com".into() });
    assert_eq!(people[1].teams_id(), "writer@fabrikam.com", "no sign-in name: Teams finds them by mail");
    let rows = crate::providers::people_rows(people);
    assert_eq!(rows[0].detail, "Marketing Manager, MeganB@contoso.com");
    assert_eq!(rows[0].category, crate::config::Category::People);
    assert_eq!(chat_link("MeganB@contoso.com", false), "https://teams.microsoft.com/l/chat/0/0?users=MeganB%40contoso.com");
    assert_eq!(chat_link("MeganB@contoso.com", true), "msteams:/l/chat/0/0?users=MeganB%40contoso.com", "in the Teams app when it's there");
    assert_eq!(call_link("a@b.c", true), "msteams:/l/call/0/0?users=a%40b.c");
    assert_eq!(mail_link("a@b.c"), "mailto:a@b.c");
    assert_eq!(teams_link("https://example.com/x", true), "https://example.com/x", "only Teams links go to Teams");
}

#[test]
fn mail_is_found_by_its_words_and_shows_subject_sender_and_date() {
    let path = graph::mail_path("budget 2027");
    assert!(path.starts_with("/me/messages?$search=%22budget%202027%22&$top=5&"), "{path}");
    let recorded = r#"{
      "value": [
        { "subject": "Budget 2027", "from": { "emailAddress": { "name": "Adele Vance", "address": "adele@contoso.com" } }, "receivedDateTime": "2026-11-12T08:15:00Z", "webLink": "https://outlook.office365.com/owa/?ItemID=AAMk9&exvsurl=1&viewmodel=ReadMessageItem" },
        { "subject": "", "from": { "emailAddress": { "name": "", "address": "noreply@contoso.com" } }, "receivedDateTime": "2026-11-11T17:00:00Z", "webLink": "https://outlook.office365.com/owa/?ItemID=AAMk8" },
        { "subject": "No link, so it can't be opened" }
      ]
    }"#;
    let mail = graph::parse_mail(recorded);
    assert_eq!(mail.len(), 2);
    assert_eq!((mail[0].subject.as_str(), mail[0].from.as_str()), ("Budget 2027", "Adele Vance"));
    assert_eq!(mail[0].received, calendar::utc_secs("2026-11-12T08:15:00Z").unwrap());
    assert_eq!(mail[1].from, "noreply@contoso.com", "no name: the address");
    let rows = crate::providers::mail_rows(mail);
    assert_eq!(rows[0].kind, crate::search::RowKind::Link("https://outlook.office365.com/owa/?ItemID=AAMk9&exvsurl=1&viewmodel=ReadMessageItem".into()));
    assert!(rows[0].detail.starts_with("Adele Vance, "));
    assert!(!rows[1].name.is_empty(), "a message without a subject still has a name to show");
}

#[test]
fn microsoft_is_asked_only_for_what_is_switched_on() {
    let mut cfg = signed_in_cfg();
    assert_eq!(scopes(&cfg), ["User.Read"]);
    cfg.m365_people = true;
    assert_eq!(scopes(&cfg), ["User.Read", "User.ReadBasic.All"]);
    cfg.m365_calendar = true;
    cfg.m365_mail = true;
    assert_eq!(scopes(&cfg), ["User.Read", "User.ReadBasic.All", "Calendars.ReadWrite", "Mail.ReadBasic"]);
    // A session asks for new tokens once what's switched on changes.
    let tokens = Arc::new(CountedTokens::default());
    let session = Session::with(&signed_in_cfg(), temp_store("scopes"), Arc::new(RecordedGraph::default()), tokens.clone());
    session.set_scopes(&signed_in_cfg());
    assert_eq!(tokens.forgotten.load(Ordering::SeqCst), 0);
    session.set_scopes(&cfg);
    assert_eq!(session.scopes().len(), 4);
    assert_eq!(tokens.forgotten.load(Ordering::SeqCst), 1);
}

#[test]
fn everything_is_off_until_turned_on_and_unavailable_without_an_app_registration() {
    let cfg = Config::default();
    assert!(!cfg.m365 && !cfg.m365_calendar && !cfg.m365_people && !cfg.m365_mail);
    assert!(!signed_in(&cfg) && !panel_shown(&cfg));
    if !is_guid(BUILT_IN_CLIENT_ID) && !is_guid(&installed().client_id) {
        assert!(!available(&cfg), "no app registration built in, installed or set by policy: not available");
    }
    let mut cfg = signed_in_cfg();
    cfg.m365_calendar = true;
    assert!(panel_shown(&cfg), "the Windows 11 look has the panel");
    cfg.experience = crate::styles::Experience::Windows10;
    assert!(!panel_shown(&cfg), "the older looks don't, for now");
    cfg.experience = crate::styles::Experience::Startline;
    assert!(panel_shown(&cfg));
    cfg.m365_account_id.clear();
    assert!(!panel_shown(&cfg), "not without an account");
}

#[test]
fn a_tenant_that_blocks_consent_asks_for_an_administrator() {
    let token_error = r#"{"error":"invalid_grant","error_description":"AADSTS65001: The user or administrator has not consented to use the application with ID '11111111-2222-3333-4444-555555555555' named 'Startline'. Send an interactive authorization request for this user and resource.","error_codes":[65001]}"#;
    assert_eq!(auth::parse_granted(400, token_error), Err(SignInError::AdminConsent));
    assert_eq!(graph::error_of(400, token_error), GraphError::AdminConsent);
    assert!(needs_admin_consent("AADSTS90094: The grant requires admin permission."));
    assert!(needs_admin_consent("consent_required"));
    assert!(!needs_admin_consent("AADSTS50126: Error validating credentials due to invalid username or password."));
    let denied = r#"{"error":{"code":"Authorization_RequestDenied","message":"Insufficient privileges to complete the operation."}}"#;
    assert_eq!(graph::error_of(403, denied), GraphError::Refused(403, "Insufficient privileges to complete the operation.".into()));
    assert_eq!(graph::error_of(401, r#"{"error":{"code":"InvalidAuthenticationToken","message":"Access token has expired."}}"#), GraphError::SignInNeeded);
    // The page hears it from the session, and has the link for the administrator.
    let recorded = RecordedGraph::answering(vec![("/me", Err(GraphError::AdminConsent))]);
    let cfg = signed_in_cfg();
    let session = Session::with(&cfg, temp_store("consent"), Arc::new(recorded), Arc::new(CountedTokens::default()));
    assert_eq!(session.who(), Err(GraphError::AdminConsent));
    assert_eq!(admin_consent_link(&cfg).unwrap(), "https://login.microsoftonline.com/common/adminconsent?client_id=11111111-2222-3333-4444-555555555555");
    let granted = auth::parse_granted(200, r#"{"token_type":"Bearer","scope":"User.Read","expires_in":3599,"access_token":"eyJ0","refresh_token":"0.AR"}"#).unwrap();
    assert_eq!((granted.access.as_str(), granted.refresh.as_str(), granted.lasts), ("eyJ0", "0.AR", 3599));
}

#[test]
fn signing_out_forgets_everything() {
    let store = temp_store("sign-out");
    store.save_refresh("refresh-token-1");
    calendar::save_cache(&store.calendar_file(), &calendar::Cached { day: DAY, fetched: 1, events: vec![event((9, 0), (10, 0))] });
    assert_eq!(store.load_refresh().as_deref(), Some("refresh-token-1"));
    assert!(calendar::load_cache(&store.calendar_file()).is_some());
    // Neither is readable as plain text.
    assert!(!String::from_utf8_lossy(&std::fs::read(store.dir.join("m365-refresh.bin")).unwrap()).contains("refresh-token-1"));

    let recorded = RecordedGraph::answering(vec![("/me", Ok(r#"{"id":"u-1","displayName":"Megan Bowen","mail":"MeganB@contoso.com"}"#.into()))]);
    let tokens = Arc::new(CountedTokens::default());
    let mut cfg = signed_in_cfg();
    let session = Session::with(&cfg, store.clone(), Arc::new(recorded), tokens.clone());
    assert_eq!(session.who().unwrap().name, "Megan Bowen");
    session.sign_out(&mut cfg);
    assert!(cfg.m365_account_id.is_empty() && !signed_in(&cfg));
    assert!(session.me.lock().unwrap().is_none());
    assert!(tokens.forgotten.load(Ordering::SeqCst) >= 1, "tokens in memory are gone");
    assert!(store.load_refresh().is_none() && calendar::load_cache(&store.calendar_file()).is_none());
    assert!(!store.dir.join("m365-refresh.bin").exists() && !store.calendar_file().exists());

    // Without a session, the same.
    store.save_refresh("again");
    let mut cfg = signed_in_cfg();
    forget_everything(&mut cfg, &store);
    assert!(cfg.m365_account_id.is_empty() && store.load_refresh().is_none());
    let _ = std::fs::remove_dir_all(&store.dir);
}

fn policies(values: serde_json::Value) -> Policies {
    Policies::from_raw(&tests_raw(values))
}

#[test]
fn group_policy_beats_the_installer_which_beats_the_build() {
    const POLICY: &str = "aaaaaaaa-0000-0000-0000-000000000001";
    const INSTALLER: &str = "bbbbbbbb-0000-0000-0000-000000000002";
    const BUILT: &str = "cccccccc-0000-0000-0000-000000000003";
    let id = |policy, installer, built| pick(policy, installer, built, is_guid);
    assert_eq!(id(POLICY, INSTALLER, BUILT), Some((POLICY.to_string(), Source::Organization)));
    assert_eq!(id("", INSTALLER, BUILT), Some((INSTALLER.to_string(), Source::Installer)));
    assert_eq!(id("", "", BUILT), Some((BUILT.to_string(), Source::BuiltIn)));
    assert_eq!(id("", "", ""), None, "none of the three: not available");
    let spaced = format!(" {BUILT} ");
    assert_eq!(pick("", "", &spaced, is_guid), Some((BUILT.to_string(), Source::BuiltIn)), "spaces around it don't matter");
    // A value that can't be an app registration is passed over, not used.
    assert_eq!(id("not a guid", INSTALLER, BUILT), Some((INSTALLER.to_string(), Source::Installer)));
    assert_eq!(id("", "{oops}", BUILT), Some((BUILT.to_string(), Source::BuiltIn)));
    // The tenant goes the same way, on its own: one place can set the registration and another the tenant.
    let tenant = |policy, installer, built| pick(policy, installer, built, is_tenant).map(|(t, s)| (t, s));
    assert_eq!(tenant("", "fabrikam.onmicrosoft.com", "contoso.onmicrosoft.com"), Some(("fabrikam.onmicrosoft.com".to_string(), Source::Installer)));
    assert_eq!(tenant("", "", "contoso.onmicrosoft.com"), Some(("contoso.onmicrosoft.com".to_string(), Source::BuiltIn)));
    assert_eq!(tenant("", "", ""), None);

    // A policy, fixed or as a default, is the organization's, and wins over whatever the installer or build set.
    for raws in [(json!({ "M365ClientId": POLICY }), json!({})), (json!({}), json!({ "M365ClientId": POLICY }))] {
        let mut cfg = Config::default();
        Policies::from_raws(&tests_raw(raws.0), &tests_raw(raws.1)).enforce(&mut cfg);
        assert_eq!(client_source(&cfg), Some((POLICY.to_string(), Source::Organization)));
    }
    // Without one, it's whatever the installer or the build gave, never something saved in the settings file.
    let mut saved = Config::parse(br#"{"m365_client_id": "dddddddd-0000-0000-0000-000000000004", "m365_tenant": "evil.example"}"#).unwrap();
    saved.repair();
    assert!(saved.m365_client_id.is_empty() && saved.m365_tenant.is_empty());
    assert_ne!(client_source(&saved).map(|(_, s)| s), Some(Source::Organization));
    assert_eq!(super::page::source_label(Source::Installer), "Set by the installer");
}

#[test]
fn an_organization_sets_its_own_app_registration_and_tenant() {
    let mut cfg = signed_in_cfg();
    cfg.m365_client_id = "99999999-9999-9999-9999-999999999999".into();
    policies(json!({ "M365ClientId": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "M365TenantId": "contoso.onmicrosoft.com" })).enforce(&mut cfg);
    assert_eq!(client_id(&cfg).as_deref(), Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"), "the organization's wins over the person's");
    assert_eq!(tenant(&cfg), "contoso.onmicrosoft.com");
    assert_eq!(admin_consent_link(&cfg).unwrap(), "https://login.microsoftonline.com/contoso.onmicrosoft.com/adminconsent?client_id=aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    assert!(available(&cfg), "an organization's registration makes it available even in a build without one");
    // Values that can't be an app registration or a tenant are ignored.
    let mut cfg = signed_in_cfg();
    policies(json!({ "M365ClientId": "not a guid", "M365TenantId": "https://evil.example/x" })).enforce(&mut cfg);
    assert_eq!(client_id(&cfg).as_deref(), Some("11111111-2222-3333-4444-555555555555"));
    assert_eq!(tenant(&cfg), "common");
    assert!(is_guid("AAAAAAAA-bbbb-1111-2222-333333333333") && !is_guid("aaaa-bbbb"));
    assert!(is_tenant("f00dcafe-0000-0000-0000-000000000000") && !is_tenant("two words"));
}

#[test]
fn features_an_organization_turns_off_stay_off() {
    let mut cfg = signed_in_cfg();
    cfg.m365_people = true;
    cfg.m365_mail = true;
    cfg.m365_calendar = true;
    let fixed = policies(json!({ "M365People": 0, "M365Mail": 0 }));
    fixed.enforce(&mut cfg);
    assert!(!cfg.m365_people && !cfg.m365_mail && cfg.m365_calendar);
    assert!(!feature_on(&cfg, Feature::People) && !feature_on(&cfg, Feature::Mail) && feature_on(&cfg, Feature::Calendar));
    assert_eq!(scopes(&cfg), ["User.Read", "Calendars.ReadWrite"], "nothing is asked for a feature that's off");
    assert!(fixed.is_set("M365People"));
    // The account itself off: no one is signed in, whatever was saved.
    policies(json!({ "M365Account": 0 })).enforce(&mut cfg);
    assert!(!signed_in(&cfg) && !feature_on(&cfg, Feature::Calendar) && !panel_shown(&cfg));
}

#[test]
fn secrets_and_the_sign_in_address_are_encoded_properly() {
    let sealed = auth::protect(b"secret").unwrap();
    assert_ne!(sealed, b"secret");
    assert_eq!(auth::unprotect(&sealed).unwrap(), b"secret");
    // RFC 7636, appendix B.
    assert_eq!(auth::pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    assert_eq!(auth::base64url(b"\xfb\xff"), "-_8");
    let back = auth::parse_redirect("GET /?code=0.AR%2Fx&state=s1&session_state=z HTTP/1.1");
    assert!(back.contains(&("code".into(), "0.AR/x".into())) && back.contains(&("state".into(), "s1".into())));
    let refused = auth::parse_redirect("GET /?error=consent_required&error_description=AADSTS65001%3A+needs+admin HTTP/1.1");
    assert!(refused.contains(&("error_description".into(), "AADSTS65001: needs admin".into())));
}
