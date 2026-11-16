mod accounts;
mod blocklist;
mod compose;
mod google;
mod imap;
mod links;
mod microsoft;
mod oauth;
mod preview;
mod remote;
mod secrets;
mod store;
mod updater;
mod window;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::Manager;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

const STORE_FILE: &str = "mailsage.db";

/** Counts the moves and size changes of the window. A delayed save runs only if no newer change came after it. */
static WINDOW_CHANGES: AtomicU64 = AtomicU64::new(0);
const WINDOW_SAVE_DELAY: Duration = Duration::from_millis(600);

/**
 * Saves the window size and position a short time after the last change.
 * The plugin saves them only at a normal exit; this keeps them also when
 * the app stops in a different way (a kill, a crash, an installer).
 */
fn save_window_state_soon(app: tauri::AppHandle, flags: StateFlags) {
    let change = WINDOW_CHANGES.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(WINDOW_SAVE_DELAY).await;
        if WINDOW_CHANGES.load(Ordering::SeqCst) == change {
            let _ = app.save_window_state(flags);
        }
    });
}

fn restore_main_window(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let window_state_flags = StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED;

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            restore_main_window(app);
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags)
                .build(),
        )
        /* Rust uses this to open the sign-in page in the browser. The window has no permission to use it. */
        .plugin(tauri_plugin_opener::init())
        /* Rust uses this for the "Save as" dialog of an attachment. The window has no permission to use it. */
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(move |window, event| {
            if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                save_window_state_soon(window.app_handle().clone(), window_state_flags);
            }
        })
        .setup(|app| {
            let path = app.path().app_data_dir()?.join(STORE_FILE);
            let store = store::Store::open(&path)?;
            app.manage(accounts::AppState::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            accounts::google_sign_in_ready,
            accounts::microsoft_sign_in_ready,
            accounts::list_accounts,
            accounts::add_gmail_account,
            accounts::add_microsoft_account,
            accounts::cancel_sign_in,
            accounts::remove_account,
            accounts::sync_folders,
            accounts::delete_folder,
            accounts::list_messages,
            accounts::sync_messages,
            accounts::load_older_messages,
            accounts::search_messages,
            accounts::open_message,
            accounts::prefetch_messages,
            accounts::save_attachment,
            accounts::set_sender_trusted,
            accounts::list_blocked,
            accounts::block_sender,
            accounts::add_blocked,
            accounts::unblock_sender,
            accounts::set_messages_seen,
            accounts::set_messages_flagged,
            accounts::delete_messages,
            accounts::move_to_inbox,
            accounts::move_messages_to,
            accounts::mark_folder_seen,
            accounts::delete_folder_messages,
            accounts::reply_draft,
            accounts::pick_attachments,
            accounts::send_message,
            accounts::check_link,
            accounts::open_link,
            window::cursor_position,
            window::log_frontend_error,
            updater::check_for_update,
            updater::download_and_run_installer,
            updater::get_app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running Mail Sage");
}
