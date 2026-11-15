/**
 * Link checks. A link in a message never opens by a click alone: the window
 * first shows the real address, with warnings, and the user confirms.
 */
use anyhow::{bail, Context, Result};
use serde::Serialize;
use url::Url;

/** Query parameters that exist only to track the reader. They are removed before a link opens. */
const TRACKING_PARAMETERS: [&str; 20] = [
    "fbclid", "gclid", "dclid", "gbraid", "wbraid", "msclkid", "mc_cid", "mc_eid", "_hsenc", "_hsmi", "mkt_tok", "igshid", "yclid",
    "oly_anon_id", "oly_enc_id", "vero_id", "s_cid", "ml_subscriber", "ml_subscriber_hash", "_openstat",
];
const TRACKING_PREFIXES: [&str; 2] = ["utm_", "pk_"];

#[derive(Debug, Serialize, PartialEq)]
pub struct LinkCheck {
    /** The address that opens: the link's address without tracking parameters and without a sign-in part. */
    pub url: String,
    /** The site that the link really goes to. */
    pub host: String,
    pub tracking_removed: bool,
    /** True when the link's text shows one site and the link goes to a different site. */
    pub text_mismatch: bool,
    /** True when the site name uses characters that can look like other letters. */
    pub lookalike_risk: bool,
    /** True when the address had a sign-in part before the site name (`name@site`), a known trick to hide the real site. */
    pub had_user_part: bool,
    /** True for a link that is not encrypted (http). */
    pub not_encrypted: bool,
}

fn is_tracking_parameter(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    TRACKING_PARAMETERS.contains(&name.as_str()) || TRACKING_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn without_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

/** The site name that a link's text shows, when the text looks like a web address. */
fn host_in_text(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let with_scheme = if lower.starts_with("http://") || lower.starts_with("https://") {
        text.to_string()
    } else if lower.starts_with("www.") || (text.contains('.') && !text.contains('@')) {
        format!("https://{text}")
    } else {
        return None;
    };
    Url::parse(&with_scheme).ok()?.host_str().map(|host| host.to_ascii_lowercase())
}

/** Checks a web link and makes the address that will open. `text` is what the message shows for the link. */
pub fn check(href: &str, text: &str) -> Result<LinkCheck> {
    let mut url = Url::parse(href.trim()).context("this link is not a valid address")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("Mail Sage opens only web links");
    }
    let host = url.host_str().context("this link has no site name")?.to_ascii_lowercase();

    let had_user_part = !url.username().is_empty() || url.password().is_some();
    if had_user_part {
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }

    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let kept: Vec<&(String, String)> = pairs.iter().filter(|(name, _)| !is_tracking_parameter(name)).collect();
    let tracking_removed = kept.len() != pairs.len();
    if tracking_removed {
        if kept.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(kept.iter().map(|(name, value)| (name.as_str(), value.as_str())));
        }
    }

    let text_mismatch = host_in_text(text).is_some_and(|shown| without_www(&shown) != without_www(&host));
    Ok(LinkCheck {
        url: url.to_string(),
        lookalike_risk: host.split('.').any(|label| label.starts_with("xn--")),
        host,
        tracking_removed,
        text_mismatch,
        had_user_part,
        not_encrypted: url.scheme() == "http",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_tracking_parameters_and_keeps_the_others() {
        let link = check("https://shop.example/item?id=7&utm_source=mail&UTM_Campaign=x&fbclid=abc&color=red#top", "Buy").unwrap();
        assert_eq!(link.url, "https://shop.example/item?id=7&color=red#top");
        assert!(link.tracking_removed);
        let link = check("https://shop.example/item?utm_source=mail", "Buy").unwrap();
        assert_eq!(link.url, "https://shop.example/item");
        assert!(!check("https://shop.example/item?id=7", "Buy").unwrap().tracking_removed);
    }

    #[test]
    fn finds_text_that_shows_a_different_site() {
        assert!(check("https://evil.example/login", "https://www.mybank.com/login").unwrap().text_mismatch);
        assert!(check("https://evil.example/login", "www.mybank.com").unwrap().text_mismatch);
        assert!(check("https://evil.example/login", "mybank.com").unwrap().text_mismatch);
        assert!(!check("https://www.mybank.com/login", "mybank.com").unwrap().text_mismatch);
        assert!(!check("https://evil.example/login", "Click here").unwrap().text_mismatch);
        assert!(!check("https://evil.example/", "me@example.com").unwrap().text_mismatch);
    }

    #[test]
    fn removes_a_sign_in_part_that_hides_the_real_site() {
        let link = check("https://www.mybank.com@evil.example/login", "x").unwrap();
        assert_eq!(link.host, "evil.example");
        assert_eq!(link.url, "https://evil.example/login");
        assert!(link.had_user_part);
    }

    #[test]
    fn warns_about_lookalike_names_and_plain_http() {
        assert!(check("https://аpple.com/", "x").unwrap().lookalike_risk);
        assert!(!check("https://apple.com/", "x").unwrap().lookalike_risk);
        assert!(check("http://example.com/", "x").unwrap().not_encrypted);
    }

    #[test]
    fn refuses_links_that_are_not_web_links() {
        for href in ["javascript:alert(1)", "file:///C:/Windows/System32/calc.exe", "ms-msdt:/id", "mailto:a@example.com", "data:text/html,x", "not a link"] {
            assert!(check(href, "x").is_err(), "{href}");
        }
    }
}
