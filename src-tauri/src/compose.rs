/**
 * Writing mail: builds a message, sends it through the account's SMTP
 * server (always encrypted), and makes the starting text of a reply or a forward.
 *
 * A message from Mail Sage is plain text. It names no mail program and no
 * computer: there is no "User-Agent" header, and the message id and the
 * SMTP greeting do not hold the computer's name.
 */
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use mail_parser::MessageParser;
use serde::{Deserialize, Serialize};

use crate::preview::address_list;

const SEND_TIME_LIMIT: Duration = Duration::from_secs(60);
/** Most mail servers refuse a message larger than about 25 MB. */
pub const MAX_ATTACHMENT_BYTES: u64 = 24 * 1024 * 1024;

/** A message to send, as the compose window gives it. */
#[derive(Debug, Deserialize)]
pub struct Outgoing {
    pub account_id: i64,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    /** The message id of the message that this one answers. */
    pub in_reply_to: Option<String>,
    /** The message ids of the conversation, oldest first, separated by spaces. */
    pub references: Option<String>,
    /** Paths of files to attach. Only files that the user picked in the "Attach" dialog are accepted. */
    pub attachments: Vec<String>,
}

/** The starting content of the compose window for a reply or a forward. */
#[derive(Debug, Serialize, PartialEq)]
pub struct Draft {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReplyMode {
    Reply,
    ReplyAll,
    Forward,
}

/** Splits a list of addresses at each comma or semicolon that is not in quotes or in angle brackets. */
fn split_addresses(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut in_brackets = false;
    for character in text.chars() {
        match character {
            '"' => in_quotes = !in_quotes,
            '<' if !in_quotes => in_brackets = true,
            '>' if !in_quotes => in_brackets = false,
            ',' | ';' | '\n' if !in_quotes && !in_brackets => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    parts.push(current);
    parts.into_iter().map(|part| part.trim().to_string()).filter(|part| !part.is_empty()).collect()
}

/** Reads a list of addresses such as `Ann <ann@example.com>, bob@example.com`. */
pub fn parse_recipients(text: &str) -> Result<Vec<Mailbox>> {
    split_addresses(text)
        .into_iter()
        .map(|part| part.parse::<Mailbox>().map_err(|_| anyhow!("\"{part}\" is not a valid email address")))
        .collect()
}

/** Builds the message. `files` has the name and the bytes of each attachment. */
pub fn build(from: &str, outgoing: &Outgoing, files: Vec<(String, Vec<u8>)>) -> Result<Message> {
    let to = parse_recipients(&outgoing.to)?;
    let cc = parse_recipients(&outgoing.cc)?;
    let bcc = parse_recipients(&outgoing.bcc)?;
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        bail!("Add one address or more in the To line.");
    }
    let mut builder = Message::builder()
        .from(from.parse::<Mailbox>().map_err(|_| anyhow!("the account address is not valid"))?)
        .subject(outgoing.subject.trim());
    for mailbox in to {
        builder = builder.to(mailbox);
    }
    for mailbox in cc {
        builder = builder.cc(mailbox);
    }
    for mailbox in bcc {
        builder = builder.bcc(mailbox);
    }
    if let Some(id) = outgoing.in_reply_to.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        builder = builder.in_reply_to(id.to_string());
    }
    if let Some(ids) = outgoing.references.as_deref().map(str::trim).filter(|ids| !ids.is_empty()) {
        builder = builder.references(ids.to_string());
    }
    /* Lines end with CR LF in mail. */
    let text = SinglePart::plain(outgoing.body.replace("\r\n", "\n").replace('\n', "\r\n"));
    let message = if files.is_empty() {
        builder.singlepart(text)
    } else {
        let binary = ContentType::parse("application/octet-stream").map_err(|_| anyhow!("internal error: content type"))?;
        let mut parts = MultiPart::mixed().singlepart(text);
        for (name, bytes) in files {
            parts = parts.singlepart(Attachment::new(name).body(bytes, binary.clone()));
        }
        builder.multipart(parts)
    };
    message.context("Mail Sage could not build the message")
}

/** How the connection to an SMTP server becomes encrypted. Mail Sage does not send on a connection that is not encrypted. */
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Encryption {
    /** Encrypted from the first byte (port 465). */
    FromStart,
    /** Starts plain and changes to encrypted before the sign-in (STARTTLS, port 587). If the server cannot do that, the send fails. */
    StartTls,
}

/** Sends a message through an SMTP server with an OAuth access token. */
pub async fn send_with_token(host: &str, encryption: Encryption, email: &str, access_token: &str, message: Message) -> Result<()> {
    let transport = match encryption {
        Encryption::FromStart => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
        Encryption::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
    };
    let mailer = transport
        .map_err(|error| anyhow!("could not prepare the connection to {host}: {error}"))?
        .credentials(Credentials::new(email.to_string(), access_token.to_string()))
        .authentication(vec![Mechanism::Xoauth2])
        .timeout(Some(SEND_TIME_LIMIT))
        .build();
    mailer.send(message).await.map_err(|error| anyhow!("{host} did not accept the message: {error}"))?;
    Ok(())
}

fn with_prefix(subject: &str, prefix: &str, known: &[&str]) -> String {
    let subject = subject.trim();
    let lower = subject.to_ascii_lowercase();
    if known.iter().any(|known| lower.starts_with(known)) {
        subject.to_string()
    } else {
        format!("{prefix} {subject}").trim_end().to_string()
    }
}

fn message_id_in_brackets(id: &str) -> String {
    format!("<{}>", id.trim().trim_matches(|character| character == '<' || character == '>'))
}

/** Makes the starting content of a reply or a forward from the raw message. `own_address` is left out of the recipients of "reply all". */
pub fn reply_draft(raw: &[u8], mode: ReplyMode, own_address: &str) -> Result<Draft> {
    let message = MessageParser::default().parse(raw).context("Mail Sage cannot read this message")?;
    let subject = message.subject().unwrap_or("");
    let sender = address_list(message.from());
    let date = message.date().map(|date| date.to_rfc822()).unwrap_or_default();
    let text = message.body_text(0).unwrap_or_default();
    let text = text.trim_end();

    if mode == ReplyMode::Forward {
        let mut body = format!("\n\n---------- Forwarded message ----------\nFrom: {sender}\nDate: {date}\nSubject: {subject}\nTo: {}\n", address_list(message.to()));
        let cc = address_list(message.cc());
        if !cc.is_empty() {
            body.push_str(&format!("Cc: {cc}\n"));
        }
        body.push_str(&format!("\n{text}\n"));
        return Ok(Draft { to: String::new(), cc: String::new(), subject: with_prefix(subject, "Fwd:", &["fwd:", "fw:"]), body, in_reply_to: None, references: None });
    }

    let is_own = |entry: &str| entry.to_ascii_lowercase().contains(&own_address.to_ascii_lowercase());
    let to = address_list(message.reply_to().or(message.from()));
    let mut cc = Vec::new();
    if mode == ReplyMode::ReplyAll {
        for list in [address_list(message.to()), address_list(message.cc())] {
            for entry in split_addresses(&list) {
                if !is_own(&entry) && !split_addresses(&to).contains(&entry) && !cc.contains(&entry) {
                    cc.push(entry);
                }
            }
        }
    }
    let quoted: String = text.lines().map(|line| if line.is_empty() { ">\n".to_string() } else { format!("> {line}\n") }).collect();
    let id = message.message_id().map(message_id_in_brackets);
    let earlier = message.header_raw("References").map(|ids| ids.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
    let references = id.as_ref().map(|id| format!("{earlier} {id}").trim().to_string());
    Ok(Draft {
        to,
        cc: cc.join(", "),
        subject: with_prefix(subject, "Re:", &["re:"]),
        body: format!("\n\nOn {date}, {sender} wrote:\n{quoted}"),
        in_reply_to: id,
        references,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outgoing(to: &str) -> Outgoing {
        Outgoing { account_id: 1, to: to.into(), cc: String::new(), bcc: String::new(), subject: " Hello ".into(), body: "Line 1\nLine 2".into(), in_reply_to: None, references: None, attachments: Vec::new() }
    }

    const ORIGINAL: &[u8] = b"From: Ann Lee <ann@example.com>\r\nTo: Me <me@example.com>, Bob <bob@example.com>\r\nCc: carol@example.com\r\n\
Subject: Lunch\r\nDate: Sat, 14 Nov 2026 10:00:00 +0000\r\nMessage-ID: <m2@example.com>\r\nReferences: <m0@example.com>\r\n  <m1@example.com>\r\n\r\nNoon?\r\n\r\nAnn\r\n";

    #[test]
    fn reads_address_lists() {
        let list = parse_recipients("Ann Lee <ann@example.com>; bob@example.com,\n\"Lee, Carol\" <carol@example.com>").unwrap();
        let addresses: Vec<String> = list.iter().map(|mailbox| mailbox.email.to_string()).collect();
        assert_eq!(addresses, ["ann@example.com", "bob@example.com", "carol@example.com"]);
        assert_eq!(list[2].name.as_deref(), Some("Lee, Carol"));
        assert!(parse_recipients("  ").unwrap().is_empty());
        assert!(parse_recipients("ann@example.com, not an address").is_err());
    }

    #[test]
    fn builds_a_plain_message_that_names_no_program_and_no_computer() {
        let text = String::from_utf8(build("me@example.com", &outgoing("ann@example.com"), Vec::new()).unwrap().formatted()).unwrap();
        assert!(text.contains("From: me@example.com\r\n"));
        assert!(text.contains("To: ann@example.com\r\n"));
        assert!(text.contains("Subject: Hello\r\n"));
        assert!(text.contains("Line 1\r\nLine 2"));
        let lower = text.to_ascii_lowercase();
        assert!(!lower.contains("user-agent") && !lower.contains("x-mailer"));
        let computer = std::env::var("COMPUTERNAME").unwrap_or_default().to_ascii_lowercase();
        assert!(computer.is_empty() || !lower.contains(&computer), "the message holds the computer name");
    }

    #[test]
    fn hides_bcc_addresses_from_the_message_text() {
        let mut message = outgoing("ann@example.com");
        message.bcc = "secret@example.com".into();
        let built = build("me@example.com", &message, Vec::new()).unwrap();
        assert!(!String::from_utf8(built.formatted()).unwrap().contains("secret@example.com"));
        assert!(built.envelope().to().iter().any(|address| address.to_string() == "secret@example.com"));
    }

    #[test]
    fn refuses_a_message_with_no_recipient() {
        assert!(build("me@example.com", &outgoing(""), Vec::new()).is_err());
    }

    #[test]
    fn attaches_files() {
        let text = String::from_utf8(build("me@example.com", &outgoing("ann@example.com"), vec![("notes.txt".into(), b"hello".to_vec())]).unwrap().formatted()).unwrap();
        assert!(text.contains("multipart/mixed"));
        assert!(text.contains("filename=\"notes.txt\""));
    }

    #[test]
    fn makes_a_reply() {
        let draft = reply_draft(ORIGINAL, ReplyMode::Reply, "me@example.com").unwrap();
        assert_eq!(draft.to, "Ann Lee <ann@example.com>");
        assert_eq!(draft.cc, "");
        assert_eq!(draft.subject, "Re: Lunch");
        assert!(draft.body.contains("Ann Lee <ann@example.com> wrote:\n> Noon?\n>\n> Ann\n"), "{}", draft.body);
        assert_eq!(draft.in_reply_to.as_deref(), Some("<m2@example.com>"));
        assert_eq!(draft.references.as_deref(), Some("<m0@example.com> <m1@example.com> <m2@example.com>"));
    }

    #[test]
    fn makes_a_reply_to_all_without_the_own_address() {
        let draft = reply_draft(ORIGINAL, ReplyMode::ReplyAll, "ME@example.com").unwrap();
        assert_eq!(draft.to, "Ann Lee <ann@example.com>");
        assert_eq!(draft.cc, "Bob <bob@example.com>, carol@example.com");
    }

    #[test]
    fn makes_a_forward() {
        let draft = reply_draft(ORIGINAL, ReplyMode::Forward, "me@example.com").unwrap();
        assert_eq!(draft.to, "");
        assert_eq!(draft.subject, "Fwd: Lunch");
        assert!(draft.body.contains("---------- Forwarded message ----------\nFrom: Ann Lee <ann@example.com>\n"));
        assert!(draft.body.contains("Subject: Lunch\n"));
        assert!(draft.body.contains("\nNoon?"));
        assert_eq!(draft.in_reply_to, None);
    }

    #[test]
    fn does_not_repeat_a_subject_prefix() {
        assert_eq!(with_prefix("RE: Lunch", "Re:", &["re:"]), "RE: Lunch");
        assert_eq!(with_prefix("", "Re:", &["re:"]), "Re:");
    }
}
