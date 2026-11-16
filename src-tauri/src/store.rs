/**
 * The local store: one SQLite file in the per-user app data folder. It holds
 * accounts, folders, and message headers. It never holds passwords or
 * sign-in tokens (see secrets.rs).
 */
use std::path::Path;
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

const SCHEMA_VERSION: i64 = 3;

/** Added in version 3: the block list (blocklist.rs). An entry is an address, or a domain with the at sign in front. */
const BLOCKED_SENDERS_SCHEMA: &str = "CREATE TABLE blocked_senders (entry TEXT PRIMARY KEY COLLATE NOCASE);";

/** Added in version 2: the senders whose remote images always load. */
const TRUSTED_SENDERS_SCHEMA: &str = "CREATE TABLE trusted_senders (address TEXT PRIMARY KEY COLLATE NOCASE);";

const SCHEMA: &str = "
CREATE TABLE accounts (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  email TEXT NOT NULL UNIQUE,
  imap_host TEXT NOT NULL,
  imap_port INTEGER NOT NULL
);
CREATE TABLE folders (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  depth INTEGER NOT NULL,
  role TEXT NOT NULL,
  selectable INTEGER NOT NULL,
  uid_validity INTEGER,
  unread INTEGER NOT NULL DEFAULT 0,
  total INTEGER NOT NULL DEFAULT 0,
  UNIQUE (account_id, path)
);
CREATE TABLE messages (
  id INTEGER PRIMARY KEY,
  folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
  uid INTEGER NOT NULL,
  subject TEXT NOT NULL,
  from_name TEXT NOT NULL,
  from_address TEXT NOT NULL,
  date INTEGER NOT NULL,
  seen INTEGER NOT NULL,
  flagged INTEGER NOT NULL,
  size INTEGER NOT NULL,
  UNIQUE (folder_id, uid)
);
CREATE INDEX messages_by_date ON messages (folder_id, date DESC);
";

/** Folders show in this order: the well-known ones first, then the others by path. */
const FOLDER_ORDER: &str = "CASE role
  WHEN 'inbox' THEN 0 WHEN 'flagged' THEN 1 WHEN 'drafts' THEN 2 WHEN 'sent' THEN 3
  WHEN 'archive' THEN 4 WHEN 'all' THEN 5 WHEN 'junk' THEN 6 WHEN 'trash' THEN 7 ELSE 8 END,
  path COLLATE NOCASE";

#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub id: i64,
    pub kind: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    /** The name the server uses, in the server's own encoding. Commands use this. */
    pub path: String,
    /** The readable name of the last part of the path. */
    pub name: String,
    pub depth: u32,
    pub role: String,
    pub selectable: bool,
    pub unread: u32,
    pub total: u32,
}

