/** The commands that the window calls for accounts, folders, and message lists. */
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, State};
use tokio::sync::Notify;

use crate::store::{Account, Folder, MessageHeader, Store};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::links::{self, LinkCheck};
use crate::preview::{self, MessageView};
use crate::{google, imap, remote, secrets};

/** How many of a folder's newest messages the list shows. */
const MESSAGE_LIST_SIZE: u32 = 200;
/** An access token with less time left than this is replaced before use. */
const TOKEN_SAFETY_MARGIN: Duration = Duration::from_secs(60);
/** The preview does not fetch a message larger than this. */
const MAX_MESSAGE_BYTES: u32 = 40 * 1024 * 1024;
/** How many opened messages stay in memory, so that "load images" and "save attachment" need no second fetch. */
const OPENED_MESSAGES_KEPT: usize = 6;
/** The mark that Windows puts on a file from the internet: zone 3. Programs then open the file with care. */
const INTERNET_ZONE_MARK: &str = "[ZoneTransfer]\r\nZoneId=3\r\n";

/** A raw message that the user opened, by folder and message number. */
struct OpenedMessage {
    folder_id: i64,
    uid: u32,
    raw: Arc<Vec<u8>>,
}

struct AccessToken {
    value: String,
    good_until: Instant,
}

pub struct AppState {
    store: Store,
    /** Short-lived access tokens, by account. They stay in memory only. */
    access_tokens: Mutex<HashMap<i64, AccessToken>>,
    sign_in_running: AtomicBool,
    sign_in_cancel: Notify,
    /** The last few opened messages. They stay in memory only; Mail Sage does not write message bodies to the disk. */
    opened: Mutex<VecDeque<OpenedMessage>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            access_tokens: Mutex::new(HashMap::new()),
            sign_in_running: AtomicBool::new(false),
            sign_in_cancel: Notify::new(),
            opened: Mutex::new(VecDeque::new()),
        }
    }

    fn tokens(&self) -> std::sync::MutexGuard<'_, HashMap<i64, AccessToken>> {
        self.access_tokens.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn keep_access_token(&self, account_id: i64, value: String, expires_in: u64) {
        let good_until = Instant::now() + Duration::from_secs(expires_in);
        self.tokens().insert(account_id, AccessToken { value, good_until });
    }

    /** An access token that works now: the one in memory, or a new one from the stored refresh token. */
    async fn access_token(&self, account: &Account) -> Result<String> {
        if let Some(token) = self.tokens().get(&account.id) {
            if token.good_until > Instant::now() + TOKEN_SAFETY_MARGIN {
                return Ok(token.value.clone());
            }
        }
        let refresh_token = secrets::read(&secrets::account_target(account.id))?
            .with_context(|| format!("Mail Sage has no sign-in for {}. Remove the account and add it again.", account.email))?;
        let (value, expires_in) = google::refresh(&refresh_token).await?;
        self.keep_access_token(account.id, value.clone(), expires_in);
        Ok(value)
    }

    fn opened(&self) -> std::sync::MutexGuard<'_, VecDeque<OpenedMessage>> {
        self.opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn opened_message(&self, folder_id: i64, uid: u32) -> Option<Arc<Vec<u8>>> {
        self.opened().iter().find(|message| message.folder_id == folder_id && message.uid == uid).map(|message| message.raw.clone())
    }

    /** The raw bytes of a message: from memory, or fetched from the server. */
    async fn raw_message(&self, folder_id: i64, uid: u32) -> Result<Arc<Vec<u8>>> {
        if let Some(raw) = self.opened_message(folder_id, uid) {
            return Ok(raw);
        }
        if self.store.message_size(folder_id, uid)? > MAX_MESSAGE_BYTES {
            bail!("This message is too large for the preview.");
        }
        let folder = self.store.folder(folder_id)?;
        let uid_validity = self.store.folder_uid_validity(folder_id)?;
        let account = self.store.account(folder.account_id)?;
        let mut session = self.open_session(&account).await?;
        let raw = imap::fetch_message(&mut session, &folder.path, uid_validity, uid).await;
        let _ = session.logout().await;
        let raw = Arc::new(raw?);
        let mut opened = self.opened();
        opened.push_back(OpenedMessage { folder_id, uid, raw: raw.clone() });
        while opened.len() > OPENED_MESSAGES_KEPT {
            opened.pop_front();
        }
        Ok(raw)
    }

    async fn open_session(&self, account: &Account) -> Result<imap::Session> {
        match account.kind.as_str() {
            "gmail" => {
                let token = self.access_token(account).await?;
                imap::open_with_token(&account.imap_host, account.imap_port, &account.email, &token).await
            }
            other => bail!("Mail Sage cannot connect to a \"{other}\" account yet"),
        }
    }
}

