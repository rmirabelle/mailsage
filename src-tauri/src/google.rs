/**
 * Google sign-in (OAuth 2 with PKCE) for Gmail.
 *
 * `oauth.rs` has the browser part. Mail Sage gets a refresh token (kept in
 * Windows Credential Manager) and short-lived access tokens (kept in memory
 * only).
 */
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tauri::AppHandle;
use tokio::sync::Notify;

use crate::oauth::{self, SignIn};

pub const IMAP_HOST: &str = "imap.gmail.com";
pub const IMAP_PORT: u16 = 993;
pub const SMTP_HOST: &str = "smtp.gmail.com";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
/** Full mail access (IMAP and SMTP need it), plus the account's email address. */
const MAIL_SCOPE: &str = "https://mail.google.com/";
const SCOPES: &str = "https://mail.google.com/ openid email";

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

async fn token_request(form: &[(&str, &str)]) -> Result<TokenResponse> {
    let response = oauth::http()?.post(TOKEN_URL).form(form).send().await.context("could not reach Google")?;
    if response.status().is_success() {
        return response.json().await.context("Google sent a token answer that Mail Sage cannot read");
    }
    let status = response.status();
    let refreshing = form.contains(&("grant_type", "refresh_token"));
    match response.json::<TokenError>().await {
        Ok(error) if refreshing && error.error == "invalid_grant" => bail!(
            "Google ended Mail Sage's sign-in for this account. Google does this after 7 days while the Google Cloud registration is in Testing status. To stop it, use an app password: File > Add account > Gmail with app password. The account keeps its folders."
        ),
        Ok(error) => bail!("Google refused the request: {} {}", error.error, error.error_description.unwrap_or_default()),
        Err(_) => bail!("Google refused the request ({status})"),
    }
}

/** The email address in Google's identity token. */
fn email_from_id_token(id_token: &str) -> Result<String> {
    oauth::id_token_claims(id_token)
        .context("Google sent an identity that Mail Sage cannot read")?
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
    let (listener, port) = oauth::listen().await?;
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let verifier = oauth::random_token();
    let state = oauth::random_token();

    let mut url = url::Url::parse(AUTH_URL)?;
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", &oauth::code_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    let code = oauth::browser_sign_in(app, listener, &url, &state, "Google", cancel).await?;

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
    let response = oauth::http()?.post(REVOKE_URL).form(&[("token", refresh_token)]).send().await?;
    if !response.status().is_success() {
        bail!("Google returned {}", response.status());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

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
