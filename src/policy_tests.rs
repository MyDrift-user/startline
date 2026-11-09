use super::templates::{guide, templates};
use super::*;
use crate::config::{Category, Config, ResultKind, SectionKind, Target};
use crate::search::RowKind;
use crate::ui::Intent;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

fn raw(values: Value) -> Raw {
    tests_raw(values)
}

fn forced(policy: &Policy) -> Option<Value> {
    match &policy.kind {
        Kind::Bool => Some(json!(if matches!(policy.field, "/regex_search" | "/mica" | "/seen_welcome" | "/compact_rows") { 1 } else { 0 })),
        Kind::Number { min, max, .. } => Some(json!(if *min + 1 <= *max { *min + 1 } else { *min })),
        Kind::Choice(items) => Some(json!(items.last().unwrap().0)),
        Kind::Text(_) => Some(json!("https://example.com/?q={}")),
        Kind::Color(_) => Some(json!("#010203")),
        Kind::List | Kind::AddList => Some(json!(match policy.field {
            "/sections" => vec!["Recent"],
            "/results" => vec!["Web", "Files"],
            "/groups" => vec!["Work|Notes|C:\\notes.txt"],
            _ => vec![".test"],
        })),
        Kind::Checks(_) => None,
    }
}

#[test]
fn every_saved_setting_has_a_policy() {
    let saved = serde_json::to_value(Config::default()).unwrap();
    let mut wanted: BTreeSet<String> = saved.as_object().unwrap().keys().filter(|k| !NOT_SETTINGS.contains(&k.as_str())).map(|k| format!("/{k}")).collect();
    wanted.remove("/theme");
    for part in saved["theme"].as_object().unwrap().keys() {
        wanted.insert(format!("/theme/{part}"));
    }
    let covered: BTreeSet<String> = all().iter().filter(|p| !p.field.is_empty()).map(|p| p.field.to_string()).collect();
    assert_eq!(wanted, covered);
    assert_eq!(all().iter().filter(|p| !p.field.is_empty()).count(), covered.len(), "two policies set the same setting");
}

#[test]
fn every_option_has_a_policy() {
    let keys: HashSet<&str> = all().iter().map(|p| p.key.as_str()).collect();
    for preset in SKIP_PRESETS {
        assert!(keys.contains(preset.key), "{}", preset.title);
    }
    let checks: HashSet<&str> = all().iter().filter_map(|p| if let Kind::Checks(items) = &p.kind { Some(items) } else { None }).flatten().map(|(id, _)| id.as_str()).collect();
    for id in QuickId::ALL {
        assert!(checks.contains(format!("Setting_{id:?}").as_str()), "{id:?}");
    }
    for action in ACTIONS {
        if let Some(id) = command_id(action.what) {
            assert!(checks.contains(id), "{id}");
        }
    }
    for key in [AUTOSTART, PAGES, WINDOWS_SETTINGS] {
        assert!(keys.contains(key), "{key}");
    }
}

#[test]
fn every_policy_really_changes_what_it_says() {
    for policy in all() {
        let Some(value) = forced(policy) else { continue };
        let policies = Policies::from_raw(&raw(json!({ policy.key.clone(): value })));
        assert!(policies.is_set(&policy.key), "{}", policy.key);
        // An AddList adds to a list rather than setting it: see a_policy_adds_rules_the_person_keeps_their_own_under.
        if policy.field.is_empty() || matches!(policy.kind, Kind::AddList) {
            continue;
        }
        let mut cfg = Config::default();
        let before = serde_json::to_value(&cfg).unwrap();
        policies.enforce(&mut cfg);
        let after = serde_json::to_value(&cfg).unwrap();
        let expected = policies.fixed.fields.iter().find(|(f, _)| *f == policy.field).unwrap().1.clone();
        assert_eq!(after.pointer(policy.field).unwrap().as_f64().map(|n| n as f32), expected.as_f64().map(|n| n as f32), "{}", policy.key);
        if expected.as_f64().is_none() {
            assert_eq!(after.pointer(policy.field), Some(&expected), "{}", policy.key);
        }
        assert!(before.pointer(policy.field).is_some(), "{}", policy.key);
    }
}

