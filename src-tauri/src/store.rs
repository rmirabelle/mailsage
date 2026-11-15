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

const SCHEMA_VERSION: i64 = 1;

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
        if version == 0 {
            connection.execute_batch(SCHEMA)?;
            connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        } else if version != SCHEMA_VERSION {
            bail!("the mail store has version {version}, and this Mail Sage reads version {SCHEMA_VERSION}");
        }
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
            transaction.execute(
                "INSERT INTO messages (folder_id, uid, subject, from_name, from_address, date, seen, flagged, size)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (folder_id, uid) DO UPDATE SET
                   subject = excluded.subject, from_name = excluded.from_name, from_address = excluded.from_address,
                   date = excluded.date, seen = excluded.seen, flagged = excluded.flagged, size = excluded.size",
                params![folder_id, header.uid, header.subject, header.from_name, header.from_address, header.date, header.seen, header.flagged, header.size],
            )?;
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
