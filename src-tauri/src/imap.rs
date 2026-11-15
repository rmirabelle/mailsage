/**
 * The IMAP connection: reads folders and message headers from a mail server.
 * Every connection is encrypted (TLS from the first byte) and the server's
 * certificate must be valid. Nothing here changes mail on the server.
 */
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use async_imap::types::{Flag, NameAttribute};
use futures_util::StreamExt;
use mail_parser::MessageParser;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

use crate::store::{FolderInfo, MessageHeader};

pub type Session = async_imap::Session<TlsStream<TcpStream>>;

const CONNECT_TIME_LIMIT: Duration = Duration::from_secs(20);
const MAX_REFUSAL_LENGTH: usize = 300;
/** What one message fetch asks for. BODY.PEEK leaves the message unread on the server. */
const HEADER_ITEMS: &str = "(UID FLAGS RFC822.SIZE INTERNALDATE BODY.PEEK[HEADER])";

/** The answer to the server's challenge in an XOAUTH2 sign-in. */
struct XOAuth2 {
    response: Option<String>,
    /** The server's error report, when it refuses the token. It names a status and the necessary permission; it has no secret. */
    refusal: Arc<Mutex<Option<String>>>,
}

impl async_imap::Authenticator for XOAuth2 {
    type Response = String;

    /** The first answer carries the token. A second challenge is the server's error report; the right answer to it is empty. */
    fn process(&mut self, challenge: &[u8]) -> String {
        if self.response.is_none() && !challenge.is_empty() {
            let report: String = String::from_utf8_lossy(challenge).chars().take(MAX_REFUSAL_LENGTH).collect();
            *self.refusal.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(report);
        }
        self.response.take().unwrap_or_default()
    }
}

fn tls_connector() -> Result<TlsConnector> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

async fn connect(host: &str, port: u16) -> Result<async_imap::Client<TlsStream<TcpStream>>> {
    let server_name = ServerName::try_from(host.to_string()).map_err(|_| anyhow!("{host} is not a valid server name"))?;
    let connector = tls_connector()?;
    let open = async {
        let tcp = TcpStream::connect((host, port)).await.with_context(|| format!("could not reach {host}"))?;
        let tls = connector.connect(server_name, tcp).await.with_context(|| format!("{host} did not give a secure connection"))?;
        let mut client = async_imap::Client::new(tls);
        client.read_response().await?.with_context(|| format!("{host} closed the connection"))?;
        Ok::<_, anyhow::Error>(client)
    };
    tokio::time::timeout(CONNECT_TIME_LIMIT, open)
        .await
        .map_err(|_| anyhow!("{host} did not answer in time"))?
}

/** Opens a session with an OAuth access token (Gmail, and later Microsoft). */
pub async fn open_with_token(host: &str, port: u16, email: &str, access_token: &str) -> Result<Session> {
    let client = connect(host, port).await?;
    let refusal = Arc::new(Mutex::new(None));
    let auth = XOAuth2 { response: Some(format!("user={email}\x01auth=Bearer {access_token}\x01\x01")), refusal: refusal.clone() };
    client.authenticate("XOAUTH2", auth).await.map_err(|(error, _)| {
        let report = refusal.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        match report {
            Some(report) => anyhow!("{host} refused the sign-in: {error}. The server's report: {report}"),
            None => anyhow!("{host} refused the sign-in: {error}"),
        }
    })
}