#[test]
fn every_setting_can_also_be_a_default() {
    for policy in all().iter().filter(|p| !p.field.is_empty()) {
        assert!(policy.has_default(), "{}", policy.key);
        let Some(value) = forced(policy) else { continue };
        let policies = Policies::from_raws(&Raw::default(), &raw(json!({ policy.key.clone(): value })));
        assert!(!policies.is_set(&policy.key), "a default doesn't lock {}", policy.key);
        assert!(!policies.is_empty());
        assert!(policies.defaults.fields.iter().any(|(f, _)| *f == policy.field), "{}", policy.key);
    }
}

#[test]
fn a_default_applies_until_the_person_changes_it() {
    let policies = Policies::from_raws(&Raw::default(), &raw(json!({ "Columns": 4, "Language": "IT", "SearchSkipDeveloperFiles": 1 })));
    let own = Config::default();
    let mut cfg = own.clone();
    policies.enforce(&mut cfg);
    assert_eq!((cfg.columns, cfg.language.as_str()), (4, "it"), "the defaults are in, a language in any case");
    let dev = SKIP_PRESETS.iter().find(|p| p.key == "SearchSkipDeveloperFiles").unwrap();
    assert!(dev.is_on(&cfg));
    // The person picks their own number of columns: it stays, and is saved as theirs.
    let before = cfg.clone();
    cfg.columns = 7;
    policies.note_choices(&mut cfg, &before);
    policies.enforce(&mut cfg);
    assert_eq!(cfg.columns, 7);
    let saved = policies.without_forced(&cfg, &own);
    assert_eq!(saved.columns, 7);
    assert_eq!(saved.language, "system", "a default they didn't touch isn't saved as theirs");
    assert_eq!(saved.chosen, ["/columns"]);
    // Next time Startline starts, the default still applies to the rest, and their choice to columns.
    let mut again = saved.clone();
    policies.enforce(&mut again);
    assert_eq!((again.columns, again.language.as_str()), (7, "it"));
    // A switch of a ready-made group works the same way.
    let before = again.clone();
    dev.set(&mut again, false);
    policies.note_choices(&mut again, &before);
    policies.enforce(&mut again);
    assert!(!dev.is_on(&again));
}

#[test]
fn a_fixed_setting_wins_over_a_default() {
    let policies = Policies::from_raws(&raw(json!({ "Columns": 5 })), &raw(json!({ "Columns": 4 })));
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.columns, 5);
    assert!(policies.is_set("Columns"));
}

#[test]
fn switches_turn_off_as_well_as_on() {
    let policies = Policies::from_raw(&raw(json!({ "FrostedGlass": 0, "TakeOverStartKey": 0, "SearchChangeWindowsSettings": 0 })));
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert!(!cfg.blur);
    assert!(!cfg.take_over_start);
    assert!(!cfg.search_windows_settings);
    assert!(!policies.allows_row(&RowKind::Quick(QuickId::Wifi, crate::intent::Want::Nothing)));
}

#[test]
fn a_policy_does_not_replace_the_persons_own_choice() {
    let mut own = Config::default();
    own.columns = 9;
    own.width = 500.0;
    let policies = Policies::from_raw(&raw(json!({ "Columns": 4, "SearchSkipDeveloperFiles": 1 })));
    let mut cfg = own.clone();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.columns, 4);
    cfg.width = 700.0;
    let saved = policies.without_forced(&cfg, &own);
    assert_eq!(saved.columns, 9, "their choice is what gets saved");
    assert_eq!(saved.width, 700.0, "other changes are kept");
}

