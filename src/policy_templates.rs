//! The Group Policy templates, in English, German and Italian, and the admin guide, all written from the one list.

use super::*;
use std::path::Path;

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn explain(policy: &Policy, lang: Lang, default: bool) -> String {
    let what = match (&policy.kind, default) {
        (Kind::Checks(_), _) => tx("Enabled applies the ticks below. Disabled or Not configured allows everything."),
        (Kind::Bool, false) => tx("Enabled turns this on and Disabled turns it off, and people can't change it. Not configured leaves it to them."),
        (Kind::AddList, false) => tx("Enabled adds the items below to people's own, and they can't remove them. They can still add more. Disabled or Not configured leaves it to them."),
        (_, false) => tx("Enabled sets it to the value below, and people can't change it. Disabled or Not configured leaves it to them."),
        (Kind::Bool, true) => tx("A default people can change: Enabled turns this on and Disabled turns it off until they change it themselves. Not configured leaves Startline's own default."),
        (_, true) => tx("A default people can change: Enabled sets it to the value below until they change it themselves. Disabled or Not configured leaves Startline's own default."),
    };
    let what = tr_in(lang, what);
    if policy.about.is_empty() { what.to_string() } else { format!("{}\n\n{what}", policy.about) }
}

/// The ADMX, which names everything, and for each language its ADML with the words.
pub fn templates() -> (String, Vec<(Lang, String)>) {
    let mut admx = String::new();
    admx.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<policyDefinitions xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" revision=\"1.1\" schemaVersion=\"1.0\" xmlns=\"http://schemas.microsoft.com/GroupPolicy/2006/07/PolicyDefinitions\">\n");
    admx.push_str("  <policyNamespaces>\n    <target prefix=\"startline\" namespace=\"Startline.Policies.Startline\"/>\n  </policyNamespaces>\n"); // i18n-ignore: template XML
    admx.push_str("  <resources minRequiredRevision=\"1.1\" fallbackCulture=\"en-us\"/>\n"); // i18n-ignore: template XML
    admx.push_str("  <supportedOn>\n    <definitions>\n      <definition name=\"SUPPORTED_Startline\" displayName=\"$(string.Supported)\"/>\n    </definitions>\n  </supportedOn>\n"); // i18n-ignore: template XML
    admx.push_str("  <categories>\n");
    // Fixed settings under Startline, and the defaults the same way under a category of their own inside it.
    for (prefix, top) in [("", "Startline"), ("Rec_", "Rec_Startline")] { // i18n-ignore: template XML
        let parent = if prefix.is_empty() { String::new() } else { "\n      <parentCategory ref=\"Startline\"/>\n    ".to_string() }; // i18n-ignore: template XML
        admx.push_str(&format!("    <category name=\"{top}\" displayName=\"$(string.Cat_{top})\">{parent}</category>\n")); // i18n-ignore: template XML
        admx.push_str(&format!("    <category name=\"{prefix}Search\" displayName=\"$(string.Cat_Search)\">\n      <parentCategory ref=\"{top}\"/>\n    </category>\n")); // i18n-ignore: template XML
        for (_, id, _, parent) in Group::ALL {
            let parent = if parent == "Startline" { top.to_string() } else { format!("{prefix}{parent}") }; // i18n-ignore: template XML
            admx.push_str(&format!("    <category name=\"{prefix}{id}\" displayName=\"$(string.Cat_{id})\">\n      <parentCategory ref=\"{parent}\"/>\n    </category>\n")); // i18n-ignore: template XML
        }
    }
    admx.push_str("  </categories>\n  <policies>\n");

    for policy in all() {
        let key = &policy.key;
        let group = Group::ALL.iter().find(|(g, _, _, _)| *g == policy.group).map_or("", |(_, id, _, _)| id);
        let mut elements = String::new();
        let mut own_value = String::new();
        let mut values = String::new();
        let list_elements = |under: &str| format!("        <list id=\"Value\" key=\"{under}\\{key}\" valuePrefix=\"\" expandable=\"true\"/>\n");
        match &policy.kind {
            Kind::Bool => {
                own_value = format!(" valueName=\"{key}\""); // i18n-ignore: template XML
                values = "      <enabledValue>\n        <decimal value=\"1\"/>\n      </enabledValue>\n      <disabledValue>\n        <decimal value=\"0\"/>\n      </disabledValue>\n".to_string(); // i18n-ignore: template XML
            }
            Kind::Number { min, max, .. } => {
                elements = format!("        <decimal id=\"Value\" valueName=\"{key}\" required=\"true\" minValue=\"{min}\" maxValue=\"{max}\"/>\n"); // i18n-ignore: template XML
            }
            Kind::Choice(items) => {
                elements.push_str(&format!("        <enum id=\"Value\" valueName=\"{key}\" required=\"true\">\n")); // i18n-ignore: template XML
                for (index, (value, _)) in items.iter().enumerate() {
                    elements.push_str(&format!("          <item displayName=\"$(string.I_{key}_{index})\">\n            <value>\n              <string>{}</string>\n            </value>\n          </item>\n", xml(value)));
                }
                elements.push_str("        </enum>\n");
            }
            Kind::Text(_) | Kind::Color(_) => {
                elements = format!("        <text id=\"Value\" valueName=\"{key}\" required=\"true\"/>\n"); // i18n-ignore: template XML
            }
            Kind::List | Kind::AddList => {
                own_value = format!(" valueName=\"{key}\""); // i18n-ignore: template XML
                values = "      <enabledValue>\n        <decimal value=\"1\"/>\n      </enabledValue>\n      <disabledValue>\n        <delete/>\n      </disabledValue>\n".to_string(); // i18n-ignore: template XML
                elements = list_elements(KEY);
            }
            Kind::Checks(items) => {
                for (id, _) in items {
                    elements.push_str(&format!("        <boolean id=\"{id}\" valueName=\"Allow_{id}\">\n          <trueValue>\n            <decimal value=\"1\"/>\n          </trueValue>\n          <falseValue>\n            <decimal value=\"0\"/>\n          </falseValue>\n        </boolean>\n")); // i18n-ignore: template XML
                }
            }
        }
        let shows = if matches!(policy.kind, Kind::Bool) { String::new() } else { format!(" presentation=\"$(presentation.P_{key})\"") };
        let write = |admx: &mut String, name: &str, explain: &str, category: &str, at: &str, elements: &str| {
            admx.push_str(&format!("    <policy name=\"{name}\" class=\"Both\" displayName=\"$(string.P_{key})\" explainText=\"$(string.{explain})\"{shows} key=\"{at}\"{own_value}>\n")); // i18n-ignore: template XML
            admx.push_str(&format!("      <parentCategory ref=\"{category}\"/>\n      <supportedOn ref=\"SUPPORTED_Startline\"/>\n{values}")); // i18n-ignore: template XML
            if !elements.is_empty() {
                admx.push_str(&format!("      <elements>\n{elements}      </elements>\n"));
            }
            admx.push_str("    </policy>\n");
        };
        write(&mut admx, key, &format!("E_{key}"), group, KEY, &elements);
        if policy.has_default() {
            let elements = if matches!(policy.kind, Kind::List | Kind::AddList) { list_elements(RECOMMENDED) } else { elements.clone() };
            write(&mut admx, &format!("Rec_{key}"), &format!("R_{key}"), &format!("Rec_{group}"), RECOMMENDED, &elements); // i18n-ignore: template XML
        }
    }
    admx.push_str("  </policies>\n</policyDefinitions>\n"); // i18n-ignore: template XML

    let adml = Lang::ALL.iter().map(|&lang| (lang, adml_in(lang))).collect();
    (admx, adml)
}

