/**
 * The parts of a browser sign-in (OAuth 2 with PKCE) that Google and
 * Microsoft have in common.
 *
 * The sign-in page opens in the user's default browser, so Mail Sage never
 * sees the password. The provider sends the result to a listener on the
 * loopback address that exists only for the time of the sign-in.
 */
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

const USER_AGENT: &str = "MailSage";

/** How long the user has to finish the sign-in in the browser. */
const SIGN_IN_TIME_LIMIT: Duration = Duration::from_secs(300);
const REQUEST_TIME_LIMIT: Duration = Duration::from_secs(5);
const HTTP_TIME_LIMIT: Duration = Duration::from_secs(30);
const MAX_REQUEST_BYTES: usize = 8192;

/** The result of a complete sign-in. */
pub struct SignIn {
    pub email: String,
    pub refresh_token: String,
    pub access_token: String,
    /** Seconds until the access token stops working. */
    pub expires_in: u64,
}

pub fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::fill(&mut bytes[..]);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent(USER_AGENT).timeout(HTTP_TIME_LIMIT).build()?)
}

/** What the browser's request to the listener says. */
#[derive(Debug, PartialEq)]
enum Callback {
    /** The provider gave a sign-in code. */
    Code(String),
    /** The provider reports that the sign-in did not complete (for example, the user said no). */
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
async fn wait_for_code(listener: &TcpListener, state: &str, provider: &str) -> Result<String> {
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
                bail!("{provider} did not complete the sign-in ({reason})");
            }
            Callback::NotOurs => respond(&mut stream, "404 Not Found", "Nothing here.").await,
        }
    }
}

/** A listener on the loopback address, on a free port, for the result of one sign-in. */
pub async fn listen() -> Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.context("could not open the sign-in listener")?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/**
 * Opens the sign-in page in the browser and waits for the sign-in code.
 * `cancel` stops the wait. `provider` is the name for error messages.
 */
pub async fn browser_sign_in(app: &AppHandle, listener: TcpListener, url: &url::Url, state: &str, provider: &str, cancel: &Notify) -> Result<String> {
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|error| anyhow!("could not open the browser: {error}"))?;
    tokio::select! {
        code = wait_for_code(&listener, state, provider) => code,
        _ = cancel.notified() => bail!("The sign-in was cancelled."),
        _ = tokio::time::sleep(SIGN_IN_TIME_LIMIT) => bail!("The sign-in took too long. Try again."),
    }
}

/** The claims in an identity token. The token came straight from the provider over TLS, so its signature needs no check. */
pub fn id_token_claims(id_token: &str) -> Option<serde_json::Value> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
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
    fn reads_the_claims_of_an_identity_token() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"me@example.com","sub":"1"}"#);
        let claims = id_token_claims(&format!("header.{payload}.signature")).unwrap();
        assert_eq!(claims.get("email").and_then(|email| email.as_str()), Some("me@example.com"));
        assert!(id_token_claims("only-one-part").is_none());
    }
}
