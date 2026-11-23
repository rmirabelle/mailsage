/**
 * The IMAP connection: reads folders and message headers from a mail server.
 * Every connection is encrypted (TLS from the first byte) and the server's
 * certificate must be valid. Nothing here changes mail on the server.
 */
use std::collections::HashSet;
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

use crate::store::{FolderInfo, Marks, MessageHeader};

pub type Session = async_imap::Session<TlsStream<TcpStream>>;

const CONNECT_TIME_LIMIT: Duration = Duration::from_secs(20);
const MAX_REFUSAL_LENGTH: usize = 300;
/** How many messages one change command to the server names. */
const CHANGE_BATCH: usize = 500;
/** A search uses this many words at most, and this many characters of each word. */
const MAX_SEARCH_WORDS: usize = 8;
const MAX_SEARCH_WORD_LENGTH: usize = 64;
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

/** Opens a session with an OAuth access token (Gmail and Microsoft). */
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

/** The server refused the address or the password. Other errors (no network, no answer) are not this one. */
#[derive(Debug)]
pub struct PasswordRefused(String);

impl std::fmt::Display for PasswordRefused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PasswordRefused {}

/** Opens a session with a password (a Gmail app password). The connection is encrypted before the password goes out. */
pub async fn open_with_password(host: &str, port: u16, email: &str, password: &str) -> Result<Session> {
    let client = connect(host, port).await?;
    client
        .login(email, password)
        .await
        .map_err(|(error, _)| PasswordRefused(format!("{host} refused the sign-in: {error}")).into())
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
    role_from_name(path)
}

/**
 * The use of a folder from its name, for a server that gives no marks.
 * These are the fixed English folder names of Outlook.com. Only a folder at
 * the top level counts.
 */
fn role_from_name(path: &str) -> &'static str {
    match path.to_ascii_lowercase().as_str() {
        "deleted" | "deleted items" | "trash" => "trash",
        "junk" | "junk email" | "junk e-mail" | "spam" => "junk",
        "sent" | "sent items" => "sent",
        "drafts" => "drafts",
        "archive" => "archive",
        _ => "other",
    }
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

/** Deletes a folder on the server. On most servers, the messages in it go with it. */
pub async fn delete_folder(session: &mut Session, path: &str) -> Result<()> {
    session.delete(path).await.context("the server did not delete the folder")
}

