/**
 * Google sign-in (OAuth 2 with PKCE) for Gmail.
 *
 * The sign-in page opens in the user's default browser, so Mail Sage never
 * sees the Google password. Google sends the result to a listener on
 * 127.0.0.1 that exists only for the time of the sign-in. Mail Sage then
 * gets a refresh token (kept in Windows Credential Manager) and short-lived
 * access tokens (kept in memory only).
 */
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

pub const IMAP_HOST: &str = "imap.gmail.com";
pub const IMAP_PORT: u16 = 993;
pub const SMTP_HOST: &str = "smtp.gmail.com";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
/** Full mail access (IMAP and SMTP need it), plus the account's email address. */
const MAIL_SCOPE: &str = "https://mail.google.com/";
const SCOPES: &str = "https://mail.google.com/ openid email";
const USER_AGENT: &str = "MailSage";

/** How long the user has to finish the sign-in in the browser. */
const SIGN_IN_TIME_LIMIT: Duration = Duration::from_secs(300);
const REQUEST_TIME_LIMIT: Duration = Duration::from_secs(5);
const HTTP_TIME_LIMIT: Duration = Duration::from_secs(30);
const MAX_REQUEST_BYTES: usize = 8192;

#[cfg(google_oauth_embedded)]
fn client() -> Option<(String, String)> {
    Some((
        obfstr::obfstr!(env!("GOOGLE_CLIENT_ID")).to_string(),
        obfstr::obfstr!(env!("GOOGLE_CLIENT_SECRET")).to_string(),
    ))
}

#[cfg(not(google_oauth_embedded))]
fn client() -> Option<(String, String)> {
    None
}

/** True when this build has the Google sign-in client values (docs/GOOGLE-SETUP.md). */
pub fn available() -> bool {
    client().is_some()
}

fn require_client() -> Result<(String, String)> {
    client().context("Google sign-in is not set up in this build of Mail Sage")
}

pub struct SignIn {
    pub email: String,
    pub refresh_token: String,
    pub access_token: String,
    /** Seconds until the access token stops working. */
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
    id_token: Option<String>,
    /** The permissions that the user really gave, separated by spaces. The user can untick the mail permission on Google's page. */
    scope: Option<String>,
}

/** True when the permissions that Google lists include full mail access. */
fn has_mail_scope(scope: Option<&str>) -> bool {
    scope.is_some_and(|scope| scope.split(' ').any(|part| part == MAIL_SCOPE))
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::fill(&mut bytes[..]);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent(USER_AGENT).timeout(HTTP_TIME_LIMIT).build()?)
}

/** What the browser's request to the listener says. */
#[derive(Debug, PartialEq)]
enum Callback {
    /** Google gave a sign-in code. */
    Code(String),
    /** Google reports that the sign-in did not complete (for example, the user said no). */
    Refused(String),
    /** A request that is not the sign-in result (a browser's icon request, or a wrong state value). */
    NotOurs,
}

/** Reads the first line of the browser's request, for example `GET /?state=..&code=.. HTTP/1.1`. */
fn read_callback(request: &str, expected_state: &str) -> Callback {
    let mut parts = request.lines().next().unwrap_or("").split(' ');
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Callback::NotOurs;
    };
    let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Callback::NotOurs;
    };
    let value = |name: &str| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned());
    if value("state").as_deref() != Some(expected_state) {
        return Callback::NotOurs;
    }
    if let Some(error) = value("error") {
        return Callback::Refused(error);
    }
    match value("code") {
        Some(code) if !code.is_empty() => Callback::Code(code),
        _ => Callback::NotOurs,
    }
}

async fn read_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 1024];
    while data.len() < MAX_REQUEST_BYTES && !data.windows(4).any(|window| window == b"\r\n\r\n") {
        match tokio::time::timeout(REQUEST_TIME_LIMIT, stream.read(&mut buffer)).await {
            Ok(Ok(count)) if count > 0 => data.extend_from_slice(&buffer[..count]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

async fn respond(stream: &mut TcpStream, status: &str, text: &str) {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Mail Sage</title></head>\
         <body style=\"font-family:Segoe UI,sans-serif;background:#1d2029;color:#d4d4d8;text-align:center;padding-top:80px\">\
         <h2>{text}</h2><p>You can close this tab.</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/** Waits for the browser to bring the sign-in result to the listener. */
async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await.context("the sign-in listener failed")?;
        let request = read_request(&mut stream).await;
        match read_callback(&request, state) {
            Callback::Code(code) => {
                respond(&mut stream, "200 OK", "Mail Sage sign-in is complete.").await;
                return Ok(code);
            }
            Callback::Refused(reason) => {
                respond(&mut stream, "200 OK", "The sign-in did not complete.").await;
                bail!("Google did not complete the sign-in ({reason})");
            }
            Callback::NotOurs => respond(&mut stream, "404 Not Found", "Nothing here.").await,
        }
    }
}

async fn token_request(form: &[(&str, &str)]) -> Result<TokenResponse> {
    let response = http()?.post(TOKEN_URL).form(form).send().await.context("could not reach Google")?;
    if response.status().is_success() {
        return response.json().await.context("Google sent a token answer that Mail Sage cannot read");
    }
    let status = response.status();
    match response.json::<TokenError>().await {
        Ok(error) => bail!("Google refused the request: {} {}", error.error, error.error_description.unwrap_or_default()),
        Err(_) => bail!("Google refused the request ({status})"),
    }
}

/** The email address in Google's identity token. The token came straight from Google over TLS, so its signature needs no check. */
fn email_from_id_token(id_token: &str) -> Result<String> {
    let payload = id_token.split('.').nth(1).context("Google sent no identity")?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).context("Google sent an identity that Mail Sage cannot read")?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).context("Google sent an identity that Mail Sage cannot read")?;
    claims
        .get("email")
        .and_then(|email| email.as_str())
        .map(str::to_string)
        .context("Google did not give the account's email address")
}