/** Clears the "sign-in running" mark when the sign-in ends, however it ends. */
struct SignInGuard<'a>(&'a AtomicBool);

impl Drop for SignInGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Serialize)]
pub struct AccountView {
    #[serde(flatten)]
    account: Account,
    folders: Vec<Folder>,
}

fn text(error: anyhow::Error) -> String {
    format!("{error:#}")
}

/** The same as `text`, for a command that has a parameter named `text`. */
fn error_text(error: anyhow::Error) -> String {
    text(error)
}

fn account_views(store: &Store) -> Result<Vec<AccountView>> {
    store
        .accounts()?
        .into_iter()
        .map(|account| Ok(AccountView { folders: store.folders(account.id)?, account }))
        .collect()
}

/** True when this build can sign in to Google. */
#[tauri::command]
pub fn google_sign_in_ready() -> bool {
    google::available()
}

/** All accounts with their stored folders. */
#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<AccountView>, String> {
    account_views(&state.store).map_err(text)
}

async fn add_gmail(app: &AppHandle, state: &AppState) -> Result<Account> {
    if state.sign_in_running.swap(true, Ordering::SeqCst) {
        bail!("A sign-in is already open in your browser.");
    }
    let _guard = SignInGuard(&state.sign_in_running);
    let sign_in = google::sign_in(app, &state.sign_in_cancel).await?;
    let account = match state.store.add_account("gmail", &sign_in.email, google::IMAP_HOST, google::IMAP_PORT) {
        Ok(account) => account,
        Err(error) => {
            /* The account is not added, so the permission that Google just gave is cancelled again. */
            let _ = google::revoke(&sign_in.refresh_token).await;
            return Err(error);
        }
    };
    if let Err(error) = secrets::save(&secrets::account_target(account.id), &sign_in.refresh_token) {
        let _ = state.store.delete_account(account.id);
        let _ = google::revoke(&sign_in.refresh_token).await;
        return Err(error);
    }
    state.keep_access_token(account.id, sign_in.access_token, sign_in.expires_in);
    Ok(account)
}

/** Signs in to Google in the browser and adds the Gmail account. */
#[tauri::command]
pub async fn add_gmail_account(app: AppHandle, state: State<'_, AppState>) -> Result<Account, String> {
    add_gmail(&app, &state).await.map_err(text)
}

/** Stops a sign-in that waits for the browser. */
#[tauri::command]
pub fn cancel_sign_in(state: State<'_, AppState>) {
    state.sign_in_cancel.notify_waiters();
}

/**
 * Removes an account: its stored mail, its sign-in in Windows Credential
 * Manager, and (best effort) the permission at Google.
 */
#[tauri::command]
pub async fn remove_account(state: State<'_, AppState>, account_id: i64) -> Result<(), String> {
    let target = secrets::account_target(account_id);
    if let Ok(Some(refresh_token)) = secrets::read(&target) {
        let _ = google::revoke(&refresh_token).await;
    }
    secrets::delete(&target).map_err(text)?;
    state.tokens().remove(&account_id);
    state.opened().clear();
    state.store.delete_account(account_id).map_err(text)
}

async fn sync_account_folders(state: &AppState, account_id: i64) -> Result<Vec<Folder>> {
    let account = state.store.account(account_id)?;
    let mut session = state.open_session(&account).await?;
    let folders = imap::list_folders(&mut session).await;
    let _ = session.logout().await;
    state.store.replace_folders(account_id, &folders?)?;
    state.store.folders(account_id)
}

/** Reads the account's folder list and counts from the server. */
#[tauri::command]
pub async fn sync_folders(state: State<'_, AppState>, account_id: i64) -> Result<Vec<Folder>, String> {
    sync_account_folders(&state, account_id).await.map_err(text)
}

/** The stored message list of a folder, newest first. */
#[tauri::command]
pub fn list_messages(state: State<'_, AppState>, folder_id: i64) -> Result<Vec<MessageHeader>, String> {
    state.store.messages(folder_id, MESSAGE_LIST_SIZE).map_err(text)
}

