//! Company policies: one list of everything an administrator can set. The Group Policy
//! templates and the admin guide are written from it, the app reads the same names back from
//! the Registry, and a test fails if a setting is ever added without a policy.
//!
//! Fixed settings: `Software\Policies\Startline`, the PC's (HKLM) winning over the account's
//! (HKCU). They always apply and are greyed out in Settings, without replacing the person's own
//! choice: remove the policy and their choice comes back. Defaults: the `Recommended` subkey
//! below that. A default applies until the person changes that setting themselves.
//!
//! The list is here; how the Registry is read and what it means is in policy_rules.rs, the
//! templates and the admin guide in policy_templates.rs.

use crate::actions::{ACTIONS, Do};
use crate::config::PRESETS;
use crate::customize::Tab;
use crate::i18n::{self, Lang, tr_in, tx};
use crate::scope::SKIP_PRESETS;
use crate::setting_list::QuickId;
use std::sync::OnceLock;

#[path = "policy_rules.rs"]
mod rules;
#[path = "policy_templates.rs"]
mod templates;
pub use rules::*;
pub use templates::write_templates;

const KEY: &str = "Software\\Policies\\Startline";
const RECOMMENDED: &str = "Software\\Policies\\Startline\\Recommended";
pub const THEME_KEYS: [&str; 6] = ["ColorTheme", "Color_Background", "Color_Panels", "Color_Text", "Color_QuietText", "Color_Accent"];
const AUTOSTART: &str = "StartWithWindows";
const PINS: &str = "PinnedApps";
const PAGES: &str = "SearchWindowsSettingsPages";
const WINDOWS_SETTINGS: &str = "SearchChangeWindowsSettings";
pub const CHECK_FOR_UPDATES: &str = "CheckForUpdates";

