//! The calendar panel beside the Windows 11 and Startline menus: a day's timeline with its events, all-day events on
//! top, the time now as a line, and the day's header with buttons for the day before and after, today, and a new
//! event.

use super::calendar::{self, Answer, Cached, Day, Event, When};
use super::graph::GraphError;
use super::Session;
use crate::i18n::{tr, tx};
use crate::ui::{Intent, Menu};
use crate::widgets::*;
use egui::*;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the day is read again while the menu is open.
const REFRESH: Duration = Duration::from_secs(5 * 60);
const HOUR: f32 = 44.0;
const LABELS: f32 = 42.0;

const WEEKDAYS: [&str; 7] = [tx("Monday"), tx("Tuesday"), tx("Wednesday"), tx("Thursday"), tx("Friday"), tx("Saturday"), tx("Sunday")];

/// A new event being written.
#[derive(Default)]
pub struct Form {
    title: String,
    start: String,
    end: String,
    problem: Option<String>,
}

type Answered = Arc<Mutex<Option<(Day, Result<Vec<Event>, GraphError>)>>>;

pub struct Calendar {
    pub day: Day,
    shown: Option<Cached>,
    loading: bool,
    problem: Option<String>,
    answered: Answered,
    done: Arc<Mutex<Option<Result<(), GraphError>>>>,
    last_read: Option<Instant>,
    form: Option<Form>,
    cache_read: bool,
    zone: String,
    scrolled: bool,
}

impl Default for Calendar {
    fn default() -> Self {
        Calendar { day: Day::today(), shown: None, loading: false, problem: None, answered: Arc::default(), done: Arc::default(), last_read: None, form: None, cache_read: false, zone: String::new(), scrolled: false }
    }
}

pub fn problem_text(error: &GraphError) -> String {
    match error {
        GraphError::SignInNeeded => tr("Sign in again on the Microsoft 365 page of Settings.").into(),
        GraphError::AdminConsent => tr("Your organization has to approve Startline first.").into(),
        GraphError::Refused(_, message) => crate::trf!("Microsoft 365 said: {message}", message = message),
        GraphError::Offline(_) => tr("Microsoft 365 can't be reached.").into(),
    }
}

impl Calendar {
    /// The menu opened: today again, read afresh.
    pub fn opened(&mut self) {
        if self.day != Day::today() {
            self.day = Day::today();
            self.shown = None;
        }
        self.last_read = None;
        self.scrolled = false;
    }

    /// Example events for a picture of the panel, without an account.
    pub fn demo(&mut self) {
        let day = Day::today();
        let at = |minute| When { day, minute };
        let event = |subject: &str, start, end, join: bool, answer| Event {
            id: String::new(),
            subject: subject.into(),
            start: at(start),
            end: at(end),
            all_day: false,
            location: String::new(),
            web_link: "https://outlook.office.com/calendar".into(),
            join_url: if join { "https://teams.microsoft.com/l/meetup-join/demo".into() } else { String::new() },
            answer,
            organizer: answer == Answer::Organizer,
            cancelled: false,
        };
        let mut all_day = event(tr("Team offsite"), 0, 0, false, Answer::Accepted);
        all_day.all_day = true;
        all_day.end = When { day: day.offset(1), minute: 0 };
        let events = vec![
            all_day,
            event(tr("Stand-up"), 9 * 60, 9 * 60 + 15, true, Answer::Accepted),
            event(tr("Design review"), 10 * 60, 11 * 60, true, Answer::NotResponded),
            event(tr("Lunch with Sam"), 12 * 60, 13 * 60, false, Answer::Organizer),
            event(tr("Budget call"), 12 * 60 + 30, 13 * 60 + 30, true, Answer::Tentative),
            event(tr("Focus time"), 14 * 60, 16 * 60, false, Answer::Organizer),
        ];
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.shown = Some(Cached { day, fetched: now, events });
        self.cache_read = true;
        self.last_read = Some(Instant::now());
    }

    fn read(&mut self, session: &Arc<Session>, ctx: &Context) {
        if self.loading {
            return;
        }
        if self.zone.is_empty() {
            self.zone = calendar::windows_zone();
        }
        self.loading = true;
        self.last_read = Some(Instant::now());
        let (day, zone, session, slot, ctx) = (self.day, self.zone.clone(), session.clone(), self.answered.clone(), ctx.clone());
        std::thread::spawn(move || {
            crate::platform::com_init();
            let answer = match calendar::day_bounds(day) {
                Some(bounds) => calendar::fetch_day(&*session.graph, bounds, &zone, &calendar::utc_to_local),
                None => Ok(Vec::new()),
            };
            *slot.lock().unwrap() = Some((day, answer));
            ctx.request_repaint();
        });
    }