/** The well-known use of a folder, from the server's marks and, for the inbox, its name. */
fn folder_role(path: &str, attributes: &[NameAttribute<'_>]) -> &'static str {
    if path.eq_ignore_ascii_case("INBOX") {
        return "inbox";
    }
    for attribute in attributes {
        let role = match attribute {
            NameAttribute::All => "all",
            NameAttribute::Archive => "archive",
            NameAttribute::Drafts => "drafts",
            NameAttribute::Flagged => "flagged",
            NameAttribute::Junk => "junk",
            NameAttribute::Sent => "sent",
            NameAttribute::Trash => "trash",
            _ => continue,
        };
        return role;
    }
    "other"
}

/** The readable last part of a folder path, and how deep the folder is. */
fn folder_name_and_depth(path: &str, delimiter: Option<&str>) -> (String, u32) {
    let decoded = utf7_imap::decode_utf7_imap(path.to_string());
    match delimiter.filter(|delimiter| !delimiter.is_empty()) {
        Some(delimiter) => {
            let parts: Vec<&str> = decoded.split(delimiter).collect();
            (parts.last().copied().unwrap_or("").to_string(), parts.len() as u32 - 1)
        }
        None => (decoded, 0),
    }
}

/** All folders of the account, with their message counts. */
pub async fn list_folders(session: &mut Session) -> Result<Vec<FolderInfo>> {
    let mut folders = Vec::new();
    {
        let mut names = session.list(Some(""), Some("*")).await.context("the server did not list the folders")?;
        while let Some(name) = names.next().await {
            let name = name.context("the server sent a folder that Mail Sage cannot read")?;
            let path = name.name().to_string();
            let (readable, depth) = folder_name_and_depth(&path, name.delimiter());
            let selectable = !name.attributes().iter().any(|attribute| matches!(attribute, NameAttribute::NoSelect));
            let role = folder_role(&path, name.attributes()).to_string();
            let readable = if role == "inbox" { "Inbox".to_string() } else { readable };
            folders.push(FolderInfo { path, name: readable, depth, role, selectable, unread: 0, total: 0 });
        }
    }
    for folder in folders.iter_mut().filter(|folder| folder.selectable) {
        /* A folder whose counts the server will not give keeps zero; the list still shows it. */
        if let Ok(status) = session.status(&folder.path, "(MESSAGES UNSEEN)").await {
            folder.total = status.exists;
            folder.unread = status.unseen.unwrap_or(0);
        }
    }
    Ok(folders)
}

/** The newest messages of a folder, as fetched. */
pub struct FolderFetch {
    pub uid_validity: Option<u32>,
    /** The lowest message number in the fetched range. */
    pub lowest_uid: u32,
    pub headers: Vec<MessageHeader>,
}

/** Reads the subject and the sender from raw header text. Encoded words become readable text. */
fn parse_header(raw: &[u8]) -> (String, String, String) {
    let Some(message) = MessageParser::default().parse_headers(raw) else {
        return Default::default();
    };
    let subject = message.subject().unwrap_or("").trim().to_string();
    let sender = message.from().and_then(|from| from.first());
    let name = sender.and_then(|sender| sender.name()).unwrap_or("").trim().to_string();
    let address = sender.and_then(|sender| sender.address()).unwrap_or("").trim().to_string();
    (subject, name, address)
}

/** Fetches the headers of the newest `count` messages of a folder. The folder opens read-only. */
pub async fn fetch_newest(session: &mut Session, path: &str, count: u32) -> Result<FolderFetch> {
    let mailbox = session.examine(path).await.context("the server did not open the folder")?;
    let mut result = FolderFetch { uid_validity: mailbox.uid_validity, lowest_uid: 1, headers: Vec::new() };
    if mailbox.exists == 0 || count == 0 {
        return Ok(result);
    }
    let first = mailbox.exists.saturating_sub(count - 1).max(1);
    {
        let mut fetches = session
            .fetch(format!("{first}:{}", mailbox.exists), HEADER_ITEMS)
            .await
            .context("the server did not send the messages")?;
        while let Some(fetch) = fetches.next().await {
            let fetch = fetch.context("the server sent a message that Mail Sage cannot read")?;
            let Some(uid) = fetch.uid else { continue };
            let (subject, from_name, from_address) = fetch.header().map(parse_header).unwrap_or_default();
            let mut seen = false;
            let mut flagged = false;
            for flag in fetch.flags() {
                match flag {
                    Flag::Seen => seen = true,
                    Flag::Flagged => flagged = true,
                    _ => {}
                }
            }
            result.headers.push(MessageHeader {
                uid,
                subject,
                from_name,
                from_address,
                date: fetch.internal_date().map(|date| date.timestamp()).unwrap_or(0),
                seen,
                flagged,
                size: fetch.size.unwrap_or(0),
            });
        }
    }
    if result.headers.is_empty() {
        bail!("the server sent no messages for a folder that has {}", mailbox.exists);
    }
    result.lowest_uid = result.headers.iter().map(|header| header.uid).min().unwrap_or(1);
    Ok(result)
}

/**
 * Fetches one full message as raw bytes. The folder opens read-only and the
 * message stays unread on the server. `uid_validity` is the folder's
 * numbering mark from the time the list was read: if the server has numbered
 * the folder again, the number could be a different message, so the fetch
 * stops.
 */
pub async fn fetch_message(session: &mut Session, path: &str, uid_validity: Option<u32>, uid: u32) -> Result<Vec<u8>> {
    let mailbox = session.examine(path).await.context("the server did not open the folder")?;
    if mailbox.uid_validity != uid_validity {
        bail!("The folder changed on the server. Refresh the message list.");
    }
    let mut fetches = session.uid_fetch(uid.to_string(), "(UID BODY.PEEK[])").await.context("the server did not send the message")?;
    let mut raw = None;
    while let Some(fetch) = fetches.next().await {
        let fetch = fetch.context("the server sent a message that Mail Sage cannot read")?;
        if fetch.uid == Some(uid) {
            raw = fetch.body().map(<[u8]>::to_vec);
        }
    }
    raw.context("The message is no longer on the server. Refresh the message list.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_folder_roles() {
        assert_eq!(folder_role("inbox", &[]), "inbox");
        assert_eq!(folder_role("[Gmail]/Sent Mail", &[NameAttribute::Sent]), "sent");
        assert_eq!(folder_role("[Gmail]/Spam", &[NameAttribute::Marked, NameAttribute::Junk]), "junk");
        assert_eq!(folder_role("Work", &[NameAttribute::Marked]), "other");
    }

    #[test]
    fn reads_folder_names() {
        assert_eq!(folder_name_and_depth("[Gmail]/Sent Mail", Some("/")), ("Sent Mail".to_string(), 1));
        assert_eq!(folder_name_and_depth("Work", Some("/")), ("Work".to_string(), 0));
        assert_eq!(folder_name_and_depth("Entw&APw-rfe", None), ("Entwürfe".to_string(), 0));
    }

    #[test]
    fn reads_plain_and_encoded_headers() {
        let raw = b"From: =?UTF-8?B?SsO8cmdlbg==?= <j@example.com>\r\nSubject: =?UTF-8?Q?Gr=C3=BC=C3=9Fe?= from Bonn\r\n\r\n";
        assert_eq!(parse_header(raw), ("Grüße from Bonn".to_string(), "Jürgen".to_string(), "j@example.com".to_string()));
        assert_eq!(parse_header(b"Subject: Hi\r\n\r\n"), ("Hi".to_string(), String::new(), String::new()));
    }
}