/** The newest messages of a folder, as fetched. */
pub struct FolderFetch {
    pub uid_validity: Option<u32>,
    /** The lowest message number in the fetched range. */
    pub lowest_uid: u32,
    /** Each message in the range, with its read and flag marks. */
    pub present: Vec<Marks>,
    /** The full list entries of the messages in the range that the caller did not know. */
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

/** What one marks fetch asks for: much less than HEADER_ITEMS. */
const MARK_ITEMS: &str = "(UID FLAGS)";

/** The read and flag marks of one fetch answer. */
fn marks_of(fetch: &async_imap::types::Fetch) -> (bool, bool) {
    let mut seen = false;
    let mut flagged = false;
    for flag in fetch.flags() {
        match flag {
            Flag::Seen => seen = true,
            Flag::Flagged => flagged = true,
            _ => {}
        }
    }
    (seen, flagged)
}

/**
 * Reads the newest `count` messages of a folder. The folder opens read-only.
 * First it gets only the number and the marks of each message, which is
 * fast. Then it gets the full headers only of the messages that are not in
 * `known`. `known` is for the folder numbering `known_validity`; when the
 * server has a different numbering, all messages are new.
 */
pub async fn fetch_newest(session: &mut Session, path: &str, count: u32, known_validity: Option<u32>, known: &HashSet<u32>) -> Result<FolderFetch> {
    let mailbox = session.examine(path).await.context("the server did not open the folder")?;
    let mut result = FolderFetch { uid_validity: mailbox.uid_validity, lowest_uid: 1, present: Vec::new(), headers: Vec::new() };
    if mailbox.exists == 0 || count == 0 {
        return Ok(result);
    }
    let first = mailbox.exists.saturating_sub(count - 1).max(1);
    {
        let mut fetches = session
            .fetch(format!("{first}:{}", mailbox.exists), MARK_ITEMS)
            .await
            .context("the server did not send the messages")?;
        while let Some(fetch) = fetches.next().await {
            let fetch = fetch.context("the server sent a message that Mail Sage cannot read")?;
            if let Some(uid) = fetch.uid {
                let (seen, flagged) = marks_of(&fetch);
                result.present.push(Marks { uid, seen, flagged });
            }
        }
    }
    if result.present.is_empty() {
        bail!("the server sent no messages for a folder that has {}", mailbox.exists);
    }
    let same_numbering = known_validity.is_some() && known_validity == mailbox.uid_validity;
    let new: Vec<u32> = result.present.iter().map(|marks| marks.uid).filter(|uid| !same_numbering || !known.contains(uid)).collect();
    for batch in new.chunks(CHANGE_BATCH) {
        result.headers.extend(fetch_headers(session, batch).await?);
    }
    /* When the fetch started at the first message, it has the full folder: each stored message that is not in it is gone. */
    result.lowest_uid = if first == 1 { 1 } else { result.present.iter().map(|marks| marks.uid).min().unwrap_or(1) };
    Ok(result)
}

/** Makes a list entry from one fetch answer of the server. An answer with no message number gives nothing. */
fn header_of(fetch: &async_imap::types::Fetch) -> Option<MessageHeader> {
    let uid = fetch.uid?;
    let (subject, from_name, from_address) = fetch.header().map(parse_header).unwrap_or_default();
    let (seen, flagged) = marks_of(fetch);
    Some(MessageHeader {
        uid,
        subject,
        from_name,
        from_address,
        date: fetch.internal_date().map(|date| date.timestamp()).unwrap_or(0),
        seen,
        flagged,
        size: fetch.size.unwrap_or(0),
    })
}

/** Opens a folder read-only, and stops if the server has numbered the folder again since the list was read. */
async fn examine_checked(session: &mut Session, path: &str, uid_validity: Option<u32>) -> Result<()> {
    let mailbox = session.examine(path).await.context("the server did not open the folder")?;
    if mailbox.uid_validity != uid_validity {
        bail!("The folder changed on the server. Refresh the message list.");
    }
    Ok(())
}

/** Fetches the headers of the given messages of the open folder. The messages stay unread. */
async fn fetch_headers(session: &mut Session, uids: &[u32]) -> Result<Vec<MessageHeader>> {
    let mut headers = Vec::new();
    if uids.is_empty() {
        return Ok(headers);
    }
    let wanted: HashSet<u32> = uids.iter().copied().collect();
    let mut fetches = session.uid_fetch(uid_set(uids), HEADER_ITEMS).await.context("the server did not send the messages")?;
    while let Some(fetch) = fetches.next().await {
        let fetch = fetch.context("the server sent a message that Mail Sage cannot read")?;
        headers.extend(header_of(&fetch).filter(|header| wanted.contains(&header.uid)));
    }
    Ok(headers)
}

/** Fetches the headers of the newest `count` messages that are older than the message `before_uid`. The folder opens read-only. */
pub async fn fetch_older(session: &mut Session, path: &str, uid_validity: Option<u32>, before_uid: u32, count: usize) -> Result<Vec<MessageHeader>> {
    examine_checked(session, path, uid_validity).await?;
    if before_uid <= 1 {
        return Ok(Vec::new());
    }
    let uids = search_uids(session, &format!("UID 1:{}", before_uid - 1)).await?;
    fetch_headers(session, &uids[uids.len().saturating_sub(count)..]).await
}

/**
 * Makes the search command for the server from the text that the user
 * typed: a message matches when its sender or its subject contains each
 * word. The text is from the user, so each word goes into a quoted string,
 * and control characters (a line end could start a second command) are
 * removed. Gives nothing for a text with no usable word.
 */
pub fn search_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| word.chars().filter(|character| !character.is_control()).take(MAX_SEARCH_WORD_LENGTH).collect::<String>())
        .filter(|word| !word.is_empty())
        .take(MAX_SEARCH_WORDS)
        .collect();
    if words.is_empty() {
        return None;
    }
    let terms: Vec<String> = words
        .iter()
        .map(|word| {
            let quoted = format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""));
            format!("OR FROM {quoted} SUBJECT {quoted}")
        })
        .collect();
    let charset = if text.is_ascii() { "" } else { "CHARSET UTF-8 " };
    Some(format!("{charset}{}", terms.join(" ")))
}