#[test]
fn locked_controls_are_put_back() {
    let policies = Policies::from_raw(&raw(json!({ "Experience": "WindowsXp", "Opacity": 70, "RecentCount": 99 })));
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.experience, crate::styles::Experience::WindowsXp);
    assert_eq!(cfg.opacity, 0.7);
    assert_eq!(cfg.recent_count, 20, "kept in range");
}

#[test]
fn bad_values_are_ignored() {
    let policies = Policies::from_raw(&raw(json!({ "Experience": "Vista", "Color_Accent": "red", "SearchWebAddress": "file:///c:/x", "Language": "fr" })));
    assert!(policies.is_empty());
}

#[test]
fn themes_and_colors_work_together() {
    let policies = Policies::from_raw(&raw(json!({ "ColorTheme": "Moss" })));
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.preset, "Moss");
    assert_eq!(cfg.theme, PRESETS.iter().find(|(n, _)| *n == "Moss").unwrap().1);
    let policies = Policies::from_raw(&raw(json!({ "ColorTheme": "Moss", "Color_Accent": "#102030" })));
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.preset, "Custom");
    assert_eq!(cfg.theme.accent, [16, 32, 48]);
}

#[test]
fn lists_keep_their_meaning() {
    let policies = Policies::from_raw(&raw(json!({
        "Sections": ["Recent", "Pinned"],
        "SearchResults": ["Files"],
        "PinnedApps": ["Work|Notes|C:\\notes.txt", "Work|Calc|app:Calc.App", "not a pin"],
        "SearchSkipFileTypes": ["ISO", "*.tmp", ".log"],
    })));
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert_eq!((cfg.sections[0].kind, cfg.sections[0].visible), (SectionKind::Recent, true));
    assert_eq!((cfg.sections[2].kind, cfg.sections[2].visible), (SectionKind::Frequent, false));
    assert_eq!(cfg.results[0], ResultKind { kind: Category::Files, on: true });
    assert_eq!(cfg.results.len(), 4);
    assert!(!cfg.shows(Category::Web), "a kind left out is never shown");
    assert_eq!(cfg.groups.len(), 1);
    assert_eq!(cfg.groups[0].pins.len(), 2);
    assert_eq!(cfg.groups[0].pins[1].target, Target::App("Calc.App".to_string()));
    assert_eq!(cfg.excluded_exts, vec![".iso", ".tmp", ".log"]);
}

#[test]
fn default_pins_are_where_a_person_starts_and_can_change() {
    let policies = Policies::from_raws(&Raw::default(), &raw(json!({ "PinnedApps": ["Work|Notes|C:\\notes.txt"] })));
    assert!(!policies.pins_locked());
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.groups[0].name, "Work");
    let before = cfg.clone();
    cfg.groups[0].pins.clear();
    policies.note_choices(&mut cfg, &before);
    policies.enforce(&mut cfg);
    assert!(cfg.groups[0].pins.is_empty(), "their change stays");
}

#[test]
fn a_policy_adds_rules_the_person_keeps_their_own_under() {
    let policies = Policies::from_raw(&raw(json!({
        "SearchSkipFolderNames": ["Secret", "Archive"],
        "SearchSkipPatterns": [r"\\Payroll(\\|$)"],
    })));
    let mut own = Config::default();
    own.exclude = vec!["archive".into(), "Mine".into()];
    let mut cfg = own.clone();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.exclude, ["archive", "Mine", "Secret"], "added once, whatever the case");
    assert_eq!(cfg.skip_patterns, [r"\\Payroll(\\|$)"], "a pattern is kept as written");
    cfg.exclude.push("Later".into());
    let saved = policies.without_forced(&cfg, &own);
    assert_eq!(saved.exclude, ["archive", "Mine", "Later"], "the organization's rules aren't saved as the person's");
    assert!(saved.skip_patterns.is_empty());
    assert_eq!(policies.added("/exclude"), ["Secret", "Archive"]);
}

