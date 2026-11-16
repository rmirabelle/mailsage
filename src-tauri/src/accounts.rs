/** The commands that the window calls for accounts, folders, and message lists. */
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
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

use crate::compose::{self, Draft, Outgoing, ReplyMode};
use crate::links::{self, LinkCheck};
use crate::preview::{self, MessageView};
use crate::{google, imap, remote, secrets};

/** How many of a folder's newest messages the list shows. */
const MESSAGE_LIST_SIZE: u32 = 200;
/** An access token with less time left than this is replaced before use. */
const TOKEN_SAFETY_MARGIN: Duration = Duration::from_secs(60);
/** The preview does not fetch a message larger than this. */
const MAX_MESSAGE_BYTES: u32 = 40 * 1024 * 1024;
/** How many messages stay in memory, and how many bytes in total. The oldest go first. Nothing goes to the disk. */
const MESSAGES_KEPT: usize = 120;
const MESSAGE_BYTES_KEPT: usize = 96 * 1024 * 1024;
/** Loading ahead skips a message larger than this, and asks the server for this many messages in one request. */
const PREFETCH_MAX_BYTES: u32 = 1536 * 1024;
const PREFETCH_BATCH: usize = 5;
/** How many open server connections wait for the next action, for each account. */
const IDLE_SESSIONS_KEPT: usize = 2;
/** A connection that waited longer than this is not used again: the network can drop a quiet connection without notice. */
const IDLE_SESSION_LIFE: Duration = Duration::from_secs(240);

/**
 * Runs one action on a server connection of the account. A new connection
 * costs about one second (network, encryption, sign-in), so a connection
 * stays open after an action and the next action uses it again. If the
 * action fails on such a waiting connection, the connection was possibly
 * dead, so the action runs one more time on a new connection.
 *
 * `$session` is the name that `$action` uses for the connection. `$action`
 * can run two times, so it must be safe to repeat.
 */
