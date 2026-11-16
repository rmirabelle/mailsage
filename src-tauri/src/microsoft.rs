/**
 * Microsoft sign-in (OAuth 2 with PKCE) for Outlook.com and Hotmail
 * (personal accounts), and for Microsoft 365 (work or school accounts).
 *
 * `oauth.rs` has the browser part. Mail Sage is a "public client" for
 * Microsoft: it has a client ID and no client secret. Microsoft gives a new
 * refresh token with each new access token; the newest one goes to Windows
 * Credential Manager. Access tokens stay in memory only.
 */
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tauri::AppHandle;
use tokio::sync::Notify;

use crate::oauth::{self, SignIn};

pub const IMAP_HOST: &str = "outlook.office365.com";
pub const IMAP_PORT: u16 = 993;

/** The two kinds of Microsoft account. Each has its own sign-in address, its own app registration, and its own server for outgoing mail. */
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Audience {
    /** Outlook.com, Hotmail, Live. */
    Personal,
    /** Microsoft 365: an account of a company or a school. */
    Work,
}

impl Audience {
    /** The part of the sign-in address that selects the kind of account. */
    fn authority(self) -> &'static str {
        match self {
            Audience::Personal => "consumers",
            Audience::Work => "organizations",
        }
    }

    fn url(self, page: &str) -> String {
        format!("https://login.microsoftonline.com/{}/oauth2/v2.0/{page}", self.authority())
    }

    /** These servers accept mail on port 587 only (the connection changes to an encrypted one before the sign-in). */
    pub fn smtp_host(self) -> &'static str {
        match self {
            Audience::Personal => "smtp-mail.outlook.com",
            Audience::Work => "smtp.office365.com",
        }
    }

    fn client(self) -> Option<String> {
        match self {
            Audience::Personal => personal_client(),
            Audience::Work => work_client(),
        }
    }

    fn require_client(self) -> Result<String> {
        self.client().context("This Microsoft sign-in is not set up in this build of Mail Sage")
    }

    /** True when this build has the client ID for this kind of account (docs/MICROSOFT-SETUP.md). */
    pub fn available(self) -> bool {
        self.client().is_some()
    }
}

const IMAP_SCOPE: &str = "https://outlook.office.com/IMAP.AccessAsUser.All";
const SMTP_SCOPE: &str = "https://outlook.office.com/SMTP.Send";
/** Mail access, a long-term permission ("offline_access"), and the account's email address. */
const SCOPES: &str = "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access openid email profile";

#[cfg(microsoft_oauth_embedded)]
fn personal_client() -> Option<String> {
    Some(obfstr::obfstr!(env!("MICROSOFT_CLIENT_ID")).to_string())
}

#[cfg(not(microsoft_oauth_embedded))]
fn personal_client() -> Option<String> {
    None
}

#[cfg(microsoft_work_oauth_embedded)]
fn work_client() -> Option<String> {
    Some(obfstr::obfstr!(env!("MICROSOFT_WORK_CLIENT_ID")).to_string())
}

