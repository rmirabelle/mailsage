# MailSage repository instructions

Mail Sage is a Windows email client that replaces Outlook. Security and privacy
come first. Tauri 2 + React + TypeScript. `PLAN.md` has the full plan and the
order of work.

## Security rules

- All mail work (network, parsing, HTML cleaning) runs in Rust. The webview
  shows only data that Rust made safe.
- An email must never run code, contact a server, or open a file by itself.
- Passwords and sign-in tokens go to Windows Credential Manager only. Never
  to SQLite, files, or logs.
- No telemetry. The app contacts only mail servers, sign-in pages, and GitHub
  for updates.
- Keep the content security policy in `src-tauri/tauri.conf.json` strict. Do
  not set it to `null`.

## Layout

- `src/App.tsx` holds the window: title bar, then three panes (accounts,
  message list, secure preview).
- `src/components/WindowControls.tsx` and `cursor_position` in
  `src-tauri/src/window.rs` are the solved fix for hover that sticks at the
  window edge. Do not change the pattern.
- `src-tauri/src/accounts.rs` holds the commands that the window calls.
  `store.rs` is the SQLite store (accounts, folders, message headers).
  `secrets.rs` is Windows Credential Manager. `google.rs` is the Google
  sign-in. `imap.rs` reads folders and headers from the server.
- `src-tauri/src/preview.rs` is the secure preview: it cleans message HTML and
  builds the page for the preview frame. Its tests are the set of hostile
  emails; add a test there for each new attack. `links.rs` checks a link
  before it opens. `remote.rs` fetches remote images (only when the user
  asks, never from the local network). `src/components/MessagePreview.tsx`
  shows the page in a frame that cannot run scripts.
- Message bodies are not written to the disk. The last few opened messages
  stay in memory (`AppState::opened` in `accounts.rs`).
- `src-tauri/build.rs` reads the sign-in client values from the environment
  or from the gitignored `secrets/oauth.env`; the code embeds them with
  `obfstr`. `docs/GOOGLE-SETUP.md` has the registration steps.
- `src-tauri/src/updater.rs` checks `rmirabelle/mailsage` GitHub releases.

## Release memory

- Read `docs/RELEASING.md` before changing installer configuration, preparing a
  release, or publishing.
- Preserve the shared XSage per-user installer contract. Keep
  `src-tauri/installer.nsi`, the bundled `icons/icon.ico` resource, and the
  Windows GUI-subsystem attribute intact.
- Use `set-version.ps1` and `publish.ps1`; do not manually create release tags or
  GitHub releases. A local build is not authorization to publish.

## Development process

- The dev app must run outside Claude's process tree, or it stops when Claude
  quits. Start it only with `start-dev.vbs`, opened through Explorer:
  `Start-Process "$env:WINDIR\explorer.exe" '"D:\Code\MailSage\start-dev.vbs"'`.
  It runs `npm run tauri dev -- --no-watch` (Vite on port 14520) with no
  console window, and writes its output to `dev.log` in the project folder.
  Start or restart the dev app only when the user asks or agrees.
- Stop it with `.\kill-dev.ps1`. It closes the window, then tree-kills what is
  left of `mailsage.exe` and frees ports 14520 and 14521.
- Do not start the dev app while an installed Mail Sage is running; the
  single-instance plugin would route the launch to it.