/// One language's words for the templates.
fn adml_in(lang: Lang) -> String {
    let t = |en: &'static str| tr_in(lang, en);
    let mut strings = String::new();
    let mut shown = String::new();
    let mut say = |id: &str, text: &str| strings.push_str(&format!("      <string id=\"{id}\">{}</string>\n", xml(text)));
    say("Supported", t(tx("Startline 0.1 and later"))); // i18n-ignore: template XML
    say("Cat_Startline", "Startline"); // i18n-ignore: template XML
    say("Cat_Rec_Startline", t(tx("Defaults people can change")));
    say("Cat_Search", t(tx("Search")));
    for (_, id, label, _) in Group::ALL {
        say(&format!("Cat_{id}"), t(label)); // i18n-ignore: template XML
    }
    for policy in build(lang) {
        let key = &policy.key;
        say(&format!("P_{key}"), &policy.title);
        say(&format!("E_{key}"), &explain(&policy, lang, false));
        if policy.has_default() {
            say(&format!("R_{key}"), &explain(&policy, lang, true));
        }
        let label = xml(&i18n::fill(t(tx("{name}:")), &[("name", &policy.title)]));
        let presentation = match &policy.kind {
            Kind::Bool => String::new(),
            Kind::Number { default, .. } => format!("        <decimalTextBox refId=\"Value\" defaultValue=\"{default}\">{label}</decimalTextBox>\n"), // i18n-ignore: template XML
            Kind::Choice(items) => {
                for (index, (_, text)) in items.iter().enumerate() {
                    say(&format!("I_{key}_{index}"), text);
                }
                format!("        <dropdownList refId=\"Value\" noSort=\"true\" defaultItem=\"0\">{label}</dropdownList>\n") // i18n-ignore: template XML
            }
            Kind::Text(default) | Kind::Color(default) => format!("        <textBox refId=\"Value\">\n          <label>{label}</label>\n          <defaultValue>{}</defaultValue>\n        </textBox>\n", xml(default)),
            Kind::List | Kind::AddList => format!("        <listBox refId=\"Value\">{label}</listBox>\n"), // i18n-ignore: template XML
            Kind::Checks(items) => items.iter().map(|(id, text)| format!("        <checkBox refId=\"{id}\" defaultChecked=\"true\">{}</checkBox>\n", xml(text))).collect(),
        };
        if !presentation.is_empty() {
            shown.push_str(&format!("      <presentation id=\"P_{key}\">\n{presentation}      </presentation>\n")); // i18n-ignore: template XML
        }
    }
    let description = xml(t(tx("Settings for the Startline Start menu and search.")));
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<policyDefinitionResources xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" revision=\"1.1\" schemaVersion=\"1.0\" xmlns=\"http://schemas.microsoft.com/GroupPolicy/2006/07/PolicyDefinitions\">\n  <displayName>Startline</displayName>\n  <description>{description}</description>\n  <resources>\n    <stringTable>\n{strings}    </stringTable>\n    <presentationTable>\n{shown}    </presentationTable>\n  </resources>\n</policyDefinitionResources>\n"
    )
}