/// Saved in the settings file, but only ever remembered for the person, never chosen by them.
#[cfg(test)]
const NOT_SETTINGS: [&str; 8] = ["version", "pins_seeded", "autostart_applied", "settings_pos", "settings_size", "usage", "chosen", "m365_account_id"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Group {
    Appearance,
    StartMenu,
    Pins,
    SearchPlaces,
    SearchSkip,
    SearchResults,
    SearchBox,
    General,
    Microsoft365,
    Updates,
    Limits,
}

impl Group {
    /// The category in the templates, its name, and the category it's in.
    const ALL: [(Group, &'static str, &'static str, &'static str); 11] = [
        (Group::Appearance, "Appearance", tx("Appearance"), "Startline"), // i18n-ignore: category ids
        (Group::StartMenu, "StartMenu", tx("Start menu"), "Startline"), // i18n-ignore: category ids
        (Group::Pins, "Pins", tx("Pinned apps"), "Startline"), // i18n-ignore: category ids
        (Group::SearchPlaces, "SearchPlaces", tx("Where to search"), "Search"), // i18n-ignore: category ids
        (Group::SearchSkip, "SearchSkip", tx("Never found"), "Search"), // i18n-ignore: category ids
        (Group::SearchResults, "SearchResults", tx("Results"), "Search"), // i18n-ignore: category ids
        (Group::SearchBox, "SearchBox", tx("Search box"), "Search"), // i18n-ignore: category ids
        (Group::General, "General", tx("General"), "Startline"), // i18n-ignore: category ids
        (Group::Microsoft365, "Microsoft365", tx("Microsoft 365"), "Startline"), // i18n-ignore: category ids
        (Group::Updates, "Updates", tx("Updates"), "Startline"), // i18n-ignore: category ids
        (Group::Limits, "Limits", tx("What people can do"), "Startline"), // i18n-ignore: category ids
    ];

    fn tab(self) -> Tab {
        match self {
            Group::Appearance => Tab::Appearance,
            Group::StartMenu => Tab::StartMenu,
            Group::Pins => Tab::Pins,
            Group::SearchPlaces | Group::SearchSkip | Group::SearchResults | Group::SearchBox => Tab::Search,
            Group::General | Group::Limits => Tab::General,
            Group::Microsoft365 => Tab::Microsoft365,
            Group::Updates => Tab::About,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Kind {
    Bool,
    Number { min: u32, max: u32, divide: u32, default: u32 },
    Choice(Vec<(String, String)>),
    Text(&'static str),
    Color(&'static str),
    List,
    /// A list whose items are added to the person's own, which they can't remove.
    AddList,
    /// One tick box per item, all ticked to begin with.
    Checks(Vec<(String, String)>),
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub key: String,
    /// Where it lands in the saved settings, if it's one of them.
    pub field: &'static str,
    pub group: Group,
    pub title: String,
    pub about: String,
    pub kind: Kind,
}

impl Policy {
    /// Whether it's a setting, which can also be given as a default the person can change.
    pub fn has_default(&self) -> bool {
        !matches!(self.kind, Kind::Checks(_)) && (!self.field.is_empty() || self.key == AUTOSTART || SKIP_PRESETS.iter().any(|p| p.key == self.key))
    }
}

fn entry(key: &str, field: &'static str, group: Group, title: &str, about: &str, kind: Kind) -> Policy {
    Policy { key: key.to_string(), field, group, title: title.to_string(), about: about.to_string(), kind }
}

fn switch(key: &str, field: &'static str, group: Group, title: &str, about: &str) -> Policy {
    entry(key, field, group, title, about, Kind::Bool)
}

fn number(key: &str, field: &'static str, group: Group, title: &str, about: &str, min: u32, max: u32, default: u32) -> Policy {
    entry(key, field, group, title, about, Kind::Number { min, max, divide: 1, default })
}

fn options(pairs: &[(&str, &str)]) -> Kind {
    Kind::Choice(pairs.iter().map(|(value, label)| (value.to_string(), label.to_string())).collect())
}

fn list(key: &str, field: &'static str, group: Group, title: &str, about: &str) -> Policy {
    entry(key, field, group, title, about, Kind::List)
}

fn add_list(key: &str, field: &'static str, group: Group, title: &str, about: &str) -> Policy {
    entry(key, field, group, title, about, Kind::AddList)
}

fn command_id(what: Do) -> Option<&'static str> {
    match what {
        Do::Open(_) => None,
        Do::Lock => Some("Command_Lock"),
        Do::Sleep => Some("Command_Sleep"),
        Do::SignOut => Some("Command_SignOut"),
        Do::SwitchUser => Some("Command_SwitchUser"),
        Do::Restart => Some("Command_Restart"),
        Do::ShutDown => Some("Command_ShutDown"),
        Do::EmptyRecycleBin => Some("Command_EmptyRecycleBin"),
        Do::Customize => Some("Command_Customize"),
        Do::PinPage => Some("Command_PinPage"),
        Do::RealStart => Some("Command_RealStart"),
        Do::Quit => Some("Command_Quit"),
    }
}

/// Everything an administrator can set, in English.
pub fn all() -> &'static [Policy] {
    static LIST: OnceLock<Vec<Policy>> = OnceLock::new();
    LIST.get_or_init(|| build(Lang::En))
}

/// The same list, with its words in `lang`, for the templates.
fn build(lang: Lang) -> Vec<Policy> {
    use Group::*;
    let t = |en: &'static str| tr_in(lang, en);
    let startline_only = t(tx("Only the Startline look uses this."));
    let languages: Vec<(&str, &str)> = i18n::CHOICES.iter().map(|(code, name)| (*code, if *code == "system" { t(tx("The language Windows is shown in")) } else { *name })).collect();
    let mut list = vec![
        // Appearance
        entry("Experience", "/experience", Appearance, t(tx("Look")), t(tx("Which look the Start menu, search and the settings window use.")), options(&[("Windows11", t(tx("Windows 11"))), ("Windows10", t(tx("Windows 10"))), ("Windows7", t(tx("Windows 7"))), ("WindowsXp", t(tx("Windows XP"))), ("Startline", "Startline")])), // i18n-ignore: registry values
        entry("Language", "/language", Appearance, t(tx("Language")), t(tx("The language Startline shows its text in: system follows the language Windows is shown in when it's English, German or Italian, and uses English otherwise.")), options(&languages)), // i18n-ignore: registry value name
        entry("TextAndIconSize", "/ui_scale", Appearance, t(tx("Text and icon size")), t(tx("In percent. The settings window offers 90 (small), 100 (normal) and 115 (large).")), Kind::Number { min: 80, max: 150, divide: 100, default: 100 }),
        switch("Mica", "/mica", Appearance, t(tx("Mica behind the Start menu")), t(tx("Windows 11's Mica material behind the Start menu, tinted by the desktop picture, instead of frosted glass. Windows 11 version 22H2 and later; older Windows ignores it. For the Windows 11 and Startline looks."))), // i18n-ignore: registry value name
        switch("MicaLight", "/mica_light", Appearance, t(tx("Lighter Mica through the search box and the selected item")), t(tx("With Mica on, lets a lighter shade of it through the search box and the selected item instead of covering them."))),
        entry("ColorTheme", "/preset", Appearance, t(tx("Color theme")), startline_only, Kind::Choice(PRESETS.iter().map(|(name, _)| (name.to_string(), t(name).to_string())).collect())),
        entry("Color_Background", "/theme/bg", Appearance, t(tx("Background color")), t(tx("Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.")), Kind::Color("#1B1B1A")),
        entry("Color_Panels", "/theme/surface", Appearance, t(tx("Panel color")), t(tx("Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.")), Kind::Color("#272725")),
        entry("Color_Text", "/theme/text", Appearance, t(tx("Text color")), t(tx("Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.")), Kind::Color("#ECEAE5")),
        entry("Color_QuietText", "/theme/subtext", Appearance, t(tx("Quiet text color")), t(tx("Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.")), Kind::Color("#94928C")),
        entry("Color_Accent", "/theme/accent", Appearance, t(tx("Accent color")), t(tx("Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.")), Kind::Color("#D9A46B")),
        entry("Corners", "/corners", Appearance, t(tx("Window corners")), startline_only, options(&[("Round", t(tx("Round"))), ("Slight", t(tx("Slight"))), ("Square", t(tx("Square")))])), // i18n-ignore: registry values
        switch("FrostedGlass", "/blur", Appearance, t(tx("Frosted glass")), t(tx("Blurs what's behind the menu. Only the Startline look uses this."))),
        entry("Opacity", "/opacity", Appearance, t(tx("How solid the menu is")), t(tx("In percent, from 50 (most see-through) to 100 (solid). Only the Startline look uses this.")), Kind::Number { min: 50, max: 100, divide: 100, default: 96 }), // i18n-ignore: registry value name
        // Start menu
        list("Sections", "/sections", StartMenu, t(tx("What the Start menu shows")), t(tx("One per line, in the order they should appear: Pinned, Recent, Frequent, AllApps. Any left out are hidden."))), // i18n-ignore: registry value name
        number("RecentCount", "/recent_count", StartMenu, t(tx("Number of recent files")), "", 2, 20, 6),
        number("FrequentCount", "/frequent_count", StartMenu, t(tx("Number of most-used items")), "", 2, 20, 6),
        switch("SearchAtBottom", "/search_at_bottom", StartMenu, t(tx("Search bar at the bottom")), startline_only),
        number("Columns", "/columns", StartMenu, t(tx("Pinned apps per row")), startline_only, 3, 12, 6), // i18n-ignore: registry value name
        number("TileSize", "/tile_size", StartMenu, t(tx("Pinned app size")), startline_only, 52, 132, 84),
        switch("ShowLabels", "/show_labels", StartMenu, t(tx("Names under pinned apps")), startline_only),
        switch("CompactRows", "/compact_rows", StartMenu, t(tx("Compact lists")), startline_only),
        entry("HorizontalPosition", "/h_anchor", StartMenu, t(tx("Where it opens across the screen")), startline_only, options(&[("Left", t(tx("Left"))), ("Center", t(tx("Center"))), ("Right", t(tx("Right"))), ("FollowStart", t(tx("Beside the Start button")))])), // i18n-ignore: registry values
        entry("VerticalPosition", "/v_anchor", StartMenu, t(tx("Where it opens up and down the screen")), startline_only, options(&[("Top", t(tx("Top"))), ("Middle", t(tx("Middle"))), ("Bottom", t(tx("Bottom")))])), // i18n-ignore: registry values
        number("Margin", "/margin", StartMenu, t(tx("Gap from the screen edge")), startline_only, 0, 80, 12), // i18n-ignore: registry value name
        number("Width", "/width", StartMenu, t(tx("Menu width")), startline_only, 420, 1200, 660), // i18n-ignore: registry value name
        number("Height", "/height", StartMenu, t(tx("Menu height")), startline_only, 420, 1100, 720), // i18n-ignore: registry value name
        // Pinned apps
        list(PINS, "/groups", Pins, t(tx("Pinned apps")), t(tx("One per line: Section|Name|Target or Section|Name|Target|Arguments. Target is a file, folder, program or web address, or app:ID for an installed app. Environment variables such as %USERPROFILE% work. As a fixed setting this replaces the person's pins and stops them changing pins; as a default it's the pins they start with."))),
        // Search: where to search
        list("SearchFolders", "/roots", SearchPlaces, t(tx("Folders to search")), t(tx("One folder or drive per line. Environment variables such as %USERPROFILE% work. This replaces the person's own places."))),
        switch("SearchLocalDrives", "/index_local", SearchPlaces, t(tx("Index local drives")), t(tx("Names and dates of files and folders on this PC's own drives, read gently and kept up to date as they change."))),
        switch("SearchNetworkDrives", "/auto_network", SearchPlaces, t(tx("Index network drives")), t(tx("Network drives and shares, including \\\\server\\share places added by hand. Read at most once a day while the PC is idle, one folder at a time, with no watch on the server."))),
        switch("SearchUsbDrives", "/auto_removable", SearchPlaces, t(tx("Index USB drives")), t(tx("USB sticks, disks and memory cards, as they get plugged in."))),
        // Search: never found
        add_list("SearchSkipFolders", "/excluded_paths", SearchSkip, t(tx("Folders never found")), t(tx("One folder per line, such as %USERPROFILE%\\AppData. Everything inside is never found either."))),
        add_list("SearchSkipFolderNames", "/exclude", SearchSkip, t(tx("Folder names never found")), t(tx("One name per line. A folder with this name is never found, nor anything in it, wherever it is."))),
        add_list("SearchSkipFileTypes", "/excluded_exts", SearchSkip, t(tx("File types never found")), t(tx("One per line, such as .iso."))),
        add_list("SearchSkipPatterns", "/skip_patterns", SearchSkip, t(tx("Paths never found (regular expressions)")), t(tx("One regular expression per line, matched against the whole path whatever the case. A file or folder that matches is never found, nor anything in a folder that matches. For example \\\\Payroll(\\\\|$) or \\.(key|pem|pfx)$. A pattern that isn't valid is ignored."))),
        switch("SearchSkipHidden", "/skip_hidden", SearchSkip, t(tx("Hidden files and folders are never found")), t(tx("Things Windows keeps out of sight. Disabled finds them."))),
        switch("SearchSkipSystem", "/skip_system", SearchSkip, t(tx("System files are never found")), t(tx("Files and folders Windows marks as part of the system, such as desktop.ini. Disabled finds them."))),
        // Search: results
        list("SearchResults", "/results", SearchResults, t(tx("Kinds of results")), t(tx("One per line, first to last: Apps, Settings, Files, People, Mail, Web. A kind left out is never shown. People and Mail also need their Microsoft 365 policies."))),
        number("SearchMaxApps", "/max_apps", SearchResults, t(tx("Most apps shown in the results")), "", 1, 30, 8),
        number("SearchMaxSettings", "/max_settings", SearchResults, t(tx("Most settings shown in the results")), "", 1, 30, 6),
        number("SearchMaxFiles", "/max_files", SearchResults, t(tx("Most files shown in the results")), "", 5, 200, 30),
        switch("SearchShowFolders", "/show_paths", SearchResults, t(tx("Show the folder under each file")), ""),
        switch("SearchBestMatch", "/best_match", SearchResults, t(tx("Best match at the top")), t(tx("The one result whose name fits what was typed best, whatever its kind, comes first."))),
        switch("SearchWebUseAddress", "/web_fixed", SearchResults, t(tx("Search the web with a fixed address")), t(tx("Enabled, web searches open the web search address. Disabled or not set, they go to the default browser and the search engine set up there."))),
        entry("SearchWebName", "/web_name", SearchResults, t(tx("Name of the web search")), t(tx("Shown as: Search <name> for ..., when the web search address is used.")), Kind::Text("the web")), // i18n-ignore: the default as it is stored
        entry("SearchWebAddress", "/web_url", SearchResults, t(tx("Web search address")), t(tx("Used when SearchWebUseAddress is enabled, and when the default browser isn't one Startline knows how to ask. Must start with http:// or https:// and hold {} where the words go. Anything else is ignored.")), Kind::Text("https://duckduckgo.com/?q={}")),
        switch(WINDOWS_SETTINGS, "/search_windows_settings", SearchResults, t(tx("Change Windows settings from search")), t(tx("Disabled removes every Windows setting search can change."))),
        switch(PAGES, "/search_settings_pages", SearchResults, t(tx("Shortcuts to Windows settings and tools")), t(tx("Disabled removes search's shortcuts to Windows settings pages and tools, and the Control Panel, printer and default program links."))),
        // Search: the search box
        switch("SearchRegex", "/regex_search", SearchBox, t(tx("Treat searches as regular expressions")), t(tx("The whole search is one regular expression, matched against names whatever the case. The switch in the search box turns it on and off."))),
        switch("SearchShowRegexSwitch", "/show_regex_switch", SearchBox, t(tx("Regular expression switch in the search box")), t(tx("Disabled hides the switch, and searches are then never regular expressions unless SearchRegex is enabled."))),
        switch("SearchShowHelp", "/show_search_help", SearchBox, t(tx("Help button in the search box")), t(tx("The ? that shows what can be typed: words, \"exact text\", wildcards, -without, ext:, kind:, size:, modified: and regular expressions."))),
        switch("SearchFileTypeButtons", "/show_file_buttons", SearchBox, t(tx("File type buttons under search")), t(tx("The buttons that narrow results to one kind of file."))),
        switch("SearchShowTime", "/show_timing", SearchBox, t(tx("Show how long a search took")), ""),
        switch("SearchShowHints", "/show_search_hints", SearchBox, t(tx("Keyboard hints under the results")), t(tx("The line that says what Enter, Ctrl+Enter, Shift+Enter and Esc do."))),
        // General
        switch(AUTOSTART, "", General, t(tx("Start with Windows")), t(tx("Keeps the Start menu working after a restart."))),
        switch("TakeOverStartKey", "/take_over_start", General, t(tx("Start key and button open this menu")), t(tx("Shift + Start still opens the Windows menu."))),
        switch("TakeOverSearchKey", "/take_over_search", General, t(tx("Start + S opens this menu too")), t(tx("Other Windows shortcuts keep working."))),
        switch("SkipWelcome", "/seen_welcome", General, t(tx("Skip the welcome note")), t(tx("Enabled hides the note shown on the first run."))),
        // Microsoft 365
        switch("M365Account", "/m365", Microsoft365, t(tx("Microsoft 365 account")), t(tx("Lets people sign in with a work or school account, for their calendar and to find people and mail. Disabled, no one can sign in and Startline never contacts Microsoft. Off unless set."))),
        switch("M365Calendar", "/m365_calendar", Microsoft365, t(tx("Calendar beside the menu")), t(tx("Today's calendar in a panel beside the menu, in the Windows 11 and Startline looks. Asks for Calendars.ReadWrite. Off unless set."))),
        switch("M365People", "/m365_people", Microsoft365, t(tx("People in search")), t(tx("Colleagues in search, to chat with in Teams, call or mail. Asks for User.ReadBasic.All. Off unless set."))),
        switch("M365Mail", "/m365_mail", Microsoft365, t(tx("Mail in search")), t(tx("The person's mail in search, by subject. Asks for Mail.ReadBasic. Off unless set."))),
        entry("M365ClientId", "/m365_client_id", Microsoft365, t(tx("App registration (client ID)")), t(tx("The application (client) ID of the organization's own app registration, used instead of the one built into Startline. Must be a GUID; anything else is ignored.")), Kind::Text("")),
        entry("M365TenantId", "/m365_tenant", Microsoft365, t(tx("Tenant")), t(tx("The tenant ID or domain, such as contoso.onmicrosoft.com. Only accounts of this tenant can sign in. Not set, any work or school account can.")), Kind::Text("")),
        // Updates
        switch("AutomaticUpdates", "/auto_update", Updates, t(tx("Install updates automatically")), t(tx("Startline installs new versions by itself, restarting while its menu is closed. On an install for the whole PC, the Startline Updater task installs them as SYSTEM within the hour. Disabled leaves updates to the person, or on an install for the whole PC, to the administrator."))),
        switch(CHECK_FOR_UPDATES, "", Updates, t(tx("Look for updates")), t(tx("Disabled stops Startline looking for new versions at all, for organizations that deploy them their own way."))),
    ];
    for preset in SKIP_PRESETS {
        let title = i18n::fill(t(tx("Never found: {what}")), &[("what", &t(preset.title))]);
        let about = i18n::fill(t(tx("{about} Enabled never finds them, disabled always finds them.")), &[("about", &t(preset.about))]);
        list.push(switch(preset.key, "", SearchSkip, &title, &about));
    }
    let windows = QuickId::ALL.iter().map(|id| (format!("Setting_{id:?}"), t(id.info().label).to_string())).collect(); // i18n-ignore: check box id
    list.push(entry("SearchWindowsSettings", "", SearchResults, t(tx("Windows settings search can change")), t(tx("Unticked settings are removed from search.")), Kind::Checks(windows)));

    let mut commands: Vec<(String, String)> = ACTIONS.iter().filter_map(|action| command_id(action.what).map(|id| (id.to_string(), t(action.name).to_string()))).collect();
    commands.push(("Command_SettingsFolder".to_string(), t(tx("Open my settings folder")).to_string()));
    commands.push(("Command_ResetSettings".to_string(), t(tx("Reset all settings")).to_string()));
    list.push(entry("Commands", "", Limits, t(tx("Commands people can use")), t(tx("Unticked commands are removed everywhere they appear. Removing Customize Startline also closes off the settings window.")), Kind::Checks(commands))); // i18n-ignore: registry value name

    let menu = [
        ("Menu_RunAsAdmin", tx("Run as administrator")),
        ("Menu_OpenWith", tx("Open with")),
        ("Menu_OpenLocation", tx("Open file location")),
        ("Menu_Copy", tx("Copy path, name or address")),
        ("Menu_Uninstall", tx("Uninstall")),
        ("Menu_Recycle", tx("Move to Recycle Bin")),
        ("Menu_Properties", tx("Properties")),
        ("Menu_ShellMenu", tx("More options (the Windows menu)")),
    ];
    let menu = menu.iter().map(|(id, label)| (id.to_string(), t(label).to_string())).collect();
    list.push(entry("RightClickOptions", "", Limits, t(tx("Right-click options")), t(tx("Unticked options are removed. Their keyboard shortcuts stop working too.")), Kind::Checks(menu)));

    list
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
