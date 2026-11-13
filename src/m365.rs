//! Microsoft 365: one optional work or school account, for a calendar beside the menu and for people and mail in
//! search. Everything is off until the person or an administrator turns it on, Microsoft is asked only for what's
//! switched on, and nothing is sent anywhere but Microsoft's sign-in and Graph.

#[path = "m365_auth.rs"]
pub mod auth;
#[path = "m365_calendar.rs"]
pub mod calendar;
#[path = "m365_graph.rs"]
pub mod graph;
#[path = "m365_page.rs"]
pub mod page;
#[path = "m365_panel.rs"]
pub mod panel;
#[cfg(test)]
#[path = "m365_tests.rs"]
mod tests;

use crate::config::Config;
use crate::styles::Experience;
use auth::SignInError;
use graph::{Graph, GraphError, HttpGraph, Me, Tokens};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The app registration built into this copy, from STARTLINE_M365_CLIENT_ID when it was built (package.ps1
/// -M365ClientId, or the release workflow's repository variable). A public client with no secret: its ID isn't one,
/// but it's never written into the source either.
pub const BUILT_IN_CLIENT_ID: &str = match option_env!("STARTLINE_M365_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// The tenant built into this copy, from STARTLINE_M365_TENANT_ID when it was built.
pub const BUILT_IN_TENANT_ID: &str = match option_env!("STARTLINE_M365_TENANT_ID") {
    Some(id) => id,
    None => "",
};

/// Where the app registration or tenant in use was set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// Group Policy, fixed or as a default.
    Organization,
    /// The MSI's M365CLIENTID or setup.exe's /M365CLIENTID=, in Startline's own registry key.
    Installer,
    /// Built into the program.
    BuiltIn,
}

/// The first usable value: Group Policy's, then the installer's, then the built-in one.
pub fn pick(policy: &str, installer: &str, built_in: &str, usable: fn(&str) -> bool) -> Option<(String, Source)> {
    [(policy, Source::Organization), (installer, Source::Installer), (built_in, Source::BuiltIn)].into_iter().find(|(value, _)| usable(value)).map(|(value, source)| (value.trim().to_string(), source))
}

/// What the installer wrote into Startline's own key: HKLM\Software\Startline for a for-this-PC install,
/// HKCU\Software\Startline for one account. The PC's wins. Read once: an installer closes Startline anyway.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Installed {
    pub client_id: String,
    pub tenant: String,
}

pub fn installed() -> &'static Installed {
    static INSTALLED: std::sync::OnceLock<Installed> = std::sync::OnceLock::new();
    INSTALLED.get_or_init(|| {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        let read = |name: &str| [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER].into_iter().find_map(|root| crate::platform::reg_text(root, "Software\\Startline", name).filter(|v| !v.trim().is_empty())).unwrap_or_default();
        Installed { client_id: read("M365ClientId"), tenant: read("M365TenantId") }
    })
}

/// The app registration in use and where it was set: Group Policy, then the installer, then the build.
pub fn client_source(cfg: &Config) -> Option<(String, Source)> {
    pick(&cfg.m365_client_id, &installed().client_id, BUILT_IN_CLIENT_ID, is_guid)
}

/// The tenant in use and where it was set, in the same order. None: any work or school account.
pub fn tenant_source(cfg: &Config) -> Option<(String, Source)> {
    pick(&cfg.m365_tenant, &installed().tenant, BUILT_IN_TENANT_ID, is_tenant)
}

/// How wide the calendar panel makes the menu.
pub const PANEL_WIDTH: f32 = 300.0;

