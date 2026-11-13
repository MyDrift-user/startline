//! What's playing, through Windows' own media controls: the same ones the volume flyout and the lock screen show, so
//! Spotify, browsers and any other player that tells Windows work without keys or a sign-in. A helper thread asks
//! Windows when it says something changed, and the menu draws a small card with the cover, the title, the artist,
//! the app, and play or pause, previous and next.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::core::HSTRING;

/// A cover, small enough to keep: about the size it's drawn at on a sharp screen.
pub struct Cover {
    pub key: String,
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// One app that plays, or is paused.
#[derive(Clone)]
pub struct Playing {
    pub app_id: String,
    pub app: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub can_toggle: bool,
    pub can_previous: bool,
    pub can_next: bool,
    pub cover: Option<Arc<Cover>>,
}

enum Job {
    Changed,
    Toggle(String),
    Previous(String),
    Next(String),
}

/// The helper thread and what it last found. Nothing runs while it's switched off.
pub struct Media {
    tx: Sender<Job>,
    on: Arc<AtomicBool>,
    found: Arc<Mutex<Vec<Playing>>>,
    /// Which of several apps the card shows, by its app id.
    pub chosen: Option<String>,
    /// Covers turned into pictures the menu can draw.
    pub textures: HashMap<String, egui::TextureHandle>,
}

impl Media {
    pub fn start(ctx: egui::Context) -> Media {
        let (tx, rx) = mpsc::channel::<Job>();
        let on = Arc::new(AtomicBool::new(false));
        let found: Arc<Mutex<Vec<Playing>>> = Arc::default();
        let (switch, slot, events) = (on.clone(), found.clone(), tx.clone());
        let _ = std::thread::Builder::new().name("media".into()).spawn(move || {
            // Windows' answers and its word that something changed come on threads of their own; this thread has no
            // message loop for them to wait on.
            let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
            let mut watcher: Option<Watcher> = None;
            let mut covers: HashMap<String, Arc<Cover>> = HashMap::new();
            loop {
                let job = rx.recv_timeout(Duration::from_secs(if watcher.is_some() { 5 } else { 1 }));
                if matches!(job, Err(RecvTimeoutError::Disconnected)) {
                    return;
                }
                if !switch.load(Ordering::Relaxed) {
                    if watcher.take().is_some() {
                        slot.lock().unwrap().clear();
                        covers.clear();
                        ctx.request_repaint();
                    }
                    continue;
                }
                if watcher.is_none() {
                    watcher = Watcher::start(events.clone());
                }
                let Some(watching) = watcher.as_mut() else { continue };
                match job {
                    Ok(Job::Toggle(id)) => watching.act(&id, |s| s.TryTogglePlayPauseAsync().and_then(|a| a.join()).map(|_| ())),
                    Ok(Job::Previous(id)) => watching.act(&id, |s| s.TrySkipPreviousAsync().and_then(|a| a.join()).map(|_| ())),
                    Ok(Job::Next(id)) => watching.act(&id, |s| s.TrySkipNextAsync().and_then(|a| a.join()).map(|_| ())),
                    _ => {}
                }
                let now = watching.read(&mut covers);
                // Covers of what no longer plays aren't kept.
                covers.retain(|key, _| now.iter().any(|p| p.cover.as_ref().is_some_and(|c| &c.key == key)));
                *slot.lock().unwrap() = now;
                ctx.request_repaint();
            }
        });
        Media { tx, on, found, chosen: None, textures: HashMap::new() }
    }

    /// Switches the helper on or off with the setting.
    pub fn set_on(&self, on: bool) {
        if self.on.swap(on, Ordering::Relaxed) != on {
            let _ = self.tx.send(Job::Changed);
        }
    }

    /// For a picture: a made-up song in two apps, without asking Windows.
    pub fn show_demo(&self) {
        let song = |app: &str, title: &str, artist: &str| Playing {
            app_id: app.to_string(),
            app: app.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            playing: true,
            can_toggle: true,
            can_previous: true,
            can_next: true,
            cover: None,
        };
        *self.found.lock().unwrap() = vec![song("Spotify", "Northern Lights", "The Quiet Hours"), song("Firefox", "Morning Podcast", "")]; // i18n-ignore: an example song for pictures
    }

    /// What plays now; empty while switched off or nothing does.
    pub fn playing(&self) -> Vec<Playing> {
        self.found.lock().unwrap().clone()
    }

