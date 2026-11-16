/**
 * The secure preview: turns a raw message into HTML that is safe to show.
 *
 * The rule: an email cannot run code, cannot contact a server, and cannot
 * open a file by itself. There are three separate guards, and each one is
 * enough alone:
 *
 * 1. This cleaner removes scripts, forms, frames, event attributes, and every
 *    address that is not a plain link. Images stay only as data inside the
 *    page (never as an address that the window would fetch).
 * 2. The page that holds the result has a content security policy that
 *    forbids all network access and all scripts.
 * 3. The window shows the page in a frame that cannot run scripts.
 */
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use mail_parser::{Address, MessageParser, MimeHeaders, PartType};
use serde::Serialize;

/** Inline images larger than this in total are not shown. */
const MAX_INLINE_IMAGE_BYTES: usize = 25 * 1024 * 1024;
const MAX_FILE_NAME_LENGTH: usize = 150;

/** The only image types that the preview shows. SVG is not here: an SVG file can hold scripts and addresses. */
const IMAGE_TYPES: [(&str, &str); 4] = [("png", "image/png"), ("jpeg", "image/jpeg"), ("gif", "image/gif"), ("webp", "image/webp")];

/** File types that can run code or hold macros. The window warns before it saves one. */
const RISKY_EXTENSIONS: [&str; 44] = [
    "exe", "com", "scr", "pif", "bat", "cmd", "js", "jse", "vbs", "vbe", "wsf", "wsh", "ps1", "msi", "msp", "hta", "lnk", "url",
    "jar", "reg", "cpl", "dll", "iso", "img", "vhd", "vhdx", "docm", "xlsm", "pptm", "dotm", "xlam", "xll", "html", "htm", "svg",
    "chm", "appx", "msix", "one", "scf", "inf", "application", "gadget", "msc",
];

/** CSS words that can make the window fetch something or run something. Each is made harmless in style text. */
const CSS_BLOCKED: [(&str, &str); 6] = [
    ("url(", "blocked("),
    ("@import", "@blocked"),
    ("image-set(", "blocked("),
    ("expression(", "blocked("),
    ("behavior", "blocked"),
    ("-moz-binding", "blocked"),
];

/**
 * The cleaner first puts this mark, with a number, in place of each remote
 * image address. A message cannot make the mark itself: the cleaner removes
 * every address with a scheme that it does not know. A second pass then
 * decides for each marked image: never load it (a tracking pixel), show it
 * from the given data, or leave it blocked.
 */
const REMOTE_MARK: &str = "x-remote:";
/** An image this small in a message has one purpose: to report that the message was opened. */
const TRACKER_MAX_SIZE: f32 = 2.0;

