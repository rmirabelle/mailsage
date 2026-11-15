/** The commands that the window calls for accounts, folders, and message lists. */
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, State};
use tokio::sync::Notify;

use crate::store::{Account, Folder, MessageHeader, Store};
use crate::{google, imap, secrets};

/** How many of a folder's newest messages the list shows. */
const MESSAGE_LIST_SIZE: u32 = 200;
/** An access token with less time left than this is replaced before use. */
const TOKEN_SAFETY_MARGIN: Duration = Duration::from_secs(60);

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
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            access_tokens: Mutex::new(HashMap::new()),
            sign_in_running: AtomicBool::new(false),
            sign_in_cancel: Notify::new(),
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