pub fn is_guid(text: &str) -> bool {
    let parts: Vec<&str> = text.trim().split('-').collect();
    parts.len() == 5 && parts.iter().zip([8, 4, 4, 4, 12]).all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A tenant's ID or one of its domains.
pub fn is_tenant(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && text.len() <= 255 && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// The app registration to sign in with.
pub fn client_id(cfg: &Config) -> Option<String> {
    client_source(cfg).map(|(id, _)| id)
}

/// Where accounts sign in: the tenant set somewhere, else any work or school account.
pub fn tenant(cfg: &Config) -> String {
    tenant_source(cfg).map_or_else(|| "common".into(), |(tenant, _)| tenant)
}

/// Whether this copy can sign in at all: it needs an app registration.
pub fn available(cfg: &Config) -> bool {
    client_id(cfg).is_some()
}

pub fn signed_in(cfg: &Config) -> bool {
    cfg.m365 && available(cfg) && !cfg.m365_account_id.is_empty()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Feature {
    Calendar,
    People,
    Mail,
}

/// Whether a feature is switched on and can work: someone is signed in.
pub fn feature_on(cfg: &Config, feature: Feature) -> bool {
    signed_in(cfg) && wanted(cfg, feature)
}

fn wanted(cfg: &Config, feature: Feature) -> bool {
    match feature {
        Feature::Calendar => cfg.m365_calendar,
        Feature::People => cfg.m365_people,
        Feature::Mail => cfg.m365_mail,
    }
}

/// What Microsoft is asked for: who's signed in, and only what the features switched on need.
pub fn scopes(cfg: &Config) -> Vec<&'static str> {
    let mut scopes = vec!["User.Read"];
    if wanted(cfg, Feature::People) {
        scopes.push("User.ReadBasic.All");
    }
    if wanted(cfg, Feature::Calendar) {
        scopes.push("Calendars.ReadWrite");
    }
    if wanted(cfg, Feature::Mail) {
        scopes.push("Mail.ReadBasic");
    }
    scopes
}

/// Whether the calendar panel is beside the menu: switched on, signed in, and a look that has room for it.
pub fn panel_shown(cfg: &Config) -> bool {
    feature_on(cfg, Feature::Calendar) && matches!(cfg.experience, Experience::Windows11 | Experience::Startline)
}

/// Whether a sign-in error means the organization has to approve Startline first.
pub fn needs_admin_consent(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["aadsts65001", "aadsts90094", "aadsts90095", "aadsts900941", "consent_required", "admin approval", "admin_consent", "adminconsent"].iter().any(|sign| lower.contains(sign)) // i18n-ignore: error codes
}

/// The address an administrator opens to approve Startline for their organization.
pub fn admin_consent_link(cfg: &Config) -> Option<String> {
    Some(format!("https://login.microsoftonline.com/{}/adminconsent?client_id={}", tenant(cfg), client_id(cfg)?))
}

/// Whether the Teams app is there to take msteams: links.
pub fn teams_installed() -> bool {
    use windows::Win32::System::Registry::{HKEY, HKEY_CLASSES_ROOT, KEY_READ, RegCloseKey, RegOpenKeyExW};
    let name = crate::platform::wide("msteams");
    let mut key = HKEY::default();
    let found = unsafe { RegOpenKeyExW(HKEY_CLASSES_ROOT, windows::core::PCWSTR(name.as_ptr()), None, KEY_READ, &mut key) }.is_ok();
    if found {
        let _ = unsafe { RegCloseKey(key) };
    }
    found
}

/// A Teams address, opened in the Teams app when it's there and in the browser otherwise.
pub fn teams_link(url: &str, installed: bool) -> String {
    match url.strip_prefix("https://teams.microsoft.com") {
        Some(rest) if installed => format!("msteams:{rest}"),
        _ => url.to_string(),
    }
}

pub fn chat_link(upn: &str, installed: bool) -> String {
    teams_link(&format!("https://teams.microsoft.com/l/chat/0/0?users={}", crate::search::percent_encode(upn)), installed)
}

pub fn call_link(upn: &str, installed: bool) -> String {
    teams_link(&format!("https://teams.microsoft.com/l/call/0/0?users={}", crate::search::percent_encode(upn)), installed)
}

pub fn mail_link(address: &str) -> String {
    format!("mailto:{address}")
}

// ---- What's kept on disk ----

/// Where the sign-in's own files are: a refresh token after a browser sign-in, and the calendar's last answer. Both
/// encrypted for this Windows account.
#[derive(Clone, Debug)]
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn app() -> Store {
        Store { dir: crate::config::data_dir() }
    }

    fn refresh_file(&self) -> PathBuf {
        self.dir.join("m365-refresh.bin")
    }

    pub fn calendar_file(&self) -> PathBuf {
        self.dir.join("m365-calendar.bin")
    }

    pub fn save_refresh(&self, token: &str) {
        if let Some(sealed) = auth::protect(token.as_bytes()) {
            let _ = std::fs::write(self.refresh_file(), sealed);
        }
    }

    pub fn load_refresh(&self) -> Option<String> {
        let sealed = std::fs::read(self.refresh_file()).ok()?;
        String::from_utf8(auth::unprotect(&sealed)?).ok().filter(|t| !t.is_empty())
    }

    /// Everything the sign-in left on disk.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(self.refresh_file());
        let _ = std::fs::remove_file(self.calendar_file());
    }
}