/** The page around the cleaned message. Its policy is guard 2: no network, no scripts. */
const PAGE_START: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'\">\
<meta name=\"referrer\" content=\"no-referrer\">\
<style>html{background:#fff;scrollbar-width:thin;scrollbar-color:#4a4f61 #1d2029}body{margin:16px;color:#1b1b1f;background:#fff;font:14px/1.5 'Segoe UI',system-ui,sans-serif;overflow-wrap:anywhere}\
img{max-width:100%;height:auto}pre{white-space:pre-wrap;font:inherit;margin:0}a{color:#1a56c4}</style></head><body>";
const PAGE_END: &str = "</body></html>";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AttachmentInfo {
    /** The attachment's position in the message; save_attachment uses it. */
    pub index: u32,
    pub name: String,
    pub mime: String,
    pub size: usize,
    /** True for a file type that can run code or hold macros. */
    pub risky: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageView {
    pub subject: String,
    pub from_name: String,
    pub from_address: String,
    pub to: String,
    pub cc: String,
    /** The full page for the preview frame. */
    pub html: String,
    /** How many remote images are not shown. The user can ask for these. */
    pub blocked_images: u32,
    /** How many tracking pixel images were removed. Mail Sage never loads these. */
    pub blocked_trackers: u32,
    /** True when the user said that images from this sender always load. */
    pub sender_trusted: bool,
    /** True when the user asked for the remote images of this message. */
    pub remote_loaded: bool,
    pub attachments: Vec<AttachmentInfo>,
}

fn lowercase_ascii_prefix(value: &str, prefix: &str) -> bool {
    value.len() >= prefix.len() && value.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/** Makes style text harmless: no fetch, no code. Text with a backslash is dropped, because CSS escapes can hide a blocked word. */
fn clean_css(css: &str) -> String {
    if css.contains('\\') {
        return String::new();
    }
    let mut result = css.to_string();
    for (word, replacement) in CSS_BLOCKED {
        let mut cleaned = String::with_capacity(result.len());
        let mut rest = result.as_str();
        while let Some(position) = rest.to_ascii_lowercase().find(word) {
            cleaned.push_str(&rest[..position]);
            cleaned.push_str(replacement);
            rest = &rest[position + word.len()..];
        }
        cleaned.push_str(rest);
        result = cleaned;
    }
    result
}

/** Applies clean_css to the text of each `<style>` block in cleaned HTML. */
fn clean_style_blocks(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<style>") {
        let body_start = start + "<style>".len();
        let Some(length) = rest[body_start..].find("</style>") else {
            /* A block with no end is not valid output of the cleaner; drop it and all that follows. */
            return result + &rest[..start];
        };
        result.push_str(&rest[..body_start]);
        result.push_str(&clean_css(&rest[body_start..body_start + length]));
        rest = &rest[body_start + length..];
    }
    result + rest
}

/** A link address that is safe to keep: web and mail addresses only. */
fn link_address(value: &str) -> Option<String> {
    let value = value.trim();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lowercase_ascii_prefix(value, scheme))
        .then(|| value.to_string())
}

/** True for a `data:` address that holds one of the allowed image types in base64. */
fn is_safe_data_image(value: &str) -> bool {
    IMAGE_TYPES.iter().any(|(_, mime)| lowercase_ascii_prefix(value, &format!("data:{mime};base64,")))
}

