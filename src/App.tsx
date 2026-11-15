import { isTauri } from "@tauri-apps/api/core";
import { ShieldCheck } from "@phosphor-icons/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AboutDialog } from "./components/AboutDialog";
import { AccountsPane } from "./components/AccountsPane";
import { AddAccountDialog } from "./components/AddAccountDialog";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { MessageList } from "./components/MessageList";
import { TitleBar } from "./components/TitleBar";
import { listAccounts, listMessages, removeAccount, syncFolders, syncMessages, type Account, type Folder, type MessageHeader } from "./lib/mail";
import { checkForUpdate, getAppVersion, type UpdateInfo } from "./lib/updater";

/** The window: the title bar, then the three panes (accounts, message list, secure preview). */
export default function App() {
  const [version, setVersion] = useState("");
  const [startupUpdate, setStartupUpdate] = useState<UpdateInfo | null>(null);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [addAccountOpen, setAddAccountOpen] = useState(false);
  const [accountToRemove, setAccountToRemove] = useState<Account | null>(null);

  const [accounts, setAccounts] = useState<Account[]>([]);
  const [busyAccountIds, setBusyAccountIds] = useState<ReadonlySet<number>>(new Set());
  const [accountErrors, setAccountErrors] = useState<ReadonlyMap<number, string>>(new Map());
  const [selectedFolderId, setSelectedFolderId] = useState<number | null>(null);
  const [messages, setMessages] = useState<MessageHeader[]>([]);
  const [messagesBusy, setMessagesBusy] = useState(false);
  const [messagesError, setMessagesError] = useState<string | null>(null);
  const [selectedUid, setSelectedUid] = useState<number | null>(null);
  /** The folder that the message list shows. An answer for a different folder is old, and is ignored. */
  const shownFolderId = useRef<number | null>(null);

  const selectedFolder = useMemo(
    () => accounts.flatMap((account) => account.folders).find((folder) => folder.id === selectedFolderId) ?? null,
    [accounts, selectedFolderId]
  );

  const setAccountBusy = (accountId: number, busy: boolean) => {
    setBusyAccountIds((current) => {
      const next = new Set(current);
      if (busy) next.add(accountId); else next.delete(accountId);
      return next;
    });
  };

  const setAccountError = (accountId: number, message: string | null) => {
    setAccountErrors((current) => {
      const next = new Map(current);
      if (message) next.set(accountId, message); else next.delete(accountId);
      return next;
    });
  };

  /** Shows the stored messages of a folder at once, then reads the newest ones from the server. */
  const openFolder = useCallback(async (folder: Folder) => {
    shownFolderId.current = folder.id;
    setSelectedFolderId(folder.id);
    setSelectedUid(null);
    setMessagesError(null);
    setMessagesBusy(true);
    const current = () => shownFolderId.current === folder.id;
    try {
      const stored = await listMessages(folder.id);
      if (current()) setMessages(stored);
      const fresh = await syncMessages(folder.id);
      if (current()) setMessages(fresh);
    } catch (error) {
      if (current()) setMessagesError(String(error));
    } finally {
      if (current()) setMessagesBusy(false);
    }
  }, []);

  /** Reads an account's folder list from the server. Returns the folders, or null after an error. */
  const refreshFolders = useCallback(async (accountId: number): Promise<Folder[] | null> => {
    setAccountBusy(accountId, true);
    setAccountError(accountId, null);
    try {
      const folders = await syncFolders(accountId);
      setAccounts((current) => current.map((account) => account.id === accountId ? { ...account, folders } : account));
      return folders;
    } catch (error) {
      setAccountError(accountId, String(error));
      return null;
    } finally {
      setAccountBusy(accountId, false);
    }
  }, []);

  /* At start: one quiet update check, then the stored accounts, then a fresh folder list for each. */
  useEffect(() => {
    if (!isTauri()) return;
    void getAppVersion().then(setVersion).catch(() => undefined);
    void checkForUpdate().then(setStartupUpdate).catch(() => undefined);
    void listAccounts().then((stored) => {
      setAccounts(stored);
      for (const account of stored) void refreshFolders(account.id);
    }).catch(() => undefined);
  }, [refreshFolders]);

  const accountAdded = async (accountId: number) => {
    setAddAccountOpen(false);
    setAccounts(await listAccounts());
    const folders = await refreshFolders(accountId);
    const inbox = folders?.find((folder) => folder.role === "inbox");
    if (inbox) void openFolder(inbox);
  };

  const confirmRemove = async () => {
    const account = accountToRemove;
    if (!account) return;
    setAccountToRemove(null);
    try {
      await removeAccount(account.id);
      if (selectedFolder?.account_id === account.id) {
        shownFolderId.current = null;
        setSelectedFolderId(null);
        setMessages([]);
        setMessagesError(null);
        setMessagesBusy(false);
      }
      setAccounts(await listAccounts());
    } catch (error) {
      setAccountError(account.id, String(error));
    }
  };

  return (
    <div className="app">
      <TitleBar updateAvailable={startupUpdate !== null} onAbout={() => setAboutOpen(true)} />
      <main className="panes">
        <AccountsPane
          accounts={accounts}
          selectedFolderId={selectedFolderId}
          busyAccountIds={busyAccountIds}
          accountErrors={accountErrors}
          onSelectFolder={(folder) => void openFolder(folder)}
          onAddAccount={() => setAddAccountOpen(true)}
          onRefreshAccount={(account) => void refreshFolders(account.id)}
          onRemoveAccount={setAccountToRemove}
        />
        <MessageList
          folder={selectedFolder}
          messages={messages}
          busy={messagesBusy}
          error={messagesError}
          selectedUid={selectedUid}
          onSelect={(message) => setSelectedUid(message.uid)}
          onRefresh={() => selectedFolder && void openFolder(selectedFolder)}
        />
        <section className="pane preview-pane" aria-label="Secure preview">
          <div className="pane-empty">
            <ShieldCheck size={30} />
            <p>{selectedUid === null ? "Select a message to read it here." : "The secure preview comes in the next step of the plan."}</p>
          </div>
        </section>
      </main>
      {aboutOpen && (
        <AboutDialog version={version} initialUpdateInfo={startupUpdate} onClose={() => setAboutOpen(false)} />
      )}
      {addAccountOpen && (
        <AddAccountDialog onClose={() => setAddAccountOpen(false)} onAdded={(accountId) => void accountAdded(accountId)} />
      )}
      {accountToRemove && (
        <ConfirmDialog
          title={`Remove ${accountToRemove.email}?`}
          confirmLabel="Remove account"
          onConfirm={() => void confirmRemove()}
          onCancel={() => setAccountToRemove(null)}
        >
          Mail Sage deletes its stored copy of this account's mail and its sign-in. The mail on the server stays.
        </ConfirmDialog>
      )}
    </div>
  );
}