/** A folder as the server reports it. */
#[derive(Debug, Clone, PartialEq)]
pub struct FolderInfo {
    pub path: String,
    pub name: String,
    pub depth: u32,
    pub role: String,
    pub selectable: bool,
    pub unread: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MessageHeader {
    pub uid: u32,
    pub subject: String,
    pub from_name: String,
    pub from_address: String,
    /** Seconds since 1970, from the time the server received the message. */
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub size: u32,
}

pub struct Store {
    connection: Mutex<Connection>,
}

/** Stores one message header, or replaces the stored one with the same number. */
fn store_header(transaction: &rusqlite::Transaction<'_>, folder_id: i64, header: &MessageHeader) -> Result<()> {
    transaction.execute(
        "INSERT INTO messages (folder_id, uid, subject, from_name, from_address, date, seen, flagged, size)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (folder_id, uid) DO UPDATE SET
           subject = excluded.subject, from_name = excluded.from_name, from_address = excluded.from_address,
           date = excluded.date, seen = excluded.seen, flagged = excluded.flagged, size = excluded.size",
        params![folder_id, header.uid, header.subject, header.from_name, header.from_address, header.date, header.seen, header.flagged, header.size],
    )?;
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).with_context(|| format!("create {}", folder.display()))?;
        }
        Self::prepare(Connection::open(path).with_context(|| format!("open {}", path.display()))?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
            0 => connection.execute_batch(&format!("{SCHEMA}{TRUSTED_SENDERS_SCHEMA}{BLOCKED_SENDERS_SCHEMA}"))?,
            1 => connection.execute_batch(&format!("{TRUSTED_SENDERS_SCHEMA}{BLOCKED_SENDERS_SCHEMA}"))?,
            2 => connection.execute_batch(BLOCKED_SENDERS_SCHEMA)?,
            SCHEMA_VERSION => {}
            _ => bail!("the mail store has version {version}, and this Mail Sage reads version {SCHEMA_VERSION}"),
        }
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { connection: Mutex::new(connection) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let connection = self.lock();
        let mut statement = connection.prepare("SELECT id, kind, email, imap_host, imap_port FROM accounts ORDER BY id")?;
        let rows = statement.query_map([], account_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn account(&self, id: i64) -> Result<Account> {
        self.lock()
            .query_row("SELECT id, kind, email, imap_host, imap_port FROM accounts WHERE id = ?1", [id], account_from_row)
            .optional()?
            .context("the account does not exist")
    }

    pub fn add_account(&self, kind: &str, email: &str, imap_host: &str, imap_port: u16) -> Result<Account> {
        let connection = self.lock();
        let exists: bool = connection.query_row("SELECT EXISTS (SELECT 1 FROM accounts WHERE email = ?1 COLLATE NOCASE)", [email], |row| row.get(0))?;
        if exists {
            bail!("{email} is already in Mail Sage");
        }
        connection.execute(
            "INSERT INTO accounts (kind, email, imap_host, imap_port) VALUES (?1, ?2, ?3, ?4)",
            params![kind, email, imap_host, imap_port],
        )?;
        Ok(Account {
            id: connection.last_insert_rowid(),
            kind: kind.to_string(),
            email: email.to_string(),
            imap_host: imap_host.to_string(),
            imap_port,
        })
    }

    /** Deletes the account with its folders and messages. */
    pub fn delete_account(&self, id: i64) -> Result<()> {
        self.lock().execute("DELETE FROM accounts WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn folders(&self, account_id: i64) -> Result<Vec<Folder>> {
        let connection = self.lock();
        let mut statement = connection.prepare(&format!(
            "SELECT id, account_id, path, name, depth, role, selectable, unread, total FROM folders WHERE account_id = ?1 ORDER BY {FOLDER_ORDER}"
        ))?;
        let rows = statement.query_map([account_id], folder_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn folder(&self, id: i64) -> Result<Folder> {
        self.lock()
            .query_row(
                "SELECT id, account_id, path, name, depth, role, selectable, unread, total FROM folders WHERE id = ?1",
                [id],
                folder_from_row,
            )
            .optional()?
            .context("the folder does not exist")
    }

    /** Makes the stored folders of an account the same as the server's list. A folder that the server no longer has goes, with its messages. */
    pub fn replace_folders(&self, account_id: i64, folders: &[FolderInfo]) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let stored: Vec<String> = {
            let mut statement = transaction.prepare("SELECT path FROM folders WHERE account_id = ?1")?;
            let rows = statement.query_map([account_id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for path in stored.iter().filter(|path| !folders.iter().any(|folder| &folder.path == *path)) {
            transaction.execute("DELETE FROM folders WHERE account_id = ?1 AND path = ?2", params![account_id, path])?;
        }
        for folder in folders {
            transaction.execute(
                "INSERT INTO folders (account_id, path, name, depth, role, selectable, unread, total)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (account_id, path) DO UPDATE SET
                   name = excluded.name, depth = excluded.depth, role = excluded.role,
                   selectable = excluded.selectable, unread = excluded.unread, total = excluded.total",
                params![account_id, folder.path, folder.name, folder.depth, folder.role, folder.selectable, folder.unread, folder.total],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /**
     * Stores the newest messages of a folder, as fetched from the server.
     * `uid_validity` is the server's mark for the folder's numbering: when it
     * changes, all stored message numbers are void, so the folder is emptied
     * first. A stored message from `lowest_uid` up that is not in `headers`
     * was deleted on the server, so it goes.
     */
    pub fn save_messages(&self, folder_id: i64, uid_validity: Option<u32>, lowest_uid: u32, headers: &[MessageHeader]) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let stored_validity: Option<u32> = transaction.query_row("SELECT uid_validity FROM folders WHERE id = ?1", [folder_id], |row| row.get(0))?;
        if stored_validity != uid_validity {
            transaction.execute("DELETE FROM messages WHERE folder_id = ?1", [folder_id])?;
            transaction.execute("UPDATE folders SET uid_validity = ?2 WHERE id = ?1", params![folder_id, uid_validity])?;
        }
        if headers.is_empty() {
            transaction.execute("DELETE FROM messages WHERE folder_id = ?1", [folder_id])?;
        } else {
            let kept = headers.iter().map(|header| header.uid.to_string()).collect::<Vec<_>>().join(",");
            transaction.execute(
                &format!("DELETE FROM messages WHERE folder_id = ?1 AND uid >= ?2 AND uid NOT IN ({kept})"),
                params![folder_id, lowest_uid],
            )?;
        }
        for header in headers {
            store_header(&transaction, folder_id, header)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /**
     * Stores more messages of a folder (older ones, or the results of a
     * search), so that they can be opened, marked and deleted the same as
     * the newest ones. No stored message is removed.
     */
    pub fn add_messages(&self, folder_id: i64, headers: &[MessageHeader]) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        for header in headers {
            store_header(&transaction, folder_id, header)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /** True when the user said that remote images from this sender address always load. */
    pub fn is_sender_trusted(&self, address: &str) -> Result<bool> {
        Ok(self.lock().query_row("SELECT EXISTS (SELECT 1 FROM trusted_senders WHERE address = ?1)", [address.trim()], |row| row.get(0))?)
    }

    pub fn set_sender_trusted(&self, address: &str, trusted: bool) -> Result<()> {
        let address = address.trim();
        if address.is_empty() {
            bail!("this message has no sender address");
        }
        let statement = if trusted { "INSERT OR IGNORE INTO trusted_senders (address) VALUES (?1)" } else { "DELETE FROM trusted_senders WHERE address = ?1" };
        self.lock().execute(statement, [address])?;
        Ok(())
    }

    /** The block list, in order of the alphabet. */
    pub fn blocked(&self) -> Result<Vec<String>> {
        let connection = self.lock();
        let mut statement = connection.prepare("SELECT entry FROM blocked_senders ORDER BY entry COLLATE NOCASE")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /** Adds an entry to the block list, or removes it. The entry is clean already (blocklist::normalize). */
    pub fn set_blocked(&self, entry: &str, blocked: bool) -> Result<()> {
        let statement = if blocked { "INSERT OR IGNORE INTO blocked_senders (entry) VALUES (?1)" } else { "DELETE FROM blocked_senders WHERE entry = ?1" };
        self.lock().execute(statement, [entry])?;
        Ok(())
    }

    /** The server's numbering mark of a folder, as stored with its messages. */
    pub fn folder_uid_validity(&self, folder_id: i64) -> Result<Option<u32>> {
        self.lock()
            .query_row("SELECT uid_validity FROM folders WHERE id = ?1", [folder_id], |row| row.get(0))
            .optional()?
            .context("the folder does not exist")
    }

    /** The stored size in bytes of one message. */
    pub fn message_size(&self, folder_id: i64, uid: u32) -> Result<u32> {
        self.lock()
            .query_row("SELECT size FROM messages WHERE folder_id = ?1 AND uid = ?2", params![folder_id, uid], |row| row.get(0))
            .optional()?
            .context("the message is not in the list. Refresh the folder.")
    }

    /** The account's folder with a well-known use, for example "trash". */
    pub fn folder_with_role(&self, account_id: i64, role: &str) -> Result<Option<Folder>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT id, account_id, path, name, depth, role, selectable, unread, total FROM folders WHERE account_id = ?1 AND role = ?2 AND selectable ORDER BY id LIMIT 1",
                params![account_id, role],
                folder_from_row,
            )
            .optional()?)
    }

    /** Marks stored messages as read or as unread, and keeps the folder's unread count correct. */
    pub fn mark_seen(&self, folder_id: i64, uids: &[u32], seen: bool) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let mut changed = 0_i64;
        for uid in uids {
            changed += transaction.execute("UPDATE messages SET seen = ?3 WHERE folder_id = ?1 AND uid = ?2 AND seen != ?3", params![folder_id, uid, seen])? as i64;
        }
        let change = if seen { -changed } else { changed };
        transaction.execute("UPDATE folders SET unread = MAX(0, unread + ?2) WHERE id = ?1", params![folder_id, change])?;
        transaction.commit()?;
        Ok(())
    }

    /** Puts the star on stored messages, or removes it, after the server did the same. */
    pub fn mark_flagged(&self, folder_id: i64, uids: &[u32], flagged: bool) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        for uid in uids {
            transaction.execute("UPDATE messages SET flagged = ?3 WHERE folder_id = ?1 AND uid = ?2", params![folder_id, uid, flagged])?;
        }
        transaction.commit()?;
        Ok(())
    }

    /** Marks all stored messages of a folder as read, after the server did the same. The unread count becomes 0. */
    pub fn mark_all_seen(&self, folder_id: i64) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE messages SET seen = 1 WHERE folder_id = ?1", [folder_id])?;
        transaction.execute("UPDATE folders SET unread = 0 WHERE id = ?1", [folder_id])?;
        transaction.commit()?;
        Ok(())
    }

    /**
     * Removes all stored messages of a folder after the server moved or
     * deleted them. The folder counts become 0. `moved_to` is the folder
     * that the server moved them to; its counts go up by the same numbers.
     */
    pub fn remove_all_messages(&self, folder_id: i64, moved_to: Option<i64>) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let (total, unread): (i64, i64) = transaction.query_row("SELECT total, unread FROM folders WHERE id = ?1", [folder_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        transaction.execute("DELETE FROM messages WHERE folder_id = ?1", [folder_id])?;
        transaction.execute("UPDATE folders SET total = 0, unread = 0 WHERE id = ?1", [folder_id])?;
        if let Some(target) = moved_to {
            transaction.execute("UPDATE folders SET total = total + ?2, unread = unread + ?3 WHERE id = ?1", params![target, total, unread])?;
        }
        transaction.commit()?;
        Ok(())
    }

    /**
     * Removes stored messages after the server moved or deleted them, and
     * keeps the folder counts correct. `moved_to` is the folder that the
     * server moved them to; its counts go up by the same numbers.
     */
    pub fn remove_messages(&self, folder_id: i64, uids: &[u32], moved_to: Option<i64>) -> Result<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let mut removed = 0_i64;
        let mut unread = 0_i64;
        for uid in uids {
            let seen: Option<bool> = transaction.query_row("SELECT seen FROM messages WHERE folder_id = ?1 AND uid = ?2", params![folder_id, uid], |row| row.get(0)).optional()?;
            if let Some(seen) = seen {
                transaction.execute("DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2", params![folder_id, uid])?;
                removed += 1;
                unread += i64::from(!seen);
            }
        }
        transaction.execute("UPDATE folders SET total = MAX(0, total - ?2), unread = MAX(0, unread - ?3) WHERE id = ?1", params![folder_id, removed, unread])?;
        if let Some(target) = moved_to {
            transaction.execute("UPDATE folders SET total = total + ?2, unread = unread + ?3 WHERE id = ?1", params![target, removed, unread])?;
        }
        transaction.commit()?;
        Ok(())
    }

    /** The newest messages of a folder first. */
    pub fn messages(&self, folder_id: i64, limit: u32) -> Result<Vec<MessageHeader>> {
        let connection = self.lock();
        let mut statement = connection.prepare(
            "SELECT uid, subject, from_name, from_address, date, seen, flagged, size FROM messages
             WHERE folder_id = ?1 ORDER BY date DESC, uid DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![folder_id, limit], |row| {
            Ok(MessageHeader {
                uid: row.get(0)?,
                subject: row.get(1)?,
                from_name: row.get(2)?,
                from_address: row.get(3)?,
                date: row.get(4)?,
                seen: row.get(5)?,
                flagged: row.get(6)?,
                size: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn account_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account { id: row.get(0)?, kind: row.get(1)?, email: row.get(2)?, imap_host: row.get(3)?, imap_port: row.get(4)? })
}

fn folder_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        account_id: row.get(1)?,
        path: row.get(2)?,
        name: row.get(3)?,
        depth: row.get(4)?,
        role: row.get(5)?,
        selectable: row.get(6)?,
        unread: row.get(7)?,
        total: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str, role: &str) -> FolderInfo {
        FolderInfo { path: path.into(), name: path.into(), depth: 0, role: role.into(), selectable: true, unread: 1, total: 2 }
    }

    fn header(uid: u32, date: i64) -> MessageHeader {
        MessageHeader { uid, subject: format!("Subject {uid}"), from_name: "A".into(), from_address: "a@example.com".into(), date, seen: false, flagged: false, size: 10 }
    }

    #[test]
    fn refuses_the_same_address_twice() {
        let store = Store::open_in_memory().unwrap();
        store.add_account("gmail", "me@example.com", "imap.gmail.com", 993).unwrap();
        assert!(store.add_account("gmail", "ME@example.com", "imap.gmail.com", 993).is_err());
    }

    #[test]
    fn orders_folders_and_drops_the_ones_the_server_lost() {
        let store = Store::open_in_memory().unwrap();
        let account = store.add_account("gmail", "me@example.com", "imap.gmail.com", 993).unwrap();
        store.replace_folders(account.id, &[folder("Work", "other"), folder("Trash", "trash"), folder("INBOX", "inbox")]).unwrap();
        let names: Vec<String> = store.folders(account.id).unwrap().into_iter().map(|folder| folder.path).collect();
        assert_eq!(names, ["INBOX", "Trash", "Work"]);
        store.replace_folders(account.id, &[folder("INBOX", "inbox")]).unwrap();
        assert_eq!(store.folders(account.id).unwrap().len(), 1);
    }

    #[test]
    fn keeps_messages_in_step_with_the_server() {
        let store = Store::open_in_memory().unwrap();
        let account = store.add_account("gmail", "me@example.com", "imap.gmail.com", 993).unwrap();
        store.replace_folders(account.id, &[folder("INBOX", "inbox")]).unwrap();
        let inbox = store.folders(account.id).unwrap().remove(0).id;

        store.save_messages(inbox, Some(7), 1, &[header(1, 100), header(2, 200), header(3, 300)]).unwrap();
        let uids = |store: &Store| store.messages(inbox, 50).unwrap().into_iter().map(|message| message.uid).collect::<Vec<_>>();
        assert_eq!(uids(&store), [3, 2, 1]);

        /* Message 2 was deleted on the server; message 1 is older than the fetched range, so it stays. */
        store.save_messages(inbox, Some(7), 2, &[header(3, 300), header(4, 400)]).unwrap();
        assert_eq!(uids(&store), [4, 3, 1]);

        /* The server numbered the folder again: all the old messages go. */
        store.save_messages(inbox, Some(8), 1, &[header(1, 500)]).unwrap();
        assert_eq!(uids(&store), [1]);
    }

    #[test]
    fn puts_the_star_on_a_message_and_removes_it() {
        let store = Store::open_in_memory().unwrap();
        let account = store.add_account("gmail", "me@example.com", "imap.gmail.com", 993).unwrap();
        store.replace_folders(account.id, &[folder("INBOX", "inbox")]).unwrap();
        let inbox = store.folders(account.id).unwrap()[0].id;
        store.save_messages(inbox, Some(1), 1, &[header(1, 10), header(2, 20)]).unwrap();
        store.mark_flagged(inbox, &[2], true).unwrap();
        let flagged: Vec<(u32, bool)> = store.messages(inbox, 10).unwrap().iter().map(|message| (message.uid, message.flagged)).collect();
        assert_eq!(flagged, [(2, true), (1, false)]);
        store.mark_flagged(inbox, &[2], false).unwrap();
        assert!(store.messages(inbox, 10).unwrap().iter().all(|message| !message.flagged));
    }

    #[test]
    fn keeps_the_block_list() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.blocked().unwrap().is_empty());
        store.set_blocked("news@shop.example", true).unwrap();
        store.set_blocked("@ads.example", true).unwrap();
        store.set_blocked("news@shop.example", true).unwrap();
        assert_eq!(store.blocked().unwrap(), ["@ads.example", "news@shop.example"]);
        store.set_blocked("@ads.example", false).unwrap();
        assert_eq!(store.blocked().unwrap(), ["news@shop.example"]);
    }

    #[test]
    fn upgrades_a_version_2_store() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(&format!("{SCHEMA}{TRUSTED_SENDERS_SCHEMA}")).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        let store = Store::prepare(connection).unwrap();
        store.set_blocked("a@example.com", true).unwrap();
        assert_eq!(store.blocked().unwrap(), ["a@example.com"]);
    }

    #[test]
    fn remembers_trusted_senders_without_regard_to_letter_case() {
        let store = Store::open_in_memory().unwrap();
        assert!(!store.is_sender_trusted("news@shop.example").unwrap());
        store.set_sender_trusted("News@Shop.example", true).unwrap();
        store.set_sender_trusted("news@shop.example", true).unwrap();
        assert!(store.is_sender_trusted("news@shop.example").unwrap());
        assert!(!store.is_sender_trusted("other@shop.example").unwrap());
        store.set_sender_trusted("NEWS@shop.example", false).unwrap();
        assert!(!store.is_sender_trusted("news@shop.example").unwrap());
        assert!(store.set_sender_trusted("  ", true).is_err());
    }

    #[test]
    fn upgrades_a_version_1_store() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        let store = Store::prepare(connection).unwrap();
        store.set_sender_trusted("a@example.com", true).unwrap();
        assert!(store.is_sender_trusted("a@example.com").unwrap());
    }

    #[test]
    fn marks_messages_and_moves_them_with_correct_counts() {
        let store = Store::open_in_memory().unwrap();
        let account = store.add_account("gmail", "me@example.com", "imap.gmail.com", 993).unwrap();
        let mut inbox = folder("INBOX", "inbox");
        inbox.unread = 3;
        inbox.total = 3;
        let mut trash = folder("Trash", "trash");
        trash.unread = 0;
        trash.total = 5;
        store.replace_folders(account.id, &[inbox, trash]).unwrap();
        let inbox = store.folder_with_role(account.id, "inbox").unwrap().unwrap().id;
        let trash = store.folder_with_role(account.id, "trash").unwrap().unwrap().id;
        assert!(store.folder_with_role(account.id, "junk").unwrap().is_none());
        store.save_messages(inbox, Some(1), 1, &[header(1, 100), header(2, 200), header(3, 300)]).unwrap();

        store.mark_seen(inbox, &[1, 2], true).unwrap();
        store.mark_seen(inbox, &[2], true).unwrap();
        assert_eq!(store.folder(inbox).unwrap().unread, 1);
        store.mark_seen(inbox, &[2], false).unwrap();
        assert_eq!(store.folder(inbox).unwrap().unread, 2);
        assert!(!store.messages(inbox, 50).unwrap().iter().find(|message| message.uid == 2).unwrap().seen);

        store.remove_messages(inbox, &[1, 2, 99], Some(trash)).unwrap();
        let (inbox, trash) = (store.folder(inbox).unwrap(), store.folder(trash).unwrap());
        assert_eq!((inbox.total, inbox.unread), (1, 1));
        assert_eq!((trash.total, trash.unread), (7, 1));
    }

    #[test]
    fn deletes_an_account_with_its_folders_and_messages() {
        let store = Store::open_in_memory().unwrap();
        let account = store.add_account("gmail", "me@example.com", "imap.gmail.com", 993).unwrap();
        store.replace_folders(account.id, &[folder("INBOX", "inbox")]).unwrap();
        let inbox = store.folders(account.id).unwrap().remove(0).id;
        store.save_messages(inbox, Some(1), 1, &[header(1, 100)]).unwrap();
        store.delete_account(account.id).unwrap();
        assert!(store.accounts().unwrap().is_empty());
        assert!(store.messages(inbox, 50).unwrap().is_empty());
    }
}