// ---- Tokens ----

/// Access tokens for Graph: from Windows's account broker, or from the refresh token of a browser sign-in. Only ever
/// kept in memory, until they run out.
struct SignInTokens {
    client_id: String,
    tenant: String,
    account_id: String,
    scopes: Arc<Mutex<Vec<&'static str>>>,
    store: Store,
    cached: Mutex<Option<(String, Instant)>>,
}

impl Tokens for SignInTokens {
    fn token(&self) -> Result<String, GraphError> {
        let mut cached = self.cached.lock().unwrap();
        if let Some((token, until)) = cached.as_ref() {
            if Instant::now() + Duration::from_secs(60) < *until {
                return Ok(token.clone());
            }
        }
        let scopes = self.scopes.lock().unwrap().clone();
        let (token, lasts) = match self.store.load_refresh() {
            Some(refresh) => {
                let granted = auth::refresh(&self.tenant, &self.client_id, &scopes, &refresh)?;
                if !granted.refresh.is_empty() {
                    self.store.save_refresh(&granted.refresh);
                }
                (granted.access, granted.lasts)
            }
            None => (auth::broker_token(&self.tenant, &self.client_id, &scopes, &self.account_id)?, 3000),
        };
        *cached = Some((token.clone(), Instant::now() + Duration::from_secs(lasts)));
        Ok(token)
    }

    fn forget(&self) {
        *self.cached.lock().unwrap() = None;
    }
}

/// A signed-in account, while it's in use.
pub struct Session {
    pub client_id: String,
    pub tenant: String,
    pub account_id: String,
    pub graph: Arc<dyn Graph>,
    pub store: Store,
    scopes: Arc<Mutex<Vec<&'static str>>>,
    tokens: Arc<dyn Tokens>,
    /// Who it is, once Graph said.
    pub me: Mutex<Option<Me>>,
    /// Whether Windows's account broker holds its tokens, to tell it to forget them on sign-out.
    broker: bool,
}

impl Session {
    /// The session for the account in the settings, talking to the real Graph.
    pub fn open(cfg: &Config, store: Store) -> Option<Session> {
        let (client_id, tenant) = (client_id(cfg)?, tenant(cfg));
        let scopes = Arc::new(Mutex::new(scopes(cfg)));
        let broker = store.load_refresh().is_none();
        let tokens: Arc<dyn Tokens> = Arc::new(SignInTokens { client_id: client_id.clone(), tenant: tenant.clone(), account_id: cfg.m365_account_id.clone(), scopes: scopes.clone(), store: store.clone(), cached: Mutex::new(None) });
        let graph: Arc<dyn Graph> = Arc::new(HttpGraph { tokens: tokens.clone() });
        Some(Session { client_id, tenant, account_id: cfg.m365_account_id.clone(), graph, store, scopes, tokens, me: Mutex::new(None), broker })
    }