#[test]
fn an_older_settings_file_keeps_its_meaning() {
    let cfg = Config::parse(br#"{"index_hidden": true, "web_search": false, "category_order": ["Files", "Apps", "Settings", "Web"]}"#).unwrap();
    assert!(!cfg.skip_hidden, "hidden files were included");
    assert_eq!(cfg.results[0].kind, Category::Files);
    assert!(cfg.shows(Category::Apps) && !cfg.shows(Category::Web), "the web search was off");
}

#[test]
fn switched_off_things_are_refused() {
    let policies = Policies::from_raw(&raw(json!({
        "Allow_Command_Quit": 0,
        "Allow_Command_Lock": 1,
        "Allow_Menu_RunAsAdmin": 0,
        "Allow_Setting_Wifi": 0,
        "SearchWindowsSettingsPages": 0,
        "StartWithWindows": 0,
    })));
    assert!(!policies.allows_do(Do::Quit));
    assert!(policies.allows_do(Do::Lock));
    assert!(!policies.allows_do(Do::Open("ms-settings:display")));
    assert!(policies.allows_do(Do::Open("shell:Personal")));
    assert!(!policies.allows_row(&RowKind::Quick(QuickId::Wifi, crate::intent::Want::Nothing)));
    assert!(policies.allows_row(&RowKind::Quick(QuickId::Volume, crate::intent::Want::Nothing)));
    assert!(!policies.allows_row(&RowKind::Quick(QuickId::Autostart, crate::intent::Want::Nothing)), "it's set by policy");
    assert!(!policies.allows_intent(&Intent::Open { name: String::new(), target: Target::path("x"), admin: true }));
    assert!(policies.allows_intent(&Intent::Open { name: String::new(), target: Target::path("x"), admin: false }));
    assert_eq!(policies.autostart(), Some(false));
}

#[test]
fn pins_can_be_locked() {
    let policies = Policies::from_raw(&raw(json!({ "PinnedApps": [] })));
    assert!(policies.pins_locked());
    assert!(!policies.allows_intent(&Intent::TogglePin { name: String::new(), target: Target::path("x") }));
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.groups.len(), 1, "an empty list still leaves the place pins go");
}

#[test]
fn the_registry_can_be_read() {
    // Windows' own version details are always there and hold both kinds of value. Only read.
    let found = read_key(HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    assert!(found.iter().any(|(name, value)| name == "ProductName" && matches!(value, Stored::Text(text) if text.starts_with("Windows"))));
    assert!(found.iter().any(|(name, value)| name == "CurrentMajorVersionNumber" && matches!(value, Stored::Number(10))));
    assert!(read_key(HKEY_CURRENT_USER, "Software\\StartlineNoSuchKey").is_empty());
}

#[test]
fn values_are_understood_the_way_the_templates_write_them() {
    let values = vec![
        ("Columns".to_string(), Stored::Number(5)),
        ("Corners".to_string(), Stored::Text("Square".to_string())),
        ("FrostedGlass".to_string(), Stored::Number(0)),
        ("SearchFolders".to_string(), Stored::Number(1)),
        ("SearchSkipFolders".to_string(), Stored::Number(1)),
        ("Sections".to_string(), Stored::Number(0)),
    ];
    let mut raw = Raw::default();
    merge_values(&mut raw, values, |list| {
        if list == "SearchFolders" { vec![("2".to_string(), Stored::Text("D:\\second".to_string())), ("1".to_string(), Stored::Text("C:\\first".to_string()))] } else { Vec::new() }
    });
    let policies = Policies::from_raw(&raw);
    let mut cfg = Config::default();
    policies.enforce(&mut cfg);
    assert_eq!(cfg.columns, 5);
    assert_eq!(cfg.corners, crate::config::CornerKind::Square);
    assert!(!cfg.blur);
    assert_eq!(cfg.roots, vec!["C:\\first", "D:\\second"], "in order");
    assert!(policies.is_set("SearchSkipFolders"), "an empty list can be meant");
    assert!(!policies.is_set("Sections"));
}

#[test]
fn nothing_set_changes_nothing() {
    let policies = Policies::from_raw(&Raw::default());
    assert!(policies.is_empty());
    let mut cfg = Config::default();
    let before = cfg.clone();
    policies.enforce(&mut cfg);
    assert_eq!(cfg, before);
    assert!(policies.allows_do(Do::Quit));
}

/// What's between each `open` and the next `close`, for every one in the text.
fn each_between<'a>(mut text: &'a str, open: &str, close: char) -> Vec<&'a str> {
    let mut found = Vec::new();
    while let Some(start) = text.find(open) {
        text = &text[start + open.len()..];
        let end = text.find(close).unwrap();
        found.push(&text[..end]);
        text = &text[end..];
    }
    found
}