/**
 * Runs the full sign-in: opens the browser, waits for the result, and gets
 * the tokens. `cancel` stops the wait.
 */
pub async fn sign_in(app: &AppHandle, cancel: &Notify) -> Result<SignIn> {
    let (client_id, client_secret) = require_client()?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.context("could not open the sign-in listener")?;
    let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let verifier = random_token();
    let state = random_token();

    let mut url = url::Url::parse(AUTH_URL)?;
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", &code_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|error| anyhow!("could not open the browser: {error}"))?;

    let code = tokio::select! {
        code = wait_for_code(&listener, &state) => code?,
        _ = cancel.notified() => bail!("The sign-in was cancelled."),
        _ = tokio::time::sleep(SIGN_IN_TIME_LIMIT) => bail!("The sign-in took too long. Try again."),
    };
    drop(listener);

    let tokens = token_request(&[
        ("client_id", &client_id),
        ("client_secret", &client_secret),
        ("code", &code),
        ("code_verifier", &verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", &redirect_uri),
    ])
    .await?;
    if !has_mail_scope(tokens.scope.as_deref()) {
        /* A sign-in without mail access is of no use, so the permission is cancelled again. */
        if let Some(refresh_token) = &tokens.refresh_token {
            let _ = revoke(refresh_token).await;
        }
        bail!("Google did not give Mail Sage access to your mail. Add the account again, and on Google's page select the box for mail access (\"Read, compose, send, and permanently delete all your email from Gmail\").");
    }
    let email = email_from_id_token(tokens.id_token.as_deref().context("Google sent no identity")?)?;
    let refresh_token = tokens.refresh_token.context("Google gave no long-term permission. Try again.")?;
    Ok(SignIn { email, refresh_token, access_token: tokens.access_token, expires_in: tokens.expires_in })
}

/** A new access token and the seconds until it stops working. */
pub async fn refresh(refresh_token: &str) -> Result<(String, u64)> {
    let (client_id, client_secret) = require_client()?;
    let tokens = token_request(&[
        ("client_id", &client_id),
        ("client_secret", &client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ])
    .await?;
    if !has_mail_scope(tokens.scope.as_deref()) {
        bail!("This sign-in has no access to your mail. Remove the account and add it again, and on Google's page select the box for mail access (\"Read, compose, send, and permanently delete all your email from Gmail\").");
    }
    Ok((tokens.access_token, tokens.expires_in))
}

/** Tells Google to cancel the permission that this token gives. */
pub async fn revoke(refresh_token: &str) -> Result<()> {
    let response = http()?.post(REVOKE_URL).form(&[("token", refresh_token)]).send().await?;
    if !response.status().is_success() {
        bail!("Google returned {}", response.status());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_the_challenge_of_the_pkce_example() {
        /* The example pair from RFC 7636, appendix B. */
        assert_eq!(code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn makes_tokens_that_differ_and_fit_the_pkce_rules() {
        let first = random_token();
        assert_ne!(first, random_token());
        assert_eq!(first.len(), 43);
        assert!(first.chars().all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_'));
    }

    #[test]
    fn reads_the_sign_in_result() {
        assert_eq!(read_callback("GET /?state=abc&code=4%2Fxyz HTTP/1.1\r\nHost: x\r\n\r\n", "abc"), Callback::Code("4/xyz".into()));
        assert_eq!(read_callback("GET /?error=access_denied&state=abc HTTP/1.1\r\n\r\n", "abc"), Callback::Refused("access_denied".into()));
    }

    #[test]
    fn ignores_requests_that_are_not_the_result() {
        assert_eq!(read_callback("GET /favicon.ico HTTP/1.1\r\n\r\n", "abc"), Callback::NotOurs);
        assert_eq!(read_callback("GET /?state=wrong&code=xyz HTTP/1.1\r\n\r\n", "abc"), Callback::NotOurs);
        assert_eq!(read_callback("POST /?state=abc&code=xyz HTTP/1.1\r\n\r\n", "abc"), Callback::NotOurs);
        assert_eq!(read_callback("", "abc"), Callback::NotOurs);
    }

    #[test]
    fn finds_the_mail_permission() {
        assert!(has_mail_scope(Some("openid https://mail.google.com/ https://www.googleapis.com/auth/userinfo.email")));
        assert!(!has_mail_scope(Some("openid https://www.googleapis.com/auth/userinfo.email")));
        assert!(!has_mail_scope(None));
    }

    #[test]
    fn reads_the_email_from_an_identity_token() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"me@example.com","sub":"1"}"#);
        assert_eq!(email_from_id_token(&format!("header.{payload}.signature")).unwrap(), "me@example.com");
        assert!(email_from_id_token("only-one-part").is_err());
    }
}