    /// A session over any Graph and tokens, for tests.
    #[cfg(test)]
    pub fn with(cfg: &Config, store: Store, graph: Arc<dyn Graph>, tokens: Arc<dyn Tokens>) -> Session {
        Session { client_id: client_id(cfg).unwrap_or_default(), tenant: tenant(cfg), account_id: cfg.m365_account_id.clone(), graph, store, scopes: Arc::new(Mutex::new(scopes(cfg))), tokens, me: Mutex::new(None), broker: false }
    }

    /// Whether this session is still the one the settings describe.
    pub fn fits(&self, cfg: &Config) -> bool {
        client_id(cfg).as_deref() == Some(self.client_id.as_str()) && tenant(cfg) == self.tenant && cfg.m365_account_id == self.account_id
    }

    /// The features switched on changed: tokens from now on carry what they need.
    pub fn set_scopes(&self, cfg: &Config) {
        let wanted = scopes(cfg);
        let mut scopes = self.scopes.lock().unwrap();
        if *scopes != wanted {
            *scopes = wanted;
            self.tokens.forget();
        }
    }

    pub fn scopes(&self) -> Vec<&'static str> {
        self.scopes.lock().unwrap().clone()
    }

    /// Who's signed in, asked of Graph once.
    pub fn who(&self) -> Result<Me, GraphError> {
        if let Some(me) = self.me.lock().unwrap().clone() {
            return Ok(me);
        }
        let me = graph::parse_me(&self.graph.get(graph::ME_PATH, &[])?).ok_or(GraphError::SignInNeeded)?;
        *self.me.lock().unwrap() = Some(me.clone());
        Ok(me)
    }

    /// Forgets everything of the sign-in: tokens in memory, the files on disk, the account in the settings, and the
    /// tokens Windows's account broker holds for Startline.
    pub fn sign_out(&self, cfg: &mut Config) {
        self.tokens.forget();
        *self.me.lock().unwrap() = None;
        self.store.clear();
        if self.broker && !self.account_id.is_empty() {
            auth::broker_forget(&self.tenant, &self.client_id, &self.account_id);
        }
        cfg.m365_account_id.clear();
    }
}

/// What's left after signing out when there's no session (it never opened): the files, and the account's id.
pub fn forget_everything(cfg: &mut Config, store: &Store) {
    store.clear();
    cfg.m365_account_id.clear();
}

/// Signs in: through Windows's account broker over the window `hwnd`, else through the browser. The account's id.
pub fn sign_in(hwnd: isize, cfg: &Config, store: &Store) -> Result<String, SignInError> {
    let client = client_id(cfg).ok_or_else(|| SignInError::Failed(crate::i18n::tr("Not available in this build").into()))?;
    let (tenant, scopes) = (tenant(cfg), scopes(cfg));
    match auth::broker_sign_in(hwnd, &tenant, &client, &scopes) {
        Ok((_, account)) if !account.is_empty() => {
            // The broker keeps the tokens now; a refresh token from an earlier browser sign-in has no use.
            store.clear();
            return Ok(account);
        }
        Err(SignInError::Cancelled) => return Err(SignInError::Cancelled),
        Err(SignInError::AdminConsent) => return Err(SignInError::AdminConsent),
        _ => {}
    }
    let granted = auth::browser_sign_in(&tenant, &client, &scopes)?;
    let bearer = format!("Bearer {}", granted.access);
    let (status, body) = graph::http("GET", &format!("{}{}", graph::GRAPH, graph::ME_PATH), &[("Authorization", bearer.as_str())], None).map_err(SignInError::Failed)?; // i18n-ignore: an HTTP header
    let me = graph::parse_me(&body).filter(|_| (200..300).contains(&status)).ok_or_else(|| SignInError::Failed(crate::i18n::tr("Microsoft didn't say who signed in.").into()))?;
    store.save_refresh(&granted.refresh);
    Ok(me.id)
}