#[test]
fn every_policy_is_in_the_templates_as_fixed_and_as_default() {
    let (admx, admls) = templates();
    let names: Vec<&str> = each_between(&admx, "<policy name=\"", '"');
    let unique: HashSet<&&str> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "two policies with one name");
    for policy in all() {
        assert!(admx.contains(&format!("<policy name=\"{}\"", policy.key)), "no fixed policy for {}", policy.key);
        assert_eq!(admx.contains(&format!("<policy name=\"Rec_{}\"", policy.key)), policy.has_default(), "default for {}", policy.key);
    }
    assert_eq!(names.len(), all().len() + all().iter().filter(|p| p.has_default()).count());
    assert!(admx.contains("key=\"Software\\Policies\\Startline\\Recommended\""), "defaults go under Recommended");
    for (lang, adml) in &admls {
        for id in each_between(&admx, "$(string.", ')') {
            assert!(adml.contains(&format!("<string id=\"{id}\">")), "{lang:?} has no words for {id}");
        }
        for id in each_between(&admx, "$(presentation.", ')') {
            assert!(adml.contains(&format!("<presentation id=\"{id}\">")), "{lang:?} has no layout for {id}");
        }
        for id in each_between(adml, "refId=\"", '"') {
            assert!(admx.contains(&format!(" id=\"{id}\"")), "layout for nothing: {id}");
        }
        let ids: Vec<&str> = each_between(adml, "<string id=\"", '"');
        let unique: HashSet<&&str> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "{lang:?}: two strings with one id");
    }
    let words = |lang: Lang| admls.iter().find(|(l, _)| *l == lang).unwrap().1.clone();
    assert!(words(Lang::De).contains("Sprache"), "the German one is in German");
    assert!(words(Lang::It).contains("Lingua"), "the Italian one in Italian");
}

#[test]
fn the_admin_guide_lists_every_value() {
    let guide = include_str!("../PolicyDefinitions/README.txt");
    for policy in all() {
        assert!(guide.contains(&format!("\n{}", policy.key)), "PolicyDefinitions/README.txt doesn't list {}", policy.key);
        if let Kind::Checks(items) = &policy.kind {
            for (id, _) in items {
                assert!(guide.contains(id.as_str()), "the guide doesn't list {id}");
            }
        }
    }
    assert!(guide.contains("Recommended"));
}

#[test]
fn the_template_files_are_up_to_date() {
    let (admx, admls) = templates();
    let same = |file: &str, text: &str| text.replace("\r\n", "\n") == file.replace("\r\n", "\n");
    let fix = "run startline --write-policy-templates PolicyDefinitions";
    assert!(same(include_str!("../PolicyDefinitions/Startline.admx"), &admx), "{fix}");
    for (lang, adml) in admls {
        let file = match lang {
            Lang::En => include_str!("../PolicyDefinitions/en-US/Startline.adml"),
            Lang::De => include_str!("../PolicyDefinitions/de-DE/Startline.adml"),
            Lang::It => include_str!("../PolicyDefinitions/it-IT/Startline.adml"),
        };
        assert!(same(file, &adml), "{lang:?}: {fix}");
    }
    assert!(same(include_str!("../PolicyDefinitions/README.txt"), &guide()), "{fix}");
}