    /// Takes in answers, and reads the day again when it's time.
    pub fn tick(&mut self, session: Option<&Arc<Session>>, ctx: &Context) {
        let Some(session) = session else { return };
        if !self.cache_read {
            self.cache_read = true;
            if let Some(cached) = calendar::load_cache(&session.store.calendar_file()) {
                if cached.day == self.day {
                    self.shown = Some(cached);
                }
            }
        }
        if let Some((day, answer)) = self.answered.lock().unwrap().take() {
            self.loading = false;
            if day == self.day {
                match answer {
                    Ok(events) => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                        let cached = Cached { day, fetched: now, events };
                        if day == Day::today() {
                            calendar::save_cache(&session.store.calendar_file(), &cached);
                        }
                        self.shown = Some(cached);
                        self.problem = None;
                    }
                    Err(error) => self.problem = Some(problem_text(&error)),
                }
            }
        }
        if let Some(done) = self.done.lock().unwrap().take() {
            match done {
                Ok(()) => self.last_read = None,
                Err(error) => self.problem = Some(problem_text(&error)),
            }
        }
        if self.last_read.is_none_or(|at| at.elapsed() > REFRESH) {
            self.read(session, ctx);
        }
        // The line for now moves on, and the day is read again on time.
        ctx.request_repaint_after(Duration::from_secs(60));
    }

    fn change_day(&mut self, day: Day) {
        self.day = day;
        self.shown = None;
        self.last_read = None;
        self.scrolled = false;
    }

    fn act(&self, session: &Arc<Session>, ctx: &Context, job: impl FnOnce(&Session) -> Result<(), GraphError> + Send + 'static) {
        let (session, slot, ctx) = (session.clone(), self.done.clone(), ctx.clone());
        std::thread::spawn(move || {
            crate::platform::com_init();
            *slot.lock().unwrap() = Some(job(&session));
            ctx.request_repaint();
        });
    }
}

/// A small square button with a symbol, lit when the pointer is on it.
fn glyph_button(ui: &mut Ui, rect: Rect, glyph: char, tip: &str, color: Color32, pal: Pal) -> bool {
    glyph_response(ui, rect, glyph, tip, color, pal).clicked()
}

/// The same button, for a menu to open from it.
fn glyph_response(ui: &mut Ui, rect: Rect, glyph: char, tip: &str, color: Color32, pal: Pal) -> Response {
    let response = ui.interact(rect, ui.scope_id().with(("glyph", rect.min.x as i32, rect.min.y as i32, glyph)), Sense::click()).on_hover_text(tip);
    if response.hovered() {
        ui.painter().rect_filled(rect, pal.radius.min(4), pal.hover);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, icon_font(12.0), color);
    response
}

/// The answers to an invitation, as menu lines.
fn answer_menu(ui: &mut Ui, id: &str, answers: &mut Vec<(String, Answer)>) {
    for &(_, tip, answer) in ANSWERS.iter().rev() {
        if ui.button(tr(tip)).clicked() {
            answers.push((id.to_string(), answer));
            ui.close();
        }
    }
}

/// The answers to an invitation, right to left as their buttons stand.
const ANSWERS: [(char, &str, Answer); 3] = [('\u{E711}', tx("Decline"), Answer::Declined), ('\u{E897}', tx("Tentative"), Answer::Tentative), ('\u{E8FB}', tx("Accept"), Answer::Accepted)];

/// A small button with words, its width fitting them.
fn word_button(ui: &mut Ui, at: Pos2, text: &str, pal: Pal) -> (Rect, bool) {
    let galley = line_galley(ui, text, FontId::proportional(12.0), pal.text, 200.0);
    let rect = Rect::from_min_size(at, vec2(galley.size().x + 16.0, 24.0));
    let response = ui.interact(rect, ui.scope_id().with(("word", text)), Sense::click());
    ui.painter().rect_filled(rect, pal.radius.min(6), if response.hovered() { pal.hover } else { mix(pal.surface, pal.bg, 0.3) });
    ui.painter().rect_stroke(rect, pal.radius.min(6), Stroke::new(1.0, pal.line), StrokeKind::Inside);
    ui.painter().galley(pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0), galley, pal.text);
    (rect, response.clicked())
}

