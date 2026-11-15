/** Returns the OS cursor position in physical pixels, for the window-control hover fix. */
#[tauri::command]
pub fn cursor_position() -> (i32, i32) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut point = POINT { x: 0, y: 0 };
        unsafe {
            let _ = GetCursorPos(&mut point);
        }
        (point.x, point.y)
    }
    #[cfg(not(windows))]
    {
        (0, 0)
    }
}

/** Longest page error that goes to the error output. A page error must never fill the log. */
const MAX_ERROR_LENGTH: usize = 8000;

/** Prints an error from the page to the app's error output (the dev log). */
#[tauri::command]
pub fn log_frontend_error(message: String) {
    let short: String = message.chars().take(MAX_ERROR_LENGTH).collect();
    eprintln!("[frontend error] {short}");
}