async fn sync_folder_messages(state: &AppState, folder_id: i64) -> Result<Vec<MessageHeader>> {
    let folder = state.store.folder(folder_id)?;
    if !folder.selectable {
        return Ok(Vec::new());
    }
    let account = state.store.account(folder.account_id)?;
    let mut session = state.open_session(&account).await?;
    let fetched = imap::fetch_newest(&mut session, &folder.path, MESSAGE_LIST_SIZE).await;
    let _ = session.logout().await;
    let fetched = fetched?;
    state.store.save_messages(folder_id, fetched.uid_validity, fetched.lowest_uid, &fetched.headers)?;
    state.store.messages(folder_id, MESSAGE_LIST_SIZE)
}

/** Reads the folder's newest messages from the server, stores them, and returns the list. */
#[tauri::command]
pub async fn sync_messages(state: State<'_, AppState>, folder_id: i64) -> Result<Vec<MessageHeader>, String> {
    sync_folder_messages(&state, folder_id).await.map_err(text)
}

async fn message_view(state: &AppState, folder_id: i64, uid: u32, load_remote: bool) -> Result<MessageView> {
    let raw = state.raw_message(folder_id, uid).await?;
    let (mut view, blocked) = preview::build_view(&raw, HashMap::new())?;
    let trusted = !view.from_address.is_empty() && state.store.is_sender_trusted(&view.from_address)?;
    view.sender_trusted = trusted;
    if !(load_remote || trusted) || blocked.is_empty() {
        return Ok(view);
    }
    let images = remote::fetch_images(&blocked).await;
    let (mut view, _) = preview::build_view(&raw, images)?;
    view.remote_loaded = true;
    view.sender_trusted = trusted;
    Ok(view)
}

/**
 * The safe preview of one message. With `load_remote`, or for a sender that
 * the user trusts, Rust fetches the message's visible remote images and puts
 * them in the page as data. Tracking pixel images never load.
 */
#[tauri::command]
pub async fn open_message(state: State<'_, AppState>, folder_id: i64, uid: u32, load_remote: bool) -> Result<MessageView, String> {
    message_view(&state, folder_id, uid, load_remote).await.map_err(text)
}

async fn save_attachment_file(app: &AppHandle, state: &AppState, folder_id: i64, uid: u32, index: u32) -> Result<Option<String>> {
    let raw = state.raw_message(folder_id, uid).await?;
    let (name, bytes) = preview::attachment(&raw, index)?;
    let dialog = app.dialog().file().set_title("Save attachment").set_file_name(&name);
    let Some(target) = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await? else {
        return Ok(None);
    };
    let path = target.into_path().map_err(|_| anyhow::anyhow!("Mail Sage cannot save to that place"))?;
    tokio::fs::write(&path, &bytes).await.with_context(|| format!("could not write {}", path.display()))?;
    /* The mark is a second, hidden stream of the file. A disk that has no such streams (FAT, some network disks) refuses it; the file is saved all the same. */
    let _ = tokio::fs::write(format!("{}:Zone.Identifier", path.display()), INTERNET_ZONE_MARK).await;
    Ok(Some(path.display().to_string()))
}

/**
 * Saves one attachment where the user chooses, and marks the file as "from
 * the internet". It never opens the file. Returns the path, or nothing when
 * the user cancelled.
 */
#[tauri::command]
pub async fn save_attachment(app: AppHandle, state: State<'_, AppState>, folder_id: i64, uid: u32, index: u32) -> Result<Option<String>, String> {
    save_attachment_file(&app, &state, folder_id, uid, index).await.map_err(text)
}

/** Adds a sender address to the list of senders whose remote images always load, or removes it. */
#[tauri::command]
pub fn set_sender_trusted(state: State<'_, AppState>, address: String, trusted: bool) -> Result<(), String> {
    state.store.set_sender_trusted(&address, trusted).map_err(text)
}

/** Checks a link from a message. `text` is what the message shows for the link. Nothing opens. */
#[tauri::command]
pub fn check_link(href: String, text: String) -> Result<LinkCheck, String> {
    links::check(&href, &text).map_err(error_text)
}

/** Opens a web link in the default browser, after the user confirmed it. The address is checked and cleaned again here. */
#[tauri::command]
pub fn open_link(app: AppHandle, url: String) -> Result<(), String> {
    let link = links::check(&url, "").map_err(error_text)?;
    app.opener().open_url(link.url, None::<&str>).map_err(|error| format!("could not open the browser: {error}"))
}