macro_rules! with_session {
    ($state:expr, $account:expr, |$session:ident| $action:expr) => {{
        let (mut connection, reused) = $state.take_session($account).await?;
        let mut result = {
            let $session = &mut connection;
            $action.await
        };
        if result.is_err() && reused {
            drop(connection);
            connection = $state.open_session($account).await?;
            result = {
                let $session = &mut connection;
                $action.await
            };
        }
        $state.keep_session($account.id, connection, result.is_ok());
        result
    }};
}
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
    /** The files that the user picked in the "Attach" dialog. Only these can go out as attachments. */
    picked_files: Mutex<HashSet<PathBuf>>,
    /** Open server connections that wait for the next action, by account, each with the time it started to wait. */
    idle_sessions: Mutex<HashMap<i64, Vec<(imap::Session, Instant)>>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            access_tokens: Mutex::new(HashMap::new()),
            sign_in_running: AtomicBool::new(false),
            sign_in_cancel: Notify::new(),
            opened: Mutex::new(VecDeque::new()),
            picked_files: Mutex::new(HashSet::new()),
            idle_sessions: Mutex::new(HashMap::new()),
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

    fn picked_files(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        self.picked_files.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
        let raw: Result<Vec<u8>> = with_session!(self, &account, |session| imap::fetch_message(session, &folder.path, uid_validity, uid));
        let raw = Arc::new(raw?);
        self.keep_message(folder_id, uid, raw.clone());
        Ok(raw)
    }

    /** Puts a raw message in memory, and removes the oldest ones when there are too many or they are too large in total. */
    fn keep_message(&self, folder_id: i64, uid: u32, raw: Arc<Vec<u8>>) {
        let mut opened = self.opened();
        opened.retain(|message| !(message.folder_id == folder_id && message.uid == uid));
        opened.push_back(OpenedMessage { folder_id, uid, raw });
        while opened.len() > 1 && (opened.len() > MESSAGES_KEPT || opened.iter().map(|message| message.raw.len()).sum::<usize>() > MESSAGE_BYTES_KEPT) {
            opened.pop_front();
        }
    }

    /**
     * Loads messages into memory before the user opens them, so that the
     * preview shows at once. Messages that are in memory already, or that
     * are large, are skipped.
     */
    async fn prefetch(&self, folder_id: i64, uids: &[u32]) -> Result<()> {
        let wanted: Vec<u32> = uids
            .iter()
            .copied()
            .filter(|uid| self.opened_message(folder_id, *uid).is_none())
            .filter(|uid| self.store.message_size(folder_id, *uid).is_ok_and(|size| size <= PREFETCH_MAX_BYTES))
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let folder = self.store.folder(folder_id)?;
        let uid_validity = self.store.folder_uid_validity(folder_id)?;
        let account = self.store.account(folder.account_id)?;
        for batch in wanted.chunks(PREFETCH_BATCH) {
            /* The user possibly opened one of these in the meantime. */
            let batch: Vec<u32> = batch.iter().copied().filter(|uid| self.opened_message(folder_id, *uid).is_none()).collect();
            if batch.is_empty() {
                continue;
            }
            let messages: Result<Vec<(u32, Vec<u8>)>> = with_session!(self, &account, |session| imap::fetch_messages(session, &folder.path, uid_validity, &batch));
            for (uid, raw) in messages? {
                if self.opened_message(folder_id, uid).is_none() {
                    self.keep_message(folder_id, uid, Arc::new(raw));
                }
            }
        }
        Ok(())
    }

    fn idle_sessions(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Vec<(imap::Session, Instant)>>> {
        self.idle_sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /** A server connection for the account: one that waits, or a new one. The second value is true for one that waited. */
    async fn take_session(&self, account: &Account) -> Result<(imap::Session, bool)> {
        let waiting = {
            let mut idle = self.idle_sessions();
            let sessions = idle.entry(account.id).or_default();
            sessions.retain(|(_, since)| since.elapsed() < IDLE_SESSION_LIFE);
            sessions.pop()
        };
        match waiting {
            Some((session, _)) => Ok((session, true)),
            None => Ok((self.open_session(account).await?, false)),
        }
    }

    /** After an action: a connection that worked waits for the next action. A connection that failed, or one too many, closes. */
    fn keep_session(&self, account_id: i64, session: imap::Session, worked: bool) {
        if !worked {
            return;
        }
        let mut idle = self.idle_sessions();
        let sessions = idle.entry(account_id).or_default();
        if sessions.len() < IDLE_SESSIONS_KEPT {
            sessions.push((session, Instant::now()));
        }
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
    state.idle_sessions().remove(&account_id);
    state.store.delete_account(account_id).map_err(text)
}

async fn sync_account_folders(state: &AppState, account_id: i64) -> Result<Vec<Folder>> {
    let account = state.store.account(account_id)?;
    let folders: Result<Vec<crate::store::FolderInfo>> = with_session!(state, &account, |session| imap::list_folders(session));
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
    let fetched: Result<imap::FolderFetch> = with_session!(state, &account, |session| imap::fetch_newest(session, &folder.path, MESSAGE_LIST_SIZE));
    let fetched = fetched?;
    state.store.save_messages(folder_id, fetched.uid_validity, fetched.lowest_uid, &fetched.headers)?;
    state.store.messages(folder_id, MESSAGE_LIST_SIZE)
}

/** Reads the folder's newest messages from the server, stores them, and returns the list. */
#[tauri::command]
pub async fn sync_messages(state: State<'_, AppState>, folder_id: i64) -> Result<Vec<MessageHeader>, String> {
    sync_folder_messages(&state, folder_id).await.map_err(text)
}

/** Stores more messages of a folder and gives them back, newest first. */
fn keep_headers(state: &AppState, folder_id: i64, mut headers: Vec<MessageHeader>) -> Result<Vec<MessageHeader>> {
    state.store.add_messages(folder_id, &headers)?;
    headers.sort_by(|first, second| second.date.cmp(&first.date).then(second.uid.cmp(&first.uid)));
    Ok(headers)
}

async fn older_messages(state: &AppState, folder_id: i64, before_uid: u32) -> Result<Vec<MessageHeader>> {
    let folder = state.store.folder(folder_id)?;
    let uid_validity = state.store.folder_uid_validity(folder_id)?;
    let account = state.store.account(folder.account_id)?;
    let fetched: Result<Vec<MessageHeader>> =
        with_session!(state, &account, |session| imap::fetch_older(session, &folder.path, uid_validity, before_uid, MESSAGE_LIST_SIZE as usize));
    keep_headers(state, folder_id, fetched?)
}

/** Reads the next older messages of a folder from the server: those before the message `before_uid`. Returns only the new ones. */
#[tauri::command]
pub async fn load_older_messages(state: State<'_, AppState>, folder_id: i64, before_uid: u32) -> Result<Vec<MessageHeader>, String> {
    older_messages(&state, folder_id, before_uid).await.map_err(text)
}

async fn found_messages(state: &AppState, folder_id: i64, search: &str) -> Result<Vec<MessageHeader>> {
    let Some(query) = imap::search_query(search) else {
        return Ok(Vec::new());
    };
    let folder = state.store.folder(folder_id)?;
    let uid_validity = state.store.folder_uid_validity(folder_id)?;
    let account = state.store.account(folder.account_id)?;
    let fetched: Result<Vec<MessageHeader>> =
        with_session!(state, &account, |session| imap::search_headers(session, &folder.path, uid_validity, &query, MESSAGE_LIST_SIZE as usize));
    keep_headers(state, folder_id, fetched?)
}

/**
 * Searches a full folder on the server by sender and subject. Returns the
 * newest messages that match. The search text goes only to the mail server
 * of the account.
 */
#[tauri::command]
pub async fn search_messages(state: State<'_, AppState>, folder_id: i64, search: String) -> Result<Vec<MessageHeader>, String> {
    found_messages(&state, folder_id, &search).await.map_err(text)
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

/**
 * Loads messages into memory ahead of time, so that their preview shows at
 * once. The window calls this for the messages that the user will probably
 * open next. A failure here shows no error: the message loads when the user
 * opens it.
 */
#[tauri::command]
pub async fn prefetch_messages(state: State<'_, AppState>, folder_id: i64, uids: Vec<u32>) -> Result<(), String> {
    let _ = state.prefetch(folder_id, &uids).await;
    Ok(())
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

async fn change_seen(state: &AppState, folder_id: i64, uids: &[u32], seen: bool) -> Result<Vec<Folder>> {
    let folder = state.store.folder(folder_id)?;
    if !uids.is_empty() {
        let uid_validity = state.store.folder_uid_validity(folder_id)?;
        let account = state.store.account(folder.account_id)?;
        let result: Result<()> = with_session!(state, &account, |session| imap::set_seen(session, &folder.path, uid_validity, uids, seen));
        result?;
        state.store.mark_seen(folder_id, uids, seen)?;
    }
    state.store.folders(folder.account_id)
}

/** Marks messages as read or as unread, on the server and in the store. Returns the account's folders with the new counts. */
#[tauri::command]
pub async fn set_messages_seen(state: State<'_, AppState>, folder_id: i64, uids: Vec<u32>, seen: bool) -> Result<Vec<Folder>, String> {
    change_seen(&state, folder_id, &uids, seen).await.map_err(text)
}

async fn remove_messages(state: &AppState, folder_id: i64, uids: &[u32]) -> Result<Vec<Folder>> {
    let folder = state.store.folder(folder_id)?;
    if !uids.is_empty() {
        let uid_validity = state.store.folder_uid_validity(folder_id)?;
        let account = state.store.account(folder.account_id)?;
        /* A message in the trash is deleted for good. A message in any other folder moves to the trash. */
        let trash = if folder.role == "trash" {
            None
        } else {
            Some(state.store.folder_with_role(account.id, "trash")?.context("This account has no trash folder. Refresh the folders, then try again.")?)
        };
        let result: Result<()> = match &trash {
            Some(trash) => with_session!(state, &account, |session| imap::move_messages(session, &folder.path, uid_validity, uids, &trash.path)),
            None => with_session!(state, &account, |session| imap::delete_forever(session, &folder.path, uid_validity, uids)),
        };
        result?;
        state.store.remove_messages(folder_id, uids, trash.map(|trash| trash.id))?;
        state.opened().retain(|message| !(message.folder_id == folder_id && uids.contains(&message.uid)));
    }
    state.store.folders(folder.account_id)
}

/**
 * Deletes messages: a message moves to the account's trash folder; a message
 * that is in the trash already is deleted for good. Returns the account's
 * folders with the new counts.
 */
#[tauri::command]
pub async fn delete_messages(state: State<'_, AppState>, folder_id: i64, uids: Vec<u32>) -> Result<Vec<Folder>, String> {
    remove_messages(&state, folder_id, &uids).await.map_err(text)
}

async fn mark_all_seen(state: &AppState, folder_id: i64) -> Result<Vec<Folder>> {
    let folder = state.store.folder(folder_id)?;
    let uid_validity = state.store.folder_uid_validity(folder_id)?;
    let account = state.store.account(folder.account_id)?;
    let result: Result<()> = with_session!(state, &account, |session| imap::set_all_seen(session, &folder.path, uid_validity));
    result?;
    state.store.mark_all_seen(folder_id)?;
    state.store.folders(folder.account_id)
}

/** Marks all messages of a folder as read, on the server and in the store. Returns the account's folders with the new counts. */
#[tauri::command]
pub async fn mark_folder_seen(state: State<'_, AppState>, folder_id: i64) -> Result<Vec<Folder>, String> {
    mark_all_seen(&state, folder_id).await.map_err(text)
}

async fn remove_all_messages(state: &AppState, folder_id: i64) -> Result<Vec<Folder>> {
    let folder = state.store.folder(folder_id)?;
    let uid_validity = state.store.folder_uid_validity(folder_id)?;
    let account = state.store.account(folder.account_id)?;
    /* The same rule as for one message: the trash is emptied for good; any other folder moves to the trash. */
    let trash = if folder.role == "trash" {
        None
    } else {
        Some(state.store.folder_with_role(account.id, "trash")?.context("This account has no trash folder. Refresh the folders, then try again.")?)
    };
    let target = trash.as_ref().map(|trash| trash.path.as_str());
    let result: Result<()> = with_session!(state, &account, |session| imap::remove_all(session, &folder.path, uid_validity, target));
    result?;
    state.store.remove_all_messages(folder_id, trash.map(|trash| trash.id))?;
    state.opened().retain(|message| message.folder_id != folder_id);
    state.store.folders(folder.account_id)
}

/**
 * Deletes all messages of a folder: they move to the account's trash folder;
 * the messages of the trash folder are deleted for good. Returns the
 * account's folders with the new counts.
 */
#[tauri::command]
pub async fn delete_folder_messages(state: State<'_, AppState>, folder_id: i64) -> Result<Vec<Folder>, String> {
    remove_all_messages(&state, folder_id).await.map_err(text)
}

async fn make_reply_draft(state: &AppState, folder_id: i64, uid: u32, mode: ReplyMode) -> Result<Draft> {
    let folder = state.store.folder(folder_id)?;
    let account = state.store.account(folder.account_id)?;
    let raw = state.raw_message(folder_id, uid).await?;
    compose::reply_draft(&raw, mode, &account.email)
}

/** The starting content of the compose window for a reply, a reply to all, or a forward. */
#[tauri::command]
pub async fn reply_draft(state: State<'_, AppState>, folder_id: i64, uid: u32, mode: ReplyMode) -> Result<Draft, String> {
    make_reply_draft(&state, folder_id, uid, mode).await.map_err(text)
}

#[derive(Serialize)]
pub struct PickedFile {
    path: String,
    name: String,
    size: u64,
}

/** Opens the "Attach" dialog. Only the files that the user picks here can go out as attachments. */
#[tauri::command]
pub async fn pick_attachments(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<PickedFile>, String> {
    let dialog = app.dialog().file().set_title("Attach files");
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_files()).await.map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    for path in picked.unwrap_or_default().into_iter().filter_map(|path| path.into_path().ok()) {
        let Ok(details) = std::fs::metadata(&path) else { continue };
        if !details.is_file() {
            continue;
        }
        files.push(PickedFile {
            path: path.display().to_string(),
            name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            size: details.len(),
        });
        state.picked_files().insert(path);
    }
    Ok(files)
}

async fn send(state: &AppState, outgoing: Outgoing) -> Result<()> {
    let account = state.store.account(outgoing.account_id)?;
    let mut files = Vec::new();
    let mut total = 0_u64;
    for path in &outgoing.attachments {
        let path = PathBuf::from(path);
        if !state.picked_files().contains(&path) {
            bail!("Mail Sage attaches only files that you picked in the Attach dialog.");
        }
        let bytes = tokio::fs::read(&path).await.with_context(|| format!("could not read {}", path.display()))?;
        total += bytes.len() as u64;
        if total > compose::MAX_ATTACHMENT_BYTES {
            bail!("The attachments are too large. The limit is 24 MB in total.");
        }
        files.push((path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(), bytes));
    }
    let message = compose::build(&account.email, &outgoing, files)?;
    match account.kind.as_str() {
        "gmail" => {
            let token = state.access_token(&account).await?;
            compose::send_with_token(google::SMTP_HOST, &account.email, &token, message).await?;
        }
        other => bail!("Mail Sage cannot send from a \"{other}\" account yet"),
    }
    state.picked_files().clear();
    Ok(())
}

/** Sends a message from the compose window. Gmail keeps its own copy in the Sent folder. */
#[tauri::command]
pub async fn send_message(state: State<'_, AppState>, outgoing: Outgoing) -> Result<(), String> {
    send(&state, outgoing).await.map_err(text)
}