#[cfg(not(microsoft_work_oauth_embedded))]
fn work_client() -> Option<String> {
    None
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
    id_token: Option<String>,
    /** The permissions that the token has, separated by spaces. */
    scope: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

/** True when the permissions that Microsoft lists include reading mail (IMAP) and sending mail (SMTP). */
fn has_mail_scopes(scope: Option<&str>) -> bool {
    let has = |wanted: &str| scope.is_some_and(|scope| scope.split(' ').any(|part| part.eq_ignore_ascii_case(wanted)));
    has(IMAP_SCOPE) && has(SMTP_SCOPE)
}

/** Microsoft's error text has more than one line: the first line says what is wrong, the others are trace numbers. */
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

async fn token_request(audience: Audience, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let response = oauth::http()?.post(audience.url("token")).form(form).send().await.context("could not reach Microsoft")?;
    if response.status().is_success() {
        return response.json().await.context("Microsoft sent a token answer that Mail Sage cannot read");
    }
    let status = response.status();
    match response.json::<TokenError>().await {
        Ok(error) => bail!("Microsoft refused the request: {} {}", error.error, first_line(&error.error_description.unwrap_or_default())),
        Err(_) => bail!("Microsoft refused the request ({status})"),
    }
}

/** The email address in Microsoft's identity token: the "email" value, or the sign-in name when that is an address. */
fn email_from_id_token(id_token: &str) -> Result<String> {
    let claims = oauth::id_token_claims(id_token).context("Microsoft sent an identity that Mail Sage cannot read")?;
    ["email", "preferred_username"]
        .iter()
        .filter_map(|name| claims.get(*name).and_then(|value| value.as_str()))
        .map(str::trim)
        .find(|value| value.contains('@') && !value.contains(char::is_whitespace))
        .map(str::to_string)
        .context("Microsoft did not give the account's email address")
}

/**
 * Runs the full sign-in: opens the browser, waits for the result, and gets
 * the tokens. `cancel` stops the wait.
 */
pub async fn sign_in(app: &AppHandle, cancel: &Notify, audience: Audience) -> Result<SignIn> {
    let client_id = audience.require_client()?;
    let (listener, port) = oauth::listen().await?;
    /* Microsoft accepts each port for the registered address `http://localhost`. */
    let redirect_uri = format!("http://localhost:{port}");
    let verifier = oauth::random_token();
    let state = oauth::random_token();

    let mut url = url::Url::parse(&audience.url("authorize"))?;
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("response_mode", "query")
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", &oauth::code_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("prompt", "select_account");
    let code = oauth::browser_sign_in(app, listener, &url, &state, "Microsoft", cancel).await?;

    let tokens = token_request(audience, &[
        ("client_id", &client_id),
        ("code", &code),
        ("code_verifier", &verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", &redirect_uri),
        ("scope", SCOPES),
    ])
    .await?;
    if !has_mail_scopes(tokens.scope.as_deref()) {
        bail!("Microsoft did not give Mail Sage access to your mail. Add the account again, and accept the permissions on Microsoft's page.");
    }
    let email = email_from_id_token(tokens.id_token.as_deref().context("Microsoft sent no identity")?)?;
    let refresh_token = tokens.refresh_token.context("Microsoft gave no long-term permission. Try again.")?;
    Ok(SignIn { email, refresh_token, access_token: tokens.access_token, expires_in: tokens.expires_in })
}

/** A new access token. */
pub struct Refreshed {
    pub access_token: String,
    /** Seconds until the access token stops working. */
    pub expires_in: u64,
    /** The refresh token to use from now on, when Microsoft gave a new one. */
    pub refresh_token: Option<String>,
}

pub async fn refresh(refresh_token: &str, audience: Audience) -> Result<Refreshed> {
    let client_id = audience.require_client()?;
    let tokens = token_request(audience, &[
        ("client_id", &client_id),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
        ("scope", SCOPES),
    ])
    .await?;
    Ok(Refreshed { access_token: tokens.access_token, expires_in: tokens.expires_in, refresh_token: tokens.refresh_token })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    fn id_token(claims: &str) -> String {
        format!("header.{}.signature", URL_SAFE_NO_PAD.encode(claims.as_bytes()))
    }

    #[test]
    fn uses_the_sign_in_address_and_the_mail_server_of_each_kind_of_account() {
        assert_eq!(Audience::Personal.url("token"), "https://login.microsoftonline.com/consumers/oauth2/v2.0/token");
        assert_eq!(Audience::Work.url("authorize"), "https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize");
        assert_eq!(Audience::Personal.smtp_host(), "smtp-mail.outlook.com");
        assert_eq!(Audience::Work.smtp_host(), "smtp.office365.com");
    }

    #[test]
    fn finds_the_mail_permissions() {
        assert!(has_mail_scopes(Some("https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send openid")));
        assert!(has_mail_scopes(Some("https://outlook.office.com/imap.accessasuser.all https://outlook.office.com/smtp.send")));
        assert!(!has_mail_scopes(Some("https://outlook.office.com/IMAP.AccessAsUser.All openid")));
        assert!(!has_mail_scopes(Some("openid email")));
        assert!(!has_mail_scopes(None));
    }

    #[test]
    fn reads_the_email_from_an_identity_token() {
        assert_eq!(email_from_id_token(&id_token(r#"{"email":"me@outlook.com","preferred_username":"other@hotmail.com"}"#)).unwrap(), "me@outlook.com");
        assert_eq!(email_from_id_token(&id_token(r#"{"preferred_username":"me@hotmail.com"}"#)).unwrap(), "me@hotmail.com");
        assert!(email_from_id_token(&id_token(r#"{"preferred_username":"a phone number"}"#)).is_err());
        assert!(email_from_id_token("only-one-part").is_err());
    }

    #[test]
    fn keeps_only_the_first_line_of_an_error_text() {
        assert_eq!(first_line("AADSTS70000: The grant is not valid.\r\nTrace ID: 1\r\nCorrelation ID: 2"), "AADSTS70000: The grant is not valid.");
        assert_eq!(first_line(""), "");
    }
}