    pub fn toggle(&self, id: &str) {
        let _ = self.tx.send(Job::Toggle(id.to_string()));
    }

    pub fn previous(&self, id: &str) {
        let _ = self.tx.send(Job::Previous(id.to_string()));
    }

    pub fn next(&self, id: &str) {
        let _ = self.tx.send(Job::Next(id.to_string()));
    }
}

/// Windows' list of players, with a word from each when something changes.
struct Watcher {
    manager: Manager,
    events: Sender<Job>,
    /// The players listened to, with the tokens to stop listening.
    sessions: Vec<(String, Session, i64, i64)>,
    /// The tokens to stop hearing about the list of players and which one is current.
    tokens: (i64, i64),
}

impl Watcher {
    fn start(events: Sender<Job>) -> Option<Watcher> {
        let manager = Manager::RequestAsync().ok()?.join().ok()?;
        let tell = events.clone();
        let listed = manager
            .SessionsChanged(&TypedEventHandler::new(move |_, _| {
                let _ = tell.send(Job::Changed);
                Ok(())
            }))
            .unwrap_or_default();
        let tell = events.clone();
        let current = manager
            .CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                let _ = tell.send(Job::Changed);
                Ok(())
            }))
            .unwrap_or_default();
        Some(Watcher { manager, events, sessions: Vec::new(), tokens: (listed, current) })
    }

    fn act(&self, id: &str, what: impl Fn(&Session) -> windows::core::Result<()>) {
        if let Some((_, session, ..)) = self.sessions.iter().find(|(app, ..)| app == id) {
            let _ = what(session);
        }
    }

    /// Every player that plays or is paused, the one Windows thinks of as current first.
    fn read(&mut self, covers: &mut HashMap<String, Arc<Cover>>) -> Vec<Playing> {
        let current = self.manager.GetCurrentSession().ok().and_then(|s| s.SourceAppUserModelId().ok()).map(|id| id.to_string()).unwrap_or_default();
        let sessions: Vec<Session> = self.manager.GetSessions().map(|list| list.into_iter().collect()).unwrap_or_default();
        // Listen to new players; forget ones that went.
        let ids: Vec<String> = sessions.iter().map(|s| s.SourceAppUserModelId().map(|i| i.to_string()).unwrap_or_default()).collect();
        self.sessions.retain(|(id, session, properties, playback)| {
            let keep = ids.contains(id);
            if !keep {
                let _ = session.RemoveMediaPropertiesChanged(*properties);
                let _ = session.RemovePlaybackInfoChanged(*playback);
            }
            keep
        });
        for (session, id) in sessions.iter().zip(&ids) {
            if self.sessions.iter().any(|(known, ..)| known == id) {
                continue;
            }
            let tell = self.events.clone();
            let properties = session
                .MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = tell.send(Job::Changed);
                    Ok(())
                }))
                .unwrap_or_default();
            let tell = self.events.clone();
            let playback = session
                .PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = tell.send(Job::Changed);
                    Ok(())
                }))
                .unwrap_or_default();
            self.sessions.push((id.clone(), session.clone(), properties, playback));
        }
        let mut out: Vec<Playing> = sessions.iter().zip(&ids).filter_map(|(session, id)| playing(session, id, covers)).collect();
        out.sort_by_key(|p| (p.app_id != current, !p.playing));
        out
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.manager.RemoveSessionsChanged(self.tokens.0);
        let _ = self.manager.RemoveCurrentSessionChanged(self.tokens.1);
        for (_, session, properties, playback) in &self.sessions {
            let _ = session.RemoveMediaPropertiesChanged(*properties);
            let _ = session.RemovePlaybackInfoChanged(*playback);
        }
    }
}

