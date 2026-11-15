# Mail Sage plan

Mail Sage is a Windows email client that replaces Outlook. It puts security and privacy first.

## 1. Technology

- Tauri 2 + React + TypeScript, the same as Kaleido Sage.
- All mail work runs in Rust. The webview only shows data that Rust has made safe.
- Mail protocol: IMAP to read, SMTP to send. Crates: `async-imap`, `lettre`, `mail-parser`.
- Local store: SQLite (`rusqlite`) in the per-user app data folder. Message bodies and attachments are stored encrypted. The key is in Windows Credential Manager.
- Sign-in tokens and passwords: Windows Credential Manager only (`keyring`). Never in SQLite, files, or logs.

## 2. Accounts

| Account | Sign-in | Server |
|---|---|---|
| Gmail | OAuth 2 (browser sign-in, PKCE) | imap.gmail.com / smtp.gmail.com |
| Outlook.com | OAuth 2 (Microsoft) | outlook.office365.com / smtp-mail.outlook.com |
| Hotmail | Same as Outlook.com | Same as Outlook.com |
| Work | Password (generic IMAP account type) | The company's own server; you enter the server names and ports |

- Google and Microsoft do not accept a plain password for IMAP. Each needs an app registration that you own: one in Google Cloud, one in Microsoft Entra. I will write the exact steps; you do the registration.
- Sign-in opens your default browser. Mail Sage never sees your password.
- The OAuth client IDs go into the binary through `obfstr` (global rule 5). `publish.ps1` checks the built files.
- A generic "IMAP + password" account type covers other providers.

## 3. Window layout

- Left pane: one group for each account. Each group shows its folders and unread counts. Groups collapse.
- Middle: the message list for the selected folder (sender, subject, date, attachment mark). Search box at the top.
- Right pane: the secure preview.
- Assumption: three columns. You asked for a left and a right pane, and the message list needs a place.
- Custom title bar buttons use the solved hover pattern from DB Sage (global rule 7).

## 4. Secure preview

The rule: an email cannot run code, cannot contact a server, and cannot open a file by itself.

- Rust cleans the HTML with `ammonia` before the webview gets it. It removes scripts, forms, frames, objects, event attributes, external style sheets, and `meta` refresh.
- The cleaned HTML shows in an `iframe` with `sandbox` (no scripts, no forms, no navigation) and a content security policy of `default-src 'none'` plus inline styles and `data:` images.
- Remote images and fonts are blocked by default. This stops tracking pixels. A bar says "Remote content blocked" with a button to load it one time for that message. There is no "always load" option in version 1.
- Inline images (`cid:`) come from the message itself, as `data:` URLs.
- Links do not open on click. Mail Sage shows the real address, flags a mismatch between the text and the address, removes known tracking parameters, and opens the default browser only after you confirm.
- Read receipt requests are never answered.
- Attachments never open in the preview. "Save as" only. Dangerous types (`.exe`, `.js`, `.lnk`, Office files with macros, and others) show a warning. Saved files get the Windows "from the internet" mark.
- A "plain text only" setting is available for each account.
- Mail Sage has no telemetry. It contacts only your mail servers, the sign-in pages, and GitHub for updates.

## 5. Block sender and block domain

- Right-click a message: "Block sender" or "Block domain".
- Mail from a blocked address or domain goes to that account's Junk folder on the server and does not show in the Inbox.
- Settings has a block list page to view and remove entries.
- The match uses the real `From` address, not the display name.

## 6. Address book

- Contacts: name, email addresses, notes. One address book for all accounts.
- "Create contacts from emails" scans the `From`, `To`, and `Cc` headers of synced mail. It shows a list with a count of messages for each person. You select which ones to add.
- It leaves out no-reply addresses, blocked senders, and mailing lists by default.
- The compose window completes addresses from the address book.

## 7. Compose and send

- New, reply, reply all, forward. Plain text and simple rich text.
- Attachments, drafts (saved to the server's Drafts folder), and a signature for each account.
- Sent mail goes to the account's Sent folder.

## 8. Mail actions and sync

- Mark read or unread, flag, move, delete, archive. Every action changes the server too.
- Sync uses IMAP IDLE for new mail, with a timed check as a backup.
- Full-text search uses SQLite FTS5 on the local store.
- Windows notification for new mail. The notification shows sender and subject only.

## 9. Updates and release

Same as Kaleido Sage:

- Copy `src-tauri/installer.nsi` from IconSage before the first build (global rule 4), with the Kaleido Sage change for passive mode.
- Copy and adapt `updater.rs`, the About dialog update flow, `set-version.ps1`, `publish.ps1`, `kill-dev.ps1`, `start-dev.vbs`, and `docs/RELEASING.md`.
- The updater checks the latest GitHub release of `rmirabelle/mailsage`, downloads the setup file, exits, and runs setup with `/P /R`.
- Per-user install. Single-instance plugin. No console window.

## 10. Order of work

1. Project skeleton: Tauri app, installer, updater, dev scripts, empty three-pane window.
2. Storage and one account: SQLite store, Credential Manager, Gmail OAuth, folder list, message list.
3. Secure preview, with a test set of hostile emails.
4. Other accounts: Microsoft OAuth (Outlook.com, Hotmail), generic IMAP, the work account.
5. Mail actions, sync with IDLE, search, notifications.
6. Compose and send.
7. Block sender and block domain.
8. Address book and contact scan.
9. Security review, then the first release.

Each step ends with tests (`cargo test`, `npm test`) and a build that runs.

## 11. Not in version 1

- Calendar, tasks, and meeting invitations.
- Mail rules other than the block list.
- PGP and S/MIME.
- Import of Outlook `.pst` files.

## 12. Decisions

- The work account uses a specific company server. It uses the generic IMAP account type. Mail Sage refuses a connection that is not encrypted (TLS). If the server turns out to be Microsoft Exchange without IMAP, step 4 needs a new plan for that account.
- A calendar is not necessary.
