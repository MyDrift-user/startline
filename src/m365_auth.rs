//! Signing in. Windows's account broker (WAM) first: Windows keeps the tokens, and Startline only the account's id.
//! Where the broker isn't there, the system browser with an authorization code and PKCE on a loopback address, and the
//! refresh token kept encrypted for this Windows account with DPAPI, never in plain text.

use super::graph::{GraphError, http};
use crate::i18n::tr;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

// ---- Keeping secrets ----

/// Encrypts for this Windows account on this PC only.
pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData};
    let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
        let sealed = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        Some(sealed)
    }
}

pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData};
    let input = CRYPT_INTEGER_BLOB { cbData: sealed.len() as u32, pbData: sealed.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
        let plain = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        Some(plain)
    }
}

fn random_bytes(count: usize) -> Vec<u8> {
    use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
    let mut bytes = vec![0u8; count];
    let _ = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    bytes
}

/// Base64 for addresses: no padding, - and _ instead of + and /.
pub fn base64url(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 2);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..=chunk.len() {
            out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// The PKCE challenge for a verifier: its SHA-256, in base64url.
pub fn pkce_challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

// ---- What went wrong ----

#[derive(Clone, Debug, PartialEq)]
pub enum SignInError {
    /// The person closed the sign-in.
    Cancelled,
    /// An administrator has to approve Startline for the organization first.
    AdminConsent,
    /// Windows's account broker can't be used here: try the browser.
    NoBroker,
    Failed(String),
}

fn failure(message: &str) -> SignInError {
    if super::needs_admin_consent(message) { SignInError::AdminConsent } else { SignInError::Failed(message.to_string()) }
}

// ---- Windows's account broker ----

const BROKER: &str = "https://login.microsoftonline.com/";

fn broker_provider(tenant: &str) -> windows::core::Result<windows::Security::Credentials::WebAccountProvider> {
    use windows::Security::Authentication::Web::Core::WebAuthenticationCoreManager;
    use windows::core::HSTRING;
    // The broker's work and school accounts are "organizations"; the request itself still names "common" or the tenant.
    let authority = if tenant.eq_ignore_ascii_case("common") { "organizations" } else { tenant };
    WebAuthenticationCoreManager::FindAccountProviderWithAuthorityAsync(&HSTRING::from(BROKER), &HSTRING::from(authority))?.join()
}

fn broker_request(tenant: &str, client_id: &str, scopes: &[&str], prompt: bool) -> windows::core::Result<windows::Security::Authentication::Web::Core::WebTokenRequest> {
    use windows::Security::Authentication::Web::Core::{WebTokenRequest, WebTokenRequestPromptType};
    use windows::core::HSTRING;
    let provider = broker_provider(tenant)?;
    let kind = if prompt { WebTokenRequestPromptType::ForceAuthentication } else { WebTokenRequestPromptType::Default };
    let request = WebTokenRequest::CreateWithPromptType(&provider, &HSTRING::from(scopes.join(" ")), &HSTRING::from(client_id), kind)?;
    let properties = request.Properties()?;
    properties.Insert(&HSTRING::from("authority"), &HSTRING::from(format!("{BROKER}{tenant}")))?;
    // Scopes rather than a resource: the work and school account broker answers v2.0 requests only when asked to.
    properties.Insert(&HSTRING::from("wam_compat"), &HSTRING::from("2.0"))?;
    Ok(request)
}

/// What a finished broker request came to: the token and the account's id.
fn broker_result(result: &windows::Security::Authentication::Web::Core::WebTokenRequestResult) -> Result<(String, String), SignInError> {
    use windows::Security::Authentication::Web::Core::WebTokenRequestStatus;
    let status = result.ResponseStatus().map_err(|e| SignInError::Failed(e.message().to_string()))?;
    if status == WebTokenRequestStatus::Success {
        let response = result.ResponseData().and_then(|data| data.GetAt(0)).map_err(|e| SignInError::Failed(e.message().to_string()))?;
        let token = response.Token().map(|t| t.to_string()).unwrap_or_default();
        let account = response.WebAccount().and_then(|a| a.Id()).map(|id| id.to_string()).unwrap_or_default();
        return Ok((token, account));
    }
    if status == WebTokenRequestStatus::UserCancel {
        return Err(SignInError::Cancelled);
    }
    if status == WebTokenRequestStatus::AccountProviderNotAvailable {
        return Err(SignInError::NoBroker);
    }
    if status == WebTokenRequestStatus::UserInteractionRequired {
        return Err(SignInError::Failed(tr("Sign in again to allow what's switched on.").into()));
    }
    let message = result.ResponseError().and_then(|e| e.ErrorMessage()).map(|m| m.to_string()).unwrap_or_default();
    Err(failure(&message))
}

/// Signs in with the broker, over the window `hwnd`. The token and the account's id.
pub fn broker_sign_in(hwnd: isize, tenant: &str, client_id: &str, scopes: &[&str]) -> Result<(String, String), SignInError> {
    use windows::Security::Authentication::Web::Core::{WebAuthenticationCoreManager, WebTokenRequestResult};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IWebAuthenticationCoreManagerInterop;
    use windows_future::IAsyncOperation;
    let request = broker_request(tenant, client_id, scopes, false).map_err(|_| SignInError::NoBroker)?;
    let interop = windows::core::factory::<WebAuthenticationCoreManager, IWebAuthenticationCoreManagerInterop>().map_err(|_| SignInError::NoBroker)?;
    let operation: IAsyncOperation<WebTokenRequestResult> = unsafe { interop.RequestTokenForWindowAsync(HWND(hwnd as *mut _), &request) }.map_err(|_| SignInError::NoBroker)?;
    let result = operation.join().map_err(|e| SignInError::Failed(e.message().to_string()))?;
    broker_result(&result)
}

/// A token for the account, without asking the person, as long as Windows can.
pub fn broker_token(tenant: &str, client_id: &str, scopes: &[&str], account_id: &str) -> Result<String, GraphError> {
    use windows::Security::Authentication::Web::Core::WebAuthenticationCoreManager;
    use windows::core::HSTRING;
    let run = || -> windows::core::Result<Result<String, GraphError>> {
        let provider = broker_provider(tenant)?;
        let account = WebAuthenticationCoreManager::FindAccountAsync(&provider, &HSTRING::from(account_id))?.join()?;
        let request = broker_request(tenant, client_id, scopes, false)?;
        let result = WebAuthenticationCoreManager::GetTokenSilentlyWithWebAccountAsync(&request, &account)?.join()?;
        Ok(match broker_result(&result) {
            Ok((token, _)) => Ok(token),
            Err(SignInError::AdminConsent) => Err(GraphError::AdminConsent),
            Err(_) => Err(GraphError::SignInNeeded),
        })
    };
    run().unwrap_or(Err(GraphError::SignInNeeded))
}

/// Tells Windows to forget Startline's tokens for the account.
pub fn broker_forget(tenant: &str, client_id: &str, account_id: &str) {
    use windows::Security::Authentication::Web::Core::WebAuthenticationCoreManager;
    use windows::core::HSTRING;
    let run = || -> windows::core::Result<()> {
        let provider = broker_provider(tenant)?;
        let account = WebAuthenticationCoreManager::FindAccountAsync(&provider, &HSTRING::from(account_id))?.join()?;
        account.SignOutWithClientIdAsync(&HSTRING::from(client_id))?.join()
    };
    let _ = run();
}

// ---- The browser, with PKCE on a loopback address ----

fn login(tenant: &str) -> String {
    format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0")
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={}", crate::search::percent_encode(v))).collect::<Vec<_>>().join("&")
}

/// The tokens a sign-in or a refresh gives.
#[derive(Clone, Debug, PartialEq)]
pub struct Granted {
    pub access: String,
    pub refresh: String,
    /// Seconds it lasts.
    pub lasts: u64,
}

pub fn parse_granted(status: u16, body: &str) -> Result<Granted, SignInError> {
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    if (200..300).contains(&status) {
        if let Some(access) = json["access_token"].as_str() {
            return Ok(Granted { access: access.to_string(), refresh: json["refresh_token"].as_str().unwrap_or("").to_string(), lasts: json["expires_in"].as_u64().unwrap_or(3600) });
        }
    }
    let described = format!("{} {}", json["error"].as_str().unwrap_or(""), json["error_description"].as_str().unwrap_or(""));
    Err(failure(described.trim()))
}

/// A new access token from a refresh token.
pub fn refresh(tenant: &str, client_id: &str, scopes: &[&str], refresh_token: &str) -> Result<Granted, GraphError> {
    let scope = format!("{} offline_access", scopes.join(" "));
    let body = form(&[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("scope", &scope)]);
    let (status, text) = http("POST", &format!("{}/token", login(tenant)), &[], Some(("application/x-www-form-urlencoded", &body))).map_err(GraphError::Offline)?;
    parse_granted(status, &text).map_err(|e| if e == SignInError::AdminConsent { GraphError::AdminConsent } else { GraphError::SignInNeeded })
}

/// The `code` and `state`, or the `error` and its description, of the address the browser came back to.
pub fn parse_redirect(request_line: &str) -> Vec<(String, String)> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map_or("", |(_, q)| q);
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (percent_decode(k), percent_decode(v)))
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Signs in through the system browser. Gives the tokens; waits at most five minutes for the person.
pub fn browser_sign_in(tenant: &str, client_id: &str, scopes: &[&str]) -> Result<Granted, SignInError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| SignInError::Failed(e.to_string()))?;
    let port = listener.local_addr().map_err(|e| SignInError::Failed(e.to_string()))?.port();
    let redirect = format!("http://localhost:{port}");
    let verifier = base64url(&random_bytes(32));
    let state = base64url(&random_bytes(16));
    let scope = format!("{} offline_access", scopes.join(" "));
    let address = format!(
        "{}/authorize?{}",
        login(tenant),
        form(&[("client_id", client_id), ("response_type", "code"), ("redirect_uri", &redirect), ("scope", &scope), ("code_challenge", &pkce_challenge(&verifier)), ("code_challenge_method", "S256"), ("state", &state), ("prompt", "select_account")])
    );
    if !crate::platform::shell_open("", &address, "") {
        return Err(SignInError::Failed(tr("The browser couldn't be opened.").into()));
    }
    listener.set_nonblocking(true).map_err(|e| SignInError::Failed(e.to_string()))?;
    let started = Instant::now();
    // Browsers open spare connections ahead and ask for a favicon: wait for the request that carries the answer.
    let (mut stream, params) = loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let params = parse_redirect(request.lines().next().unwrap_or(""));
                if params.iter().any(|(k, _)| k == "code" || k == "error") {
                    break (stream, params);
                }
                let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"); // i18n-ignore: an HTTP answer
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && started.elapsed() < Duration::from_secs(300) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => return Err(SignInError::Cancelled),
        }
    };
    let get = |key: &str| params.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let page = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Startline</title><body style=\"font-family:Segoe UI,sans-serif;margin:3em\"><p>{}</p></body>", // i18n-ignore: a page of HTML
        if get("code").is_some() { tr("Signed in. You can close this tab and go back to Startline.") } else { tr("The sign-in didn't finish. You can close this tab and try again in Startline.") }
    );
    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len()); // i18n-ignore: an HTTP answer
    if let Some(error) = get("error") {
        let described = format!("{error} {}", get("error_description").unwrap_or_default());
        return Err(if error == "access_denied" && !super::needs_admin_consent(&described) { SignInError::Cancelled } else { failure(&described) });
    }
    if get("state").as_deref() != Some(state.as_str()) {
        return Err(SignInError::Failed(tr("The sign-in came back changed, so it was ignored.").into()));
    }
    let code = get("code").ok_or(SignInError::Cancelled)?;
    let body = form(&[("client_id", client_id), ("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", &redirect), ("code_verifier", &verifier), ("scope", &scope)]);
    let (status, text) = http("POST", &format!("{}/token", login(tenant)), &[], Some(("application/x-www-form-urlencoded", &body))).map_err(SignInError::Failed)?;
    parse_granted(status, &text)
}