fn playing(session: &Session, id: &str, covers: &mut HashMap<String, Arc<Cover>>) -> Option<Playing> {
    let info = session.GetPlaybackInfo().ok()?;
    let status = info.PlaybackStatus().ok()?;
    if status != Status::Playing && status != Status::Paused {
        return None;
    }
    let properties = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let title = properties.Title().map(|t| t.to_string()).unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let artist = properties.Artist().map(|a| a.to_string()).unwrap_or_default();
    let controls = info.Controls().ok();
    let enabled = |f: fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackControls) -> windows::core::Result<bool>| controls.as_ref().and_then(|c| f(c).ok()).unwrap_or(false);
    let key = format!("{id}\n{title}\n{artist}");
    let cover = match covers.get(&key) {
        Some(cover) => Some(cover.clone()),
        None => {
            let read = properties.Thumbnail().ok().and_then(|reference| cover_of(&reference, &key)).map(Arc::new);
            if let Some(cover) = &read {
                covers.insert(key.clone(), cover.clone());
            }
            read
        }
    };
    Some(Playing {
        app_id: id.to_string(),
        app: app_name(id),
        title,
        artist,
        playing: status == Status::Playing,
        can_toggle: enabled(|c| c.IsPlayPauseToggleEnabled()) || enabled(|c| c.IsPlayEnabled()) || enabled(|c| c.IsPauseEnabled()),
        can_previous: enabled(|c| c.IsPreviousEnabled()),
        can_next: enabled(|c| c.IsNextEnabled()),
        cover,
    })
}

/// The cover, turned into pixels at most 96 points on its longer side.
fn cover_of(reference: &windows::Storage::Streams::IRandomAccessStreamReference, key: &str) -> Option<Cover> {
    let stream = reference.OpenReadAsync().ok()?.join().ok()?;
    let decoder = BitmapDecoder::CreateAsync(&stream).ok()?.join().ok()?;
    let (width, height) = (decoder.PixelWidth().ok()?.max(1), decoder.PixelHeight().ok()?.max(1));
    let scale = (96.0 / width.max(height) as f32).min(1.0);
    let (w, h) = (((width as f32 * scale).round() as u32).max(1), ((height as f32 * scale).round() as u32).max(1));
    let transform = BitmapTransform::new().ok()?;
    transform.SetScaledWidth(w).ok()?;
    transform.SetScaledHeight(h).ok()?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(BitmapPixelFormat::Rgba8, BitmapAlphaMode::Straight, &transform, ExifOrientationMode::RespectExifOrientation, ColorManagementMode::DoNotColorManage)
        .ok()?
        .join()
        .ok()?
        .DetachPixelData()
        .ok()?;
    let rgba = pixels.to_vec();
    (rgba.len() == (w * h * 4) as usize).then(|| Cover { key: key.to_string(), size: [w as usize, h as usize], rgba })
}

/// What plays on this PC now, for checking on a real machine. Only reads.
pub fn probe_report() -> String {
    let (tx, _rx) = mpsc::channel();
    let Some(mut watcher) = Watcher::start(tx) else { return "media controls: unavailable\n".into() }; // i18n-ignore: probe report for tests
    let found = watcher.read(&mut HashMap::new());
    let mut out = format!("media sessions playing or paused={}\n", found.len()); // i18n-ignore: probe report for tests
    for playing in &found {
        out.push_str(&format!("  app={} playing={} cover={} controls={}/{}/{}\n", playing.app, playing.playing, playing.cover.is_some(), playing.can_previous, playing.can_toggle, playing.can_next)); // i18n-ignore: probe report for tests
    }
    out
}

/// What the app is called: the Store's name for it, or its program's name.
pub fn app_name(id: &str) -> String {
    let known = [("308046B0AF4A39CB", "Firefox"), ("MSEdge", "Microsoft Edge"), ("Chrome", "Google Chrome")]; // i18n-ignore: app names
    if let Some((_, name)) = known.iter().find(|(known, _)| known.eq_ignore_ascii_case(id)) {
        return name.to_string();
    }
    if let Ok(name) = windows::ApplicationModel::AppInfo::GetFromAppUserModelId(&HSTRING::from(id)).and_then(|info| info.DisplayInfo()).and_then(|d| d.DisplayName()) {
        if !name.is_empty() {
            return name.to_string();
        }
    }
    program_name(id)
}

/// "Spotify.exe" is Spotify; "C:\...\vlc.exe" is Vlc.
fn program_name(id: &str) -> String {
    let file = id.rsplit(['\\', '/']).next().unwrap_or(id);
    let stem = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
    let stem = stem.split('!').next().unwrap_or(stem);
    let mut chars = stem.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_are_named_by_their_program() {
        assert_eq!(program_name("Spotify.exe"), "Spotify");
        assert_eq!(program_name("C:\\Program Files\\VideoLAN\\VLC\\vlc.exe"), "Vlc");
        assert_eq!(program_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "SpotifyAB.SpotifyMusic_zpdnekdrzrea0");
        assert_eq!(app_name("308046B0AF4A39CB"), "Firefox");
    }
}