impl Menu {
    /// The calendar panel, in `rect` on the right of the menu.
    pub(crate) fn draw_calendar(&mut self, ui: &mut Ui, rect: Rect, pal: Pal) {
        let session = self.m365.clone();
        ui.painter().vline(rect.left(), rect.y_range(), Stroke::new(1.0, pal.line));
        let inner = rect.shrink2(vec2(12.0, 14.0));
        let today = Day::today();
        let day = self.calendar.day;

        // The header: which day, and the buttons.
        let header = Rect::from_min_size(inner.min, vec2(inner.width(), 40.0));
        let title = if day == today { tr("Today").to_string() } else { tr(WEEKDAYS[day.weekday() as usize]).to_string() };
        ui.painter().text(header.left_top(), Align2::LEFT_TOP, &title, semibold(15.0), pal.text);
        ui.painter().text(header.left_top() + vec2(0.0, 21.0), Align2::LEFT_TOP, crate::i18n::fmt_date(day.year, day.month, day.day), FontId::proportional(12.0), pal.sub);
        let size = vec2(26.0, 26.0);
        let mut x = header.right();
        let mut button = |ui: &mut Ui, glyph: char, tip: &str| {
            x -= size.x + 2.0;
            glyph_button(ui, Rect::from_min_size(pos2(x, header.top() + 4.0), size), glyph, tip, pal.text, pal)
        };
        let new_event = button(ui, '\u{E710}', tr("New event"));
        let next = button(ui, '\u{E76C}', tr("Next day"));
        let back_today = day != today && button(ui, '\u{E8D1}', tr("Back to today"));
        let previous = button(ui, '\u{E76B}', tr("Previous day"));
        if previous {
            self.calendar.change_day(day.offset(-1));
        }
        if next {
            self.calendar.change_day(day.offset(1));
        }
        if back_today {
            self.calendar.change_day(today);
        }
        if new_event && self.calendar.form.is_none() {
            let now = crate::platform::local_time(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()));
            let start = ((now.hour + 1).min(23)) * 60;
            self.calendar.form = Some(Form { title: String::new(), start: crate::i18n::fmt_time(start / 60, 0), end: crate::i18n::fmt_time(start / 60, 30), problem: None });
        }
        let mut top = header.bottom() + 6.0;