/// What a value holds, for the guide. The guide is for administrators and is written in English.
fn kind_in_guide(policy: &Policy) -> String {
    match &policy.kind {
        Kind::Bool => "REG_DWORD, 1 on, 0 off".to_string(), // i18n-ignore: admin guide
        Kind::Number { min, max, divide, .. } if *divide > 1 => format!("REG_DWORD, {min} to {max} (percent)"), // i18n-ignore: admin guide
        Kind::Number { min, max, .. } => format!("REG_DWORD, {min} to {max}"), // i18n-ignore: admin guide
        Kind::Choice(items) => format!("REG_SZ, one of: {}", items.iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>().join(", ")), // i18n-ignore: admin guide
        Kind::Text(_) => "REG_SZ".to_string(),
        Kind::Color(_) => "REG_SZ, #RRGGBB".to_string(),
        Kind::List => "list: REG_DWORD 1, its items in a subkey of the same name".to_string(), // i18n-ignore: admin guide
        Kind::AddList => "list: REG_DWORD 1, its items in a subkey of the same name; fixed, they're added to people's own".to_string(), // i18n-ignore: admin guide
        Kind::Checks(items) => format!("REG_DWORD values named Allow_<item>, 1 allowed, 0 removed. Items: {}", items.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(", ")), // i18n-ignore: admin guide
    }
}

const GUIDE_INTRO: &str = include_str!("../PolicyDefinitions/guide-intro.txt");

/// PolicyDefinitions/README.txt: how policies work, then every value.
pub fn guide() -> String {
    let mut out = GUIDE_INTRO.replace("\r\n", "\n");
    out.push_str("\nEVERY VALUE\n===========\n"); // i18n-ignore: admin guide
    out.push_str("The same names work under Software\\Policies\\Startline (fixed) and under Software\\Policies\\Startline\\Recommended\n(defaults people can change). Values marked [fixed only] can't be defaults.\n"); // i18n-ignore: admin guide
    for (group, _, label, parent) in Group::ALL {
        let section = if parent == "Search" { format!("Search: {label}") } else { label.to_string() }; // i18n-ignore: admin guide
        out.push_str(&format!("\n{}\n{}\n", section.to_uppercase(), "-".repeat(section.chars().count())));
        for policy in all().iter().filter(|p| p.group == group) {
            let only = if policy.has_default() { "" } else { "  [fixed only]" }; // i18n-ignore: admin guide
            out.push_str(&format!("{}{only}\n    {}\n    {}\n", policy.key, policy.title, kind_in_guide(policy)));
            if !policy.about.is_empty() {
                out.push_str(&format!("    {}\n", policy.about));
            }
        }
    }
    out
}

/// Writes the templates into a folder laid out like Windows' own PolicyDefinitions, with the guide.
pub fn write_templates(folder: &Path) -> std::io::Result<()> {
    let (admx, admls) = templates();
    std::fs::create_dir_all(folder)?;
    std::fs::write(folder.join("Startline.admx"), admx)?; // i18n-ignore: file name
    for (lang, adml) in admls {
        std::fs::create_dir_all(folder.join(lang.culture()))?;
        std::fs::write(folder.join(lang.culture()).join("Startline.adml"), adml)?; // i18n-ignore: file name
    }
    std::fs::write(folder.join("README.txt"), guide())
}