/** The image type of file bytes, from their first bytes. The type that a sender or a server claims is not trusted. */
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/** A `data:` address for image bytes, or None when the bytes are not an allowed image. */
pub fn data_address(bytes: &[u8]) -> Option<String> {
    image_mime(bytes).map(|mime| format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

pub struct Cleaned {
    pub html: String,
    /** The remote image addresses that the message asks for and that are not shown. */
    pub blocked_images: Vec<String>,
    /** How many tracking pixel images were removed. */
    pub trackers: u32,
}

/** The number at the start of a CSS or HTML size, for example 1 from "1px". */
fn leading_number(text: &str) -> Option<(f32, &str)> {
    let end = text.find(|character: char| !character.is_ascii_digit() && character != '.').unwrap_or(text.len());
    text[..end].parse().ok().map(|number| (number, &text[end..]))
}

/** True when style text gives `property` a size of a few pixels or less. `style` is lower case with no spaces. */
fn css_size_is_tiny(style: &str, property: &str) -> bool {
    let mut rest = style;
    while let Some(position) = rest.find(property) {
        let before = rest[..position].chars().last();
        let after = &rest[position + property.len()..];
        /* "line-height" and "min-width" are different properties; "max-width" limits the size just as well. */
        let own_property = before.is_none_or(|character| character == ';' || character == '{') || rest[..position].ends_with("max-");
        if own_property {
            if let Some((number, unit)) = after.strip_prefix(':').and_then(leading_number) {
                if number <= TRACKER_MAX_SIZE && !unit.starts_with('%') && !unit.starts_with("em") && !unit.starts_with("rem") {
                    return true;
                }
            }
        }
        rest = after;
    }
    false
}

/** True for an image that the reader cannot see: a size of a few pixels or less, or hidden by its style. */
fn is_tracking_pixel(attributes: &[(&str, &str)]) -> bool {
    let value = |name: &str| attributes.iter().find(|(key, _)| *key == name).map(|(_, value)| value.trim());
    let tiny = |name: &str| value(name).and_then(leading_number).is_some_and(|(number, unit)| number <= TRACKER_MAX_SIZE && !unit.starts_with('%'));
    if tiny("width") || tiny("height") {
        return true;
    }
    let style: String = value("style").unwrap_or("").to_ascii_lowercase().chars().filter(|character| !character.is_whitespace()).collect();
    if style.contains("display:none") || style.contains("visibility:hidden") {
        return true;
    }
    if let Some(position) = style.find("opacity:0") {
        if !style[position + "opacity:0".len()..].starts_with(|character: char| character == '.' || character.is_ascii_digit()) {
            return true;
        }
    }
    css_size_is_tiny(&style, "width") || css_size_is_tiny(&style, "height")
}

/** Reads the attributes of one tag in cleaned HTML. `tag` starts after the tag name. Returns the attributes and the length up to and with the closing `>`. */
fn read_attributes(tag: &str) -> Option<(Vec<(&str, &str)>, usize)> {
    let mut attributes = Vec::new();
    let mut position = 0;
    loop {
        let rest = &tag[position..];
        let trimmed = rest.trim_start_matches(|character: char| character.is_whitespace() || character == '/');
        position += rest.len() - trimmed.len();
        if trimmed.starts_with('>') {
            return Some((attributes, position + 1));
        }
        let name_length = trimmed.find(|character: char| character == '=' || character == '>' || character.is_whitespace())?;
        let name = &trimmed[..name_length];
        position += name_length;
        if let Some(value_start) = tag[position..].strip_prefix("=\"") {
            let value_length = value_start.find('"')?;
            attributes.push((name, &value_start[..value_length]));
            position += 2 + value_length + 1;
        } else {
            attributes.push((name, ""));
        }
    }
}

/**
 * The second pass over cleaned HTML: decides what each marked remote image
 * becomes. A tracking pixel goes. An image with data in `remote` shows from
 * that data. Each other image stays, with no address, and is counted as
 * blocked.
 */
fn place_remote_images(html: &str, addresses: &[String], remote: &HashMap<String, String>) -> Cleaned {
    let mut result = String::with_capacity(html.len());
    let mut blocked: Vec<String> = Vec::new();
    let mut trackers = 0;
    let mut rest = html;
    while let Some(start) = rest.find("<img") {
        let after_name = &rest[start + 4..];
        let is_tag = after_name.starts_with(|character: char| character.is_whitespace() || character == '>' || character == '/');
        let Some((attributes, length)) = is_tag.then(|| read_attributes(after_name)).flatten() else {
            result.push_str(&rest[..start + 4]);
            rest = after_name;
            continue;
        };
        result.push_str(&rest[..start]);
        rest = &after_name[length..];
        let marked = attributes.iter().find(|(name, value)| *name == "src" && value.starts_with(REMOTE_MARK)).map(|(_, value)| &value[REMOTE_MARK.len()..]);
        let Some(number) = marked else {
            result.push_str(&html_tag("img", &attributes, None));
            continue;
        };
        let Some(address) = number.parse::<usize>().ok().and_then(|number| addresses.get(number)) else { continue };
        if is_tracking_pixel(&attributes) {
            trackers += 1;
            continue;
        }
        let data = remote.get(address);
        if data.is_none() && !blocked.contains(address) {
            blocked.push(address.clone());
        }
        result.push_str(&html_tag("img", &attributes, Some(data.map(String::as_str))));
    }
    result.push_str(rest);
    Cleaned { html: result, blocked_images: blocked, trackers }
}

/** Writes a tag again from its attributes. `source` replaces the `src` attribute: with a new value, or with nothing. */
fn html_tag(name: &str, attributes: &[(&str, &str)], source: Option<Option<&str>>) -> String {
    let mut tag = format!("<{name}");
    for (key, value) in attributes {
        match (*key, source) {
            ("src", Some(Some(data))) => tag.push_str(&format!(" src=\"{data}\"")),
            ("src", Some(None)) => {}
            _ => tag.push_str(&format!(" {key}=\"{value}\"")),
        }
    }
    tag + ">"
}

/**
 * Cleans message HTML (guard 1). `inline` has the images that came inside
 * the message, by content id. `remote` has the remote images that the user
 * asked for, by address; each value is a `data:` address.
 */
pub fn clean_html(html: &str, inline: HashMap<String, String>, remote: HashMap<String, String>) -> Cleaned {
    let addresses: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let addresses_in_filter = addresses.clone();

    let image_source = move |value: &str| -> Option<String> {
        let value = value.trim();
        if lowercase_ascii_prefix(value, "cid:") {
            inline.get(value[4..].trim_matches(|character| character == '<' || character == '>')).cloned()
        } else if lowercase_ascii_prefix(value, "data:") {
            is_safe_data_image(value).then(|| value.to_string())
        } else if lowercase_ascii_prefix(value, "http://") || lowercase_ascii_prefix(value, "https://") {
            /* The image gets a mark with a number; place_remote_images decides what it becomes. */
            let mut addresses = addresses_in_filter.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let number = match addresses.iter().position(|address| address == value) {
                Some(number) => number,
                None => {
                    addresses.push(value.to_string());
                    addresses.len() - 1
                }
            };
            Some(format!("{REMOTE_MARK}{number}"))
        } else {
            None
        }
    };

    let mut builder = ammonia::Builder::default();
    builder
        .rm_clean_content_tags(["style"])
        .add_tags(["style", "font", "center"])
        .add_generic_attributes([
            "style", "class", "align", "valign", "bgcolor", "color", "face", "size", "width", "height", "border", "cellpadding",
            "cellspacing", "dir", "lang", "title", "nowrap",
        ])
        .url_schemes(HashSet::from(["http", "https", "mailto", "cid", "data"]))
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(move |element, attribute, value| match (element, attribute) {
            ("a", "href") => link_address(value).map(Cow::Owned),
            ("img", "src") => image_source(value).map(Cow::Owned),
            (_, "style") => Some(Cow::Owned(clean_css(value))),
            /* Every other attribute that can hold an address goes. */
            (_, "href" | "src" | "srcset" | "xlink:href" | "poster" | "ping" | "action" | "formaction" | "data" | "background" | "lowsrc" | "dynsrc") => None,
            _ => Some(Cow::Borrowed(value)),
        });
    let html = clean_style_blocks(&builder.clean(html).to_string());
    let addresses = std::mem::take(&mut *addresses.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    place_remote_images(&html, &addresses, &remote)
}

fn escape_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn page(body: &str) -> String {
    format!("{PAGE_START}{body}{PAGE_END}")
}

pub fn address_list(address: Option<&Address<'_>>) -> String {
    let Some(address) = address else { return String::new() };
    address
        .iter()
        .filter_map(|entry| match (entry.name().map(str::trim).filter(|name| !name.is_empty()), entry.address()) {
            (Some(name), Some(address)) => Some(format!("{name} <{address}>")),
            (None, Some(address)) => Some(address.to_string()),
            (Some(name), None) => Some(name.to_string()),
            (None, None) => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/**
 * A file name that is safe to offer in a save dialog: no folder parts, no
 * control characters, and no characters that change the direction of text
 * (those can make "exe.pdf" look like a PDF when the real ending is ".exe").
 */
pub fn safe_file_name(name: &str, index: u32) -> String {
    let cleaned: String = name
        .chars()
        .filter(|character| !character.is_control() && !matches!(*character, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .map(|character| if matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { character })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).trim_start_matches('.');
    let cleaned: String = cleaned.chars().rev().take(MAX_FILE_NAME_LENGTH).collect::<Vec<_>>().into_iter().rev().collect();
    if cleaned.is_empty() { format!("attachment-{}", index + 1) } else { cleaned }
}

pub fn is_risky_file(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| RISKY_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

/** The images that came inside the message, as `data:` addresses by content id. */
fn inline_images(message: &mail_parser::Message<'_>) -> HashMap<String, String> {
    let mut images = HashMap::new();
    let mut total = 0;
    for part in &message.parts {
        let Some(id) = part.content_id() else { continue };
        let (PartType::Binary(bytes) | PartType::InlineBinary(bytes)) = &part.body else { continue };
        total += bytes.len();
        if total > MAX_INLINE_IMAGE_BYTES {
            break;
        }
        if let Some(address) = data_address(bytes) {
            images.insert(id.trim_matches(|character| character == '<' || character == '>').to_string(), address);
        }
    }
    images
}

/** The bytes and the safe file name of one attachment. */
pub fn attachment(raw: &[u8], index: u32) -> Result<(String, Vec<u8>)> {
    let message = MessageParser::default().parse(raw).context("Mail Sage cannot read this message")?;
    let part = message.attachment(index).context("the message has no such attachment")?;
    Ok((safe_file_name(part.attachment_name().unwrap_or(""), index), part.contents().to_vec()))
}

/**
 * Builds the preview of a raw message. `remote` has the remote images that
 * the user asked for (address to `data:` address); it is empty by default,
 * and then no remote image shows. The second result is the list of remote
 * image addresses that are not shown.
 */
pub fn build_view(raw: &[u8], remote: HashMap<String, String>) -> Result<(MessageView, Vec<String>)> {
    let message = MessageParser::default().parse(raw).context("Mail Sage cannot read this message")?;
    let remote_loaded = !remote.is_empty();
    let inline = inline_images(&message);
    let inline_ids: Vec<String> = inline.keys().cloned().collect();

    let html_source = message.html_bodies().find_map(|part| match &part.body {
        PartType::Html(html) => Some(html.as_ref()),
        _ => None,
    });
    let (body, blocked, trackers) = match html_source {
        Some(html) => {
            let cleaned = clean_html(html, inline, remote);
            (cleaned.html, cleaned.blocked_images, cleaned.trackers)
        }
        None => (format!("<pre>{}</pre>", escape_text(&message.body_text(0).unwrap_or_default())), Vec::new(), 0),
    };

    let attachments = message
        .attachments()
        .enumerate()
        .filter(|(_, part)| {
            /* An image that the message body shows is not listed again as a file. */
            let shown_inline = part.content_id().is_some_and(|id| {
                let id = id.trim_matches(|character| character == '<' || character == '>');
                inline_ids.iter().any(|known| known == id) && html_source.is_some_and(|html| html.contains(&format!("cid:{id}")))
            });
            !shown_inline
        })
        .map(|(index, part)| {
            let index = index as u32;
            let name = safe_file_name(part.attachment_name().unwrap_or(""), index);
            let mime = part
                .content_type()
                .map(|kind| match kind.subtype() {
                    Some(subtype) => format!("{}/{}", kind.ctype(), subtype),
                    None => kind.ctype().to_string(),
                })
                .unwrap_or_else(|| "application/octet-stream".to_string());
            AttachmentInfo { index, risky: is_risky_file(&name), name, mime, size: part.contents().len() }
        })
        .collect();

    let sender = message.from().and_then(|from| from.first());
    let view = MessageView {
        subject: message.subject().unwrap_or("").trim().to_string(),
        from_name: sender.and_then(|sender| sender.name()).unwrap_or("").trim().to_string(),
        from_address: sender.and_then(|sender| sender.address()).unwrap_or("").trim().to_string(),
        to: address_list(message.to()),
        cc: address_list(message.cc()),
        html: page(&body),
        blocked_images: blocked.len() as u32,
        blocked_trackers: trackers,
        sender_trusted: false,
        remote_loaded,
        attachments,
    };
    Ok((view, blocked))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> Cleaned {
        clean_html(html, HashMap::new(), HashMap::new())
    }

    /** A 1 x 1 transparent GIF. */
    const TINY_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

    #[test]
    fn removes_scripts_and_event_attributes() {
        let html = clean("<p onclick=\"steal()\">Hi</p><script>steal()</script><img src=\"x\" onerror=\"steal()\"><svg onload=\"steal()\"></svg>").html;
        assert!(!html.contains("steal"), "{html}");
        assert!(html.contains("<p>Hi</p>"));
    }

    #[test]
    fn removes_links_that_run_code() {
        let html = clean("<a href=\"javascript:steal()\">a</a><a href=\" JaVaScRiPt:steal()\">b</a><a href=\"data:text/html,<script>steal()</script>\">c</a><a href=\"vbscript:steal()\">d</a><a href=\"file:///C:/x\">e</a>").html;
        assert!(!html.contains("href"), "{html}");
        assert!(!html.contains("steal"), "{html}");
    }

    #[test]
    fn keeps_plain_links_and_marks_them() {
        let html = clean("<a href=\"https://example.com/a?b=1\" target=\"_top\">x</a><a href=\"mailto:a@example.com\">y</a><a href=\"/relative\">z</a>").html;
        assert!(html.contains("href=\"https://example.com/a?b=1\""));
        assert!(html.contains("href=\"mailto:a@example.com\""));
        assert!(html.contains("noopener noreferrer"));
        assert!(!html.contains("target"));
        assert!(!html.contains("/relative"));
    }

    #[test]
    fn removes_forms_frames_objects_and_page_tags() {
        let html = clean(
            "<form action=\"https://evil.example/\"><input name=\"password\"><button>Go</button></form>\
             <iframe src=\"https://evil.example/\"></iframe><object data=\"https://evil.example/x\"></object><embed src=\"https://evil.example/x\">\
             <meta http-equiv=\"refresh\" content=\"0;url=https://evil.example/\"><base href=\"https://evil.example/\">\
             <link rel=\"stylesheet\" href=\"https://evil.example/x.css\"><video src=\"https://evil.example/v\"></video><audio src=\"https://evil.example/a\"></audio>",
        );
        assert!(!cleaned_has_any(&clean_text_only(&html.html), &["evil.example", "<form", "<input", "<iframe", "<object", "<embed", "<meta", "<base", "<link", "<video", "<audio"]), "{}", html.html);
    }

    fn clean_text_only(html: &str) -> String {
        html.to_ascii_lowercase()
    }

    fn cleaned_has_any(html: &str, needles: &[&str]) -> bool {
        needles.iter().any(|needle| html.contains(needle))
    }

    #[test]
    fn blocks_remote_images_and_counts_them() {
        let cleaned = clean("<img src=\"https://shop.example/banner.gif?id=123\" width=\"600\" alt=\"Sale\"><img src=\"http://shop.example/b.png\"><img src=\"https://shop.example/banner.gif?id=123\"><img srcset=\"https://shop.example/c.png 2x\">");
        assert!(!cleaned.html.contains("shop.example"), "{}", cleaned.html);
        assert!(!cleaned.html.contains(REMOTE_MARK), "{}", cleaned.html);
        assert!(cleaned.html.contains("alt=\"Sale\""));
        assert_eq!(cleaned.blocked_images, ["https://shop.example/banner.gif?id=123", "http://shop.example/b.png"]);
        assert_eq!(cleaned.trackers, 0);
    }

    #[test]
    fn removes_tracking_pixels_and_never_loads_them() {
        let pixels = [
            "<img src=\"https://t.example/1\" width=\"1\" height=\"1\">",
            "<img src=\"https://t.example/2\" height=\"0\">",
            "<img src=\"https://t.example/3\" width=\"1px\">",
            "<img src=\"https://t.example/4\" style=\"display: none\">",
            "<img src=\"https://t.example/5\" style=\"VISIBILITY:HIDDEN\">",
            "<img src=\"https://t.example/6\" style=\"opacity: 0;\">",
            "<img src=\"https://t.example/7\" style=\"border:0; width:1px; height:1px\">",
            "<img src=\"https://t.example/8\" style=\"max-height:0\">",
        ];
        /* Even when the caller has data for every address, a tracking pixel must not show. */
        let remote: HashMap<String, String> = (1..=8).map(|number| (format!("https://t.example/{number}"), "data:image/gif;base64,R0lG".to_string())).collect();
        let cleaned = clean_html(&pixels.concat(), HashMap::new(), remote);
        assert_eq!(cleaned.trackers, 8, "{}", cleaned.html);
        assert!(cleaned.blocked_images.is_empty());
        assert!(!cleaned.html.contains("<img"), "{}", cleaned.html);
    }

    #[test]
    fn keeps_visible_images_that_only_look_like_pixels() {
        let images = [
            "<img src=\"https://shop.example/1\" width=\"100\" height=\"20\">",
            "<img src=\"https://shop.example/2\" width=\"100%\">",
            "<img src=\"https://shop.example/3\" style=\"opacity:0.9; width:120px; line-height:1; min-width:1px\">",
            "<img src=\"https://shop.example/4\" style=\"border-width:1px; max-width:100%\">",
        ];
        let cleaned = clean(&images.concat());
        assert_eq!(cleaned.trackers, 0, "{}", cleaned.html);
        assert_eq!(cleaned.blocked_images.len(), 4);
    }

    #[test]
    fn a_message_cannot_forge_the_remote_mark() {
        let remote = HashMap::from([("https://shop.example/a.png".to_string(), "data:image/png;base64,AAAA".to_string())]);
        let cleaned = clean_html("<img src=\"x-remote:0\"><style>p::after{content:'<img src=\"x-remote:0\">'}</style><img src=\"https://shop.example/a.png\" title=\"a > b\">", HashMap::new(), remote);
        assert_eq!(cleaned.html.matches("data:image/png;base64,AAAA").count(), 2, "{}", cleaned.html);
        assert!(!cleaned.html.contains(REMOTE_MARK), "{}", cleaned.html);
        assert!(cleaned.html.starts_with("<img><style>"), "{}", cleaned.html);
        assert!(cleaned.html.contains("title=\"a &gt; b\""), "{}", cleaned.html);
    }

    #[test]
    fn shows_remote_images_only_from_the_given_data() {
        let remote = HashMap::from([("https://shop.example/logo.png".to_string(), "data:image/png;base64,AAAA".to_string())]);
        let cleaned = clean_html("<img src=\"https://shop.example/logo.png\"><img src=\"https://shop.example/other.png\">", HashMap::new(), remote);
        assert!(cleaned.html.contains("src=\"data:image/png;base64,AAAA\""));
        assert!(!cleaned.html.contains("shop.example"));
        assert_eq!(cleaned.blocked_images, ["https://shop.example/other.png"]);
    }

    #[test]
    fn shows_inline_images_and_drops_unknown_ones() {
        let inline = HashMap::from([("logo@mail".to_string(), "data:image/gif;base64,R0lG".to_string())]);
        let html = clean_html("<img src=\"cid:logo@mail\"><img src=\"CID:missing\">", inline, HashMap::new()).html;
        assert!(html.contains("src=\"data:image/gif;base64,R0lG\""));
        assert!(!html.to_ascii_lowercase().contains("cid:"));
    }

    #[test]
    fn keeps_only_safe_data_images() {
        let html = clean("<img src=\"data:image/png;base64,AAAA\"><img src=\"data:image/svg+xml;base64,PHN2Zz4=\"><img src=\"data:text/html;base64,PHNjcmlwdD4=\"><img src=\"data:image/png,raw\">").html;
        assert_eq!(html.matches("data:").count(), 1, "{html}");
        assert!(html.contains("data:image/png;base64,AAAA"));
    }

    #[test]
    fn makes_style_text_harmless() {
        let html = clean(
            "<style>@import url(https://evil.example/a.css); @IMPORT 'https://evil.example/b.css'; p { background: URL( https://evil.example/t.gif ); color: red } div { background: image-set('https://evil.example/i.png' 1x) }</style>\
             <p style=\"color: blue; background-image: url('https://evil.example/u.gif'); width: expression(steal())\">Hi</p>\
             <p style=\"background: u\\72l(https://evil.example/v.gif)\">Escaped</p>\
             <table background=\"https://evil.example/w.gif\"><tr><td>x</td></tr></table>",
        )
        .html;
        let lower = html.to_ascii_lowercase();
        assert!(!cleaned_has_any(&lower, &["url(", "@import", "image-set(", "expression(", "u\\72l", "w.gif"]), "{html}");
        assert!(lower.contains("color: red"));
        assert!(lower.contains("color: blue"));
    }

    #[test]
    fn a_style_block_cannot_bring_a_script_back() {
        let html = clean("<style>p{}</style><script>steal()</script><style></style><scr<script>ipt>steal()</script>").html;
        assert!(!html.to_ascii_lowercase().contains("<script"), "{html}");
    }

    #[test]
    fn escapes_plain_text_messages() {
        let raw = b"From: A <a@example.com>\r\nSubject: Plain\r\nContent-Type: text/plain\r\n\r\n<script>steal()</script> & <b>bold</b>\r\n";
        let (view, blocked) = build_view(raw, HashMap::new()).unwrap();
        assert!(view.html.contains("&lt;script&gt;steal()&lt;/script&gt; &amp; &lt;b&gt;bold&lt;/b&gt;"));
        assert!(!view.html.contains("<script>"));
        assert!(blocked.is_empty());
    }

    #[test]
    fn builds_a_view_with_inline_image_remote_image_and_attachments() {
        let gif = STANDARD.encode(TINY_GIF);
        let raw = format!(
            "From: =?UTF-8?Q?J=C3=BCrgen?= <j@example.com>\r\nTo: Me <me@example.com>, you@example.com\r\nSubject: Report\r\nMIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n\
             --outer\r\nContent-Type: multipart/related; boundary=\"inner\"\r\n\r\n\
             --inner\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
             <p>Hello</p><img src=\"cid:logo1\"><img src=\"https://tracker.example/open.gif\"><script>steal()</script>\r\n\
             --inner\r\nContent-Type: image/gif\r\nContent-ID: <logo1>\r\nContent-Disposition: inline; filename=\"logo.gif\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{gif}\r\n\
             --inner--\r\n\
             --outer\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"invoice.pdf.exe\"\r\nContent-Transfer-Encoding: base64\r\n\r\nTVqQAAM=\r\n\
             --outer\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n\
             --outer--\r\n"
        );
        let (view, blocked) = build_view(raw.as_bytes(), HashMap::new()).unwrap();
        assert_eq!(view.from_name, "Jürgen");
        assert_eq!(view.from_address, "j@example.com");
        assert_eq!(view.to, "Me <me@example.com>, you@example.com");
        assert!(view.html.contains("<p>Hello</p>"));
        assert!(view.html.contains(&format!("src=\"data:image/gif;base64,{gif}\"")));
        assert_eq!(view.blocked_trackers, 0);
        assert!(!view.html.contains("steal"));
        assert!(!view.html.contains("tracker.example"));
        assert_eq!(blocked, ["https://tracker.example/open.gif"]);
        assert_eq!(view.blocked_images, 1);
        assert!(!view.remote_loaded);
        let names: Vec<(&str, bool)> = view.attachments.iter().map(|attachment| (attachment.name.as_str(), attachment.risky)).collect();
        assert_eq!(names, [("invoice.pdf.exe", true), ("report.pdf", false)]);

        let (name, bytes) = attachment(raw.as_bytes(), view.attachments[1].index).unwrap();
        assert_eq!(name, "report.pdf");
        assert_eq!(bytes, b"%PDF-");
    }

    #[test]
    fn every_page_forbids_network_and_scripts() {
        let (view, _) = build_view(b"Subject: x\r\nContent-Type: text/html\r\n\r\n<p>x</p>", HashMap::new()).unwrap();
        assert!(view.html.contains("default-src 'none'; img-src data:; style-src 'unsafe-inline'"));
    }

    #[test]
    fn makes_file_names_safe() {
        assert_eq!(safe_file_name("..\\..\\Windows\\evil.exe", 0), "_.._Windows_evil.exe");
        assert_eq!(safe_file_name("invoice\u{202e}fdp.exe", 0), "invoicefdp.exe");
        assert_eq!(safe_file_name("  report.pdf. ", 0), "report.pdf");
        assert_eq!(safe_file_name("", 2), "attachment-3");
        assert_eq!(safe_file_name("a\r\nb.txt", 0), "ab.txt");
    }

    #[test]
    fn knows_risky_file_types() {
        assert!(is_risky_file("invoice.PDF.EXE"));
        assert!(is_risky_file("budget.xlsm"));
        assert!(is_risky_file("page.html"));
        assert!(!is_risky_file("report.pdf"));
        assert!(!is_risky_file("exe"));
    }

    #[test]
    fn finds_image_types_from_the_bytes() {
        assert_eq!(image_mime(TINY_GIF), Some("image/gif"));
        assert_eq!(image_mime(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(image_mime(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script/></svg>"), None);
        assert_eq!(image_mime(b"MZ\x90\x00"), None);
    }
}