/** Searches a full folder on the server, and fetches the headers of the newest `count` messages that match. The folder opens read-only. */
pub async fn search_headers(session: &mut Session, path: &str, uid_validity: Option<u32>, query: &str, count: usize) -> Result<Vec<MessageHeader>> {
    examine_checked(session, path, uid_validity).await?;
    let uids = search_uids(session, query).await?;
    fetch_headers(session, &uids[uids.len().saturating_sub(count)..]).await
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

/**
 * Fetches several full messages in one request, for loading ahead. The
 * folder opens read-only and the messages stay unread. A message that is no
 * longer on the server is left out.
 */
pub async fn fetch_messages(session: &mut Session, path: &str, uid_validity: Option<u32>, uids: &[u32]) -> Result<Vec<(u32, Vec<u8>)>> {
    let mailbox = session.examine(path).await.context("the server did not open the folder")?;
    if mailbox.uid_validity != uid_validity {
        bail!("The folder changed on the server. Refresh the message list.");
    }
    let mut fetches = session.uid_fetch(uid_set(uids), "(UID BODY.PEEK[])").await.context("the server did not send the messages")?;
    let mut messages = Vec::new();
    while let Some(fetch) = fetches.next().await {
        let fetch = fetch.context("the server sent a message that Mail Sage cannot read")?;
        if let (Some(uid), Some(body)) = (fetch.uid, fetch.body()) {
            if uids.contains(&uid) {
                messages.push((uid, body.to_vec()));
            }
        }
    }
    Ok(messages)
}

/** Opens a folder for changes, and stops if the server has numbered the folder again since the list was read. */
async fn select_checked(session: &mut Session, path: &str, uid_validity: Option<u32>) -> Result<()> {
    let mailbox = session.select(path).await.context("the server did not open the folder")?;
    if mailbox.uid_validity != uid_validity {
        bail!("The folder changed on the server. Refresh the message list.");
    }
    Ok(())
}

fn uid_set(uids: &[u32]) -> String {
    uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

async fn store_flags(session: &mut Session, uids: &[u32], change: &str) -> Result<()> {
    let mut updates = session.uid_store(uid_set(uids), change).await.context("the server did not change the messages")?;
    while let Some(update) = updates.next().await {
        update.context("the server did not change the messages")?;
    }
    Ok(())
}

/** Marks messages as read or as unread on the server. */
pub async fn set_seen(session: &mut Session, path: &str, uid_validity: Option<u32>, uids: &[u32], seen: bool) -> Result<()> {
    select_checked(session, path, uid_validity).await?;
    store_flags(session, uids, if seen { "+FLAGS.SILENT (\\Seen)" } else { "-FLAGS.SILENT (\\Seen)" }).await
}

/** Puts the star (the "flagged" mark) on messages on the server, or removes it. */
pub async fn set_flagged(session: &mut Session, path: &str, uid_validity: Option<u32>, uids: &[u32], flagged: bool) -> Result<()> {
    select_checked(session, path, uid_validity).await?;
    store_flags(session, uids, if flagged { "+FLAGS.SILENT (\\Flagged)" } else { "-FLAGS.SILENT (\\Flagged)" }).await
}

/** Removes exactly the given messages from the open folder. This needs a server that can remove messages by number (UIDPLUS). */
async fn remove_by_uid(session: &mut Session, uids: &[u32]) -> Result<()> {
    if !session.capabilities().await.context("the server did not list its abilities")?.has_str("UIDPLUS") {
        bail!("This mail server cannot remove single messages.");
    }
    store_flags(session, uids, "+FLAGS.SILENT (\\Deleted)").await?;
    let removed = session.uid_expunge(uid_set(uids)).await.context("the server did not remove the messages")?;
    let mut removed = std::pin::pin!(removed);
    while let Some(result) = removed.next().await {
        result.context("the server did not remove the messages")?;
    }
    Ok(())
}

/** Moves messages to a different folder on the server (for example, to the trash). */
pub async fn move_messages(session: &mut Session, path: &str, uid_validity: Option<u32>, uids: &[u32], target: &str) -> Result<()> {
    select_checked(session, path, uid_validity).await?;
    move_selected(session, uids, target).await
}

/** Moves messages out of the open folder. */
async fn move_selected(session: &mut Session, uids: &[u32], target: &str) -> Result<()> {
    if session.capabilities().await.context("the server did not list its abilities")?.has_str("MOVE") {
        return session.uid_mv(uid_set(uids), target).await.context("the server did not move the messages");
    }
    session.uid_copy(uid_set(uids), target).await.context("the server did not copy the messages")?;
    remove_by_uid(session, uids).await
}

/** Deletes messages from the server for good. They cannot come back. */
pub async fn delete_forever(session: &mut Session, path: &str, uid_validity: Option<u32>, uids: &[u32]) -> Result<()> {
    select_checked(session, path, uid_validity).await?;
    remove_by_uid(session, uids).await
}

/** The numbers of the messages in the open folder that match a search (for example "ALL" or "UNSEEN"), lowest first. */
async fn search_uids(session: &mut Session, query: &str) -> Result<Vec<u32>> {
    let mut uids: Vec<u32> = session.uid_search(query).await.context("the server did not search the folder")?.into_iter().collect();
    uids.sort_unstable();
    Ok(uids)
}

/**
 * Opens a folder for a change to all its messages. Such a change reads the
 * message numbers from the server in the same connection, so it does not
 * use stored numbers, and it needs no check of the folder's numbering mark.
 * It works also for a folder that the user did not open before.
 */
async fn select_for_all(session: &mut Session, path: &str) -> Result<()> {
    session.select(path).await.context("the server did not open the folder")?;
    Ok(())
}

/**
 * Marks all messages of a folder as read on the server. The change goes to
 * exactly the messages that are unread now, in batches, so that one command
 * to the server does not become too long.
 */
pub async fn set_all_seen(session: &mut Session, path: &str) -> Result<()> {
    select_for_all(session, path).await?;
    let uids = search_uids(session, "UNSEEN").await?;
    for batch in uids.chunks(CHANGE_BATCH) {
        store_flags(session, batch, "+FLAGS.SILENT (\\Seen)").await?;
    }
    Ok(())
}

/**
 * Removes all messages of a folder: with a `target`, they move to that
 * folder (the trash); with no target, they are deleted for good. Only the
 * messages that are in the folder at the start are removed; a message that
 * arrives during the work stays.
 */
pub async fn remove_all(session: &mut Session, path: &str, target: Option<&str>) -> Result<()> {
    select_for_all(session, path).await?;
    let uids = search_uids(session, "ALL").await?;
    for batch in uids.chunks(CHANGE_BATCH) {
        match target {
            Some(target) => move_selected(session, batch, target).await?,
            None => remove_by_uid(session, batch).await?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_a_search_command_for_sender_or_subject() {
        assert_eq!(search_query("ana"), Some("OR FROM \"ana\" SUBJECT \"ana\"".to_string()));
        assert_eq!(search_query("  ana   order "), Some("OR FROM \"ana\" SUBJECT \"ana\" OR FROM \"order\" SUBJECT \"order\"".to_string()));
        assert_eq!(search_query("   "), None);
    }

    #[test]
    fn search_text_cannot_leave_its_quoted_string() {
        /* A quote and a backslash are escaped, so they stay inside the string. */
        assert_eq!(search_query("a\"b\\c"), Some("OR FROM \"a\\\"b\\\\c\" SUBJECT \"a\\\"b\\\\c\"".to_string()));
        /* A line end would start a second command to the server: it is removed. */
        let query = search_query("x\r\nA1 DELETE INBOX\u{0}").unwrap();
        assert!(!query.chars().any(char::is_control));
        /* Text that is not ASCII names its character set. */
        assert_eq!(search_query("café"), Some("CHARSET UTF-8 OR FROM \"café\" SUBJECT \"café\"".to_string()));
    }

    #[test]
    fn finds_folder_roles() {
        assert_eq!(folder_role("inbox", &[]), "inbox");
        assert_eq!(folder_role("[Gmail]/Sent Mail", &[NameAttribute::Sent]), "sent");
        assert_eq!(folder_role("[Gmail]/Spam", &[NameAttribute::Marked, NameAttribute::Junk]), "junk");
        /* A server that gives no marks: the name of a top-level folder gives the use. */
        assert_eq!(folder_role("Deleted", &[NameAttribute::NoInferiors]), "trash");
        assert_eq!(folder_role("Junk Email", &[]), "junk");
        assert_eq!(folder_role("Sent Items", &[]), "sent");
        assert_eq!(folder_role("Work/Trash", &[]), "other");
        /* A mark wins over the name. */
        assert_eq!(folder_role("Trash", &[NameAttribute::Archive]), "archive");
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
