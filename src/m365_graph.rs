//! Microsoft Graph, behind a small trait so everything that reads it is tested with recorded answers. The real one
//! talks to graph.microsoft.com and nowhere else.

use serde_json::Value;

pub const GRAPH: &str = "https://graph.microsoft.com/v1.0";

#[derive(Clone, Debug, PartialEq)]
pub enum GraphError {
    /// No one is signed in, or the sign-in needs the person again (a new permission, an expired session).
    SignInNeeded,
    /// The organization doesn't let people agree to Startline's permissions themselves: an administrator has to.
    AdminConsent,
    /// Graph answered, with an error: its status and message.
    Refused(u16, String),
    /// Graph couldn't be reached.
    Offline(String),
}

/// Reading and writing Graph as the signed-in person. Paths start after /v1.0, such as `/me/events`.
pub trait Graph: Send + Sync {
    fn get(&self, path: &str, headers: &[(&str, &str)]) -> Result<String, GraphError>;
    fn post(&self, path: &str, body: &str) -> Result<String, GraphError>;
}

/// One HTTPS request through Windows's own HTTP stack: the status and the body.
pub fn http(method: &str, url: &str, headers: &[(&str, &str)], body: Option<(&str, &str)>) -> Result<(u16, String), String> {
    use windows::Foundation::Uri;
    use windows::Web::Http::{HttpClient, HttpMethod, HttpRequestMessage, HttpStringContent};
    use windows::core::HSTRING;
    let run = || -> windows::core::Result<(u16, String)> {
        let client = HttpClient::new()?;
        let verb = if method == "POST" { HttpMethod::Post()? } else { HttpMethod::Get()? };
        let request = HttpRequestMessage::Create(&verb, &Uri::CreateUri(&HSTRING::from(url))?)?;
        let request_headers = request.Headers()?;
        // Never an answer from a cache: a calendar or a sign-in is about now.
        request_headers.TryAppendWithoutValidation(&HSTRING::from("Cache-Control"), &HSTRING::from("no-cache"))?;
        for (name, value) in headers {
            request_headers.TryAppendWithoutValidation(&HSTRING::from(*name), &HSTRING::from(*value))?;
        }
        if let Some((kind, text)) = body {
            let content = HttpStringContent::CreateFromStringWithEncodingAndMediaType(&HSTRING::from(text), windows::Storage::Streams::UnicodeEncoding::Utf8, &HSTRING::from(kind))?;
            request.SetContent(&content)?;
        }
        let response = client.SendRequestAsync(&request)?.join()?;
        let status = response.StatusCode()?.0 as u16;
        let text = response.Content()?.ReadAsStringAsync()?.join()?.to_string();
        Ok((status, text))
    };
    run().map_err(|e| e.message().to_string())
}

/// Something that hands out an access token for Graph, getting a new one when it has to.
pub trait Tokens: Send + Sync {
    fn token(&self) -> Result<String, GraphError>;
    /// The token was refused: the next one has to be new.
    fn forget(&self);
}

/// The real Graph, with tokens from the sign-in.
pub struct HttpGraph {
    pub tokens: std::sync::Arc<dyn Tokens>,
}

impl HttpGraph {
    fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&str>) -> Result<String, GraphError> {
        for attempt in 0..2 {
            let bearer = format!("Bearer {}", self.tokens.token()?);
            let mut all: Vec<(&str, &str)> = vec![("Authorization", bearer.as_str())]; // i18n-ignore: an HTTP header
            all.extend_from_slice(headers);
            let (status, text) = http(method, &format!("{GRAPH}{path}"), &all, body.map(|b| ("application/json", b))).map_err(GraphError::Offline)?;
            if (200..300).contains(&status) {
                return Ok(text);
            }
            // An access token can run out between asking for it and using it: one more go with a new one.
            if status == 401 && attempt == 0 {
                self.tokens.forget();
                continue;
            }
            return Err(error_of(status, &text));
        }
        Err(GraphError::SignInNeeded)
    }
}

impl Graph for HttpGraph {
    fn get(&self, path: &str, headers: &[(&str, &str)]) -> Result<String, GraphError> {
        self.send("GET", path, headers, None)
    }