        // A new event: its title, start and end on the day shown.
        let mut create = None;
        let mut close_form = false;
        if let Some(form) = self.calendar.form.as_mut() {
            let area = Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), if form.problem.is_some() { 118.0 } else { 100.0 }));
            ui.painter().rect_filled(area, pal.radius, mix(pal.surface, pal.bg, 0.2));
            let mut child = ui.new_child(UiBuilder::new().max_rect(area.shrink(8.0)).layout(Layout::top_down(Align::Min)));
            child.spacing_mut().item_spacing.y = 6.0;
            text_box(&mut child, None, TextEdit::singleline(&mut form.title).hint_text(tr("Title")).desired_width(f32::INFINITY));
            child.horizontal(|ui| {
                ui.label(RichText::new(tr("From")).size(12.0).color(pal.sub));
                text_box(ui, None, TextEdit::singleline(&mut form.start).desired_width(48.0));
                ui.label(RichText::new(tr("to")).size(12.0).color(pal.sub));
                text_box(ui, None, TextEdit::singleline(&mut form.end).desired_width(48.0));
            });
            if let Some(problem) = &form.problem {
                child.label(RichText::new(problem).size(12.0).color(Color32::from_rgb(222, 120, 96)));
            }
            let buttons_y = area.bottom() - 30.0;
            let (create_rect, create_clicked) = word_button(ui, pos2(area.left() + 8.0, buttons_y), tr("Create"), pal);
            let (_, cancel_clicked) = word_button(ui, pos2(create_rect.right() + 6.0, buttons_y), tr("Cancel"), pal);
            if create_clicked {
                match (calendar::parse_clock(&form.start), calendar::parse_clock(&form.end)) {
                    _ if form.title.trim().is_empty() => form.problem = Some(tr("The event needs a title.").into()),
                    (Some(start), Some(end)) if end > start => create = Some((form.title.clone(), When { day, minute: start }, When { day, minute: end })),
                    _ => form.problem = Some(tr("Write the times as 9:30, the end after the start.").into()),
                }
            }
            close_form = cancel_clicked || create.is_some();
            top = area.bottom() + 8.0;
        }
        if close_form {
            self.calendar.form = None;
        }
        if let (Some((title, start, end)), Some(session)) = (create, session.as_ref()) {
            let zone = if self.calendar.zone.is_empty() { calendar::windows_zone() } else { self.calendar.zone.clone() };
            self.calendar.act(session, ui.ctx(), move |s| calendar::create(&*s.graph, &title, start, end, &zone));
        }

        let events: Vec<Event> = self.calendar.shown.as_ref().filter(|c| c.day == day).map(|c| c.events.clone()).unwrap_or_default();
        let mut intents: Vec<Intent> = Vec::new();
        let mut answers: Vec<(String, Answer)> = Vec::new();
        let teams = self.teams_installed;

        // All-day events, one line each.
        for index in calendar::all_day_on(&events, day) {
            let event = &events[index];
            let line = Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), 24.0));
            let response = ui.interact(line, ui.scope_id().with(("all-day", index)), Sense::click());
            ui.painter().rect_filled(line, pal.radius.min(6), mix(pal.surface, pal.accent, if response.hovered() { 0.35 } else { 0.22 }));
            let text = line_galley(ui, &event.subject, FontId::proportional(12.5), pal.text, line.width() - 16.0);
            ui.painter().galley(pos2(line.left() + 8.0, line.center().y - text.size().y / 2.0), text, pal.text);
            if response.clicked() && !event.web_link.is_empty() {
                intents.push(Intent::Link(event.web_link.clone()));
            }
            top = line.bottom() + 4.0;
        }

        // The timeline.
        let footer_h = 18.0;
        let timeline = Rect::from_min_max(pos2(inner.left(), top + 4.0), pos2(inner.right(), inner.bottom() - footer_h - 4.0));
        let now = crate::platform::local_time(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()));
        let now_minute = now.hour * 60 + now.minute;
        let scroll_to = if self.calendar.scrolled {
            None
        } else {
            self.calendar.scrolled = true;
            let first = calendar::place(&events, day).first().map(|p| p.top);
            let focus = if day == today { now_minute.saturating_sub(60) } else { first.unwrap_or(8 * 60).saturating_sub(30) };
            Some(focus as f32 / 60.0 * HOUR)
        };
        let mut child = ui.new_child(UiBuilder::new().max_rect(timeline).layout(Layout::top_down(Align::Min)));
        child.set_clip_rect(timeline);
        let mut scroll = ScrollArea::vertical().id_salt("calendar-day").auto_shrink([false, false]);
        if let Some(offset) = scroll_to {
            scroll = scroll.vertical_scroll_offset(offset);
        }
        quiet_scroll(&mut child, scroll, Id::unique("calendar-day-bar"), |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), HOUR * 24.0), Sense::hover());
            let y_of = |minute: u32| area.top() + minute as f32 / 60.0 * HOUR;
            for hour in 0..24u32 {
                let y = y_of(hour * 60);
                ui.painter().hline(area.left() + LABELS - 4.0..=area.right(), y, Stroke::new(1.0, mix(pal.line, pal.bg, 0.3)));
                ui.painter().text(pos2(area.left(), y + 2.0), Align2::LEFT_TOP, crate::i18n::fmt_time(hour, 0), FontId::proportional(11.0), pal.sub);
            }
            let lane = Rect::from_min_max(pos2(area.left() + LABELS, area.top()), area.max);
            for placed in calendar::place(&events, day) {
                let event = &events[placed.index];
                let width = lane.width() / placed.columns as f32;
                let block = Rect::from_min_max(pos2(lane.left() + width * placed.column as f32 + 1.0, y_of(placed.top) + 1.0), pos2(lane.left() + width * (placed.column + 1) as f32 - 1.0, y_of(placed.bottom) - 1.0));
                let faded = event.cancelled || event.answer == Answer::Declined;
                let response = ui.interact(block, ui.scope_id().with(("event", placed.index)), Sense::click());
                let strength = if response.hovered() { 0.42 } else if event.is_invitation() || event.answer == Answer::Tentative { 0.14 } else { 0.28 };
                ui.painter().rect_filled(block, pal.radius.min(6), mix(pal.surface, pal.accent, if faded { 0.08 } else { strength }));
                ui.painter().vline(block.left() + 1.5, block.top() + 2.0..=block.bottom() - 2.0, Stroke::new(3.0, if faded { pal.sub } else { pal.accent }));
                let ink = if faded { pal.sub } else { pal.text };
                // Buttons along the top right: join, and the answers to an invitation.
                let mut right = block.right() - 2.0;
                let mut clicked_button = false;
                let mut small = |ui: &mut Ui, glyph: char, tip: &str| {
                    right -= 22.0;
                    glyph_response(ui, Rect::from_min_size(pos2(right, block.top() + 2.0), vec2(20.0, 20.0)), glyph, tip, ink, pal)
                };
                if event.is_invitation() {
                    if block.width() > 120.0 {
                        for (glyph, tip, answer) in ANSWERS {
                            if small(ui, glyph, tr(tip)).clicked() {
                                answers.push((event.id.clone(), answer));
                                clicked_button = true;
                            }
                        }
                    } else {
                        // Beside another event there's no room for three buttons: one opens them as a menu.
                        let more = small(ui, '\u{E712}', tr("Answer the invitation"));
                        clicked_button |= more.clicked();
                        Popup::menu(&more).show(|ui| answer_menu(ui, &event.id, &mut answers));
                    }
                    // However narrow, a right click on it answers too.
                    response.context_menu(|ui| answer_menu(ui, &event.id, &mut answers));
                }
                if !event.join_url.is_empty() && !faded && small(ui, '\u{E714}', tr("Join")).clicked() {
                    intents.push(Intent::Link(super::teams_link(&event.join_url, teams)));
                    clicked_button = true;
                }
                let text_width = (right - block.left() - 10.0).max(20.0);
                let subject = line_galley(ui, if event.subject.is_empty() { tr("(no title)") } else { &event.subject }, semibold(12.0), ink, text_width);
                let subject_h = subject.size().y;
                ui.painter().galley(pos2(block.left() + 8.0, block.top() + 3.0), subject, ink);
                if block.height() > 34.0 {
                    let mut when = format!("{}-{}", crate::i18n::fmt_time(event.start.minute / 60, event.start.minute % 60), crate::i18n::fmt_time(event.end.minute / 60, event.end.minute % 60));
                    if !event.location.is_empty() {
                        when = format!("{when}, {}", event.location);
                    }
                    let detail = line_galley(ui, &when, FontId::proportional(11.0), pal.sub, block.width() - 12.0);
                    ui.painter().galley(pos2(block.left() + 8.0, block.top() + 4.0 + subject_h), detail, pal.sub);
                }
                if response.clicked() && !clicked_button && !event.web_link.is_empty() {
                    intents.push(Intent::Link(event.web_link.clone()));
                }
            }
            if day == today {
                let y = y_of(now_minute);
                ui.painter().hline(lane.left() - 4.0..=lane.right(), y, Stroke::new(1.5, Color32::from_rgb(222, 96, 80)));
                ui.painter().circle_filled(pos2(lane.left() - 4.0, y), 3.5, Color32::from_rgb(222, 96, 80));
            }
        });

        // When the day was last read, and anything that went wrong.
        let footer = Rect::from_min_max(pos2(inner.left(), inner.bottom() - footer_h), inner.max);
        let note = if let Some(problem) = &self.calendar.problem {
            match self.calendar.shown.as_ref().filter(|c| c.day == day) {
                Some(cached) => crate::trf!("{problem} Last updated {time}.", problem = problem, time = crate::i18n::fmt_unix(cached.fetched)),
                None => problem.clone(),
            }
        } else if self.calendar.loading && self.calendar.shown.is_none() {
            tr("Reading your calendar...").into()
        } else if let Some(cached) = self.calendar.shown.as_ref() {
            let local = crate::platform::local_time(cached.fetched);
            crate::trf!("Updated {time}", time = crate::i18n::fmt_time(local.hour, local.minute))
        } else {
            String::new()
        };
        let note = line_galley(ui, &note, FontId::proportional(11.0), pal.sub, footer.width());
        ui.painter().galley(pos2(footer.left(), footer.center().y - note.size().y / 2.0), note, pal.sub);

        if let Some(session) = session.as_ref() {
            for (id, answer) in answers {
                self.calendar.act(session, ui.ctx(), move |s| calendar::respond(&*s.graph, &id, answer));
            }
        }
        for intent in intents {
            self.intents.push(intent);
        }
    }
}
