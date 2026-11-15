mod accounts;
mod google;
mod imap;
mod links;
mod preview;
mod remote;
mod secrets;
mod store;
mod updater;
mod window;

use tauri::Manager;

const STORE_FILE: &str = "mailsage.db";

fn restore_main_window(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let window_state_flags = tauri_plugin_window_state::StateFlags::SIZE
        | tauri_plugin_window_state::StateFlags::POSITION
        | tauri_plugin_window_state::StateFlags::MAXIMIZED;

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
        .setup(|app| {
            let path = app.path().app_data_dir()?.join(STORE_FILE);
            let store = store::Store::open(&path)?;
            app.manage(accounts::AppState::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            accounts::google_sign_in_ready,
            accounts::list_accounts,
            accounts::add_gmail_account,
            accounts::cancel_sign_in,
            accounts::remove_account,
            accounts::sync_folders,
            accounts::list_messages,
            accounts::sync_messages,
            accounts::open_message,
            accounts::save_attachment,
            accounts::set_sender_trusted,
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