    fn post(&self, path: &str, body: &str) -> Result<String, GraphError> {
        self.send("POST", path, &[], Some(body))
    }
}

/// What an error answer from Graph or from the sign-in means.
pub fn error_of(status: u16, body: &str) -> GraphError {
    let json: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let message = json["error"]["message"].as_str().or_else(|| json["error_description"].as_str()).unwrap_or("").to_string();
    let code = json["error"]["code"].as_str().or_else(|| json["error"].as_str()).unwrap_or("");
    if crate::m365::needs_admin_consent(&message) || crate::m365::needs_admin_consent(code) {
        GraphError::AdminConsent
    } else if status == 401 || code == "InvalidAuthenticationToken" || code == "interaction_required" || code == "invalid_grant" {
        GraphError::SignInNeeded
    } else {
        GraphError::Refused(status, if message.is_empty() { code.to_string() } else { message })
    }
}

/// Words typed into a Graph $search: no quotes or backslashes of their own.
fn search_words(text: &str) -> String {
    text.chars().filter(|c| *c != '"' && *c != '\\').collect::<String>().trim().to_string()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub name: String,
    pub title: String,
    pub mail: String,
    /// The sign-in name, which Teams finds people by.
    pub upn: String,
}

impl Person {
    /// Who to chat with or call in Teams: the sign-in name, or the mail without one.
    pub fn teams_id(&self) -> &str {
        if self.upn.is_empty() { &self.mail } else { &self.upn }
    }
}

/// Colleagues whose name or mail starts with the words, from the organization's directory.
pub fn people_path(query: &str) -> String {
    let words = search_words(query);
    let search = format!("\"displayName:{words}\" OR \"mail:{words}\""); // i18n-ignore: Graph search syntax
    format!("/users?$search={}&$select=displayName,jobTitle,mail,userPrincipalName&$top=8", crate::search::percent_encode(&search))
}

/// Directory searches need this header.
pub const PEOPLE_HEADERS: [(&str, &str); 1] = [("ConsistencyLevel", "eventual")];

pub fn parse_people(json: &str) -> Vec<Person> {
    let value: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let text = |v: &Value, key: &str| v[key].as_str().unwrap_or("").trim().to_string();
    value["value"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|p| Person { name: text(p, "displayName"), title: text(p, "jobTitle"), mail: text(p, "mail"), upn: text(p, "userPrincipalName") })
                .filter(|p| !p.name.is_empty() && !(p.upn.is_empty() && p.mail.is_empty()))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub subject: String,
    pub from: String,
    /// When it arrived, in seconds since 1970.
    pub received: u64,
    pub web_link: String,
}

/// The person's own mail holding the words, newest first.
pub fn mail_path(query: &str) -> String {
    let search = format!("\"{}\"", search_words(query));
    format!("/me/messages?$search={}&$top=5&$select=subject,from,receivedDateTime,webLink", crate::search::percent_encode(&search))
}

pub fn parse_mail(json: &str) -> Vec<Message> {
    let value: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    value["value"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|m| {
                    let web_link = m["webLink"].as_str()?.to_string();
                    let sender = &m["from"]["emailAddress"];
                    let from = sender["name"].as_str().filter(|n| !n.trim().is_empty()).or_else(|| sender["address"].as_str()).unwrap_or("").to_string();
                    let received = m["receivedDateTime"].as_str().and_then(crate::m365::calendar::utc_secs).unwrap_or(0);
                    Some(Message { subject: m["subject"].as_str().unwrap_or("").trim().to_string(), from, received, web_link })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The signed-in person, as Graph's /me tells it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Me {
    pub id: String,
    pub name: String,
    pub mail: String,
}

pub const ME_PATH: &str = "/me?$select=id,displayName,mail,userPrincipalName";

pub fn parse_me(json: &str) -> Option<Me> {
    let value: Value = serde_json::from_str(json).ok()?;
    let id = value["id"].as_str()?.to_string();
    let mail = value["mail"].as_str().filter(|m| !m.is_empty()).or_else(|| value["userPrincipalName"].as_str()).unwrap_or("").to_string();
    Some(Me { id, name: value["displayName"].as_str().unwrap_or("").to_string(), mail })
}
