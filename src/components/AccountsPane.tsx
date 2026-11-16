import {
  ArrowsClockwise,
  CaretDown,
  CaretRight,
  CircleNotch,
  File,
  Folder as FolderIcon,
  NotePencil,
  PaperPlaneTilt,
  Plus,
  Star,
  Trash,
  Tray,
  TrayArrowDown,
  WarningOctagon,
  X
} from "@phosphor-icons/react";
import { useState, type ReactNode } from "react";
import type { AccountLabels } from "../lib/accountLabels";
import { folderKey, folderRows, loadClosedFolders, saveClosedFolders } from "../lib/folders";
import type { Account, Folder, FolderRole } from "../lib/mail";

interface Props {
  accounts: Account[];
  selectedFolderId: number | null;
  /** The accounts whose folder list is being read from the server. */
  busyAccountIds: ReadonlySet<number>;
  /** The last folder-list error of each account. */
  accountErrors: ReadonlyMap<number, string>;
  /** The labels that the user gave to accounts. An account with no label shows its email address. */
  accountLabels: AccountLabels;
  /** A right-click on an account: open its menu at the mouse. */
  onAccountMenu: (account: Account, position: { left: number; top: number }) => void;
  /** The folders that the user hid (keys from `folderKey`). */
  hiddenFolders: ReadonlySet<string>;
  /** True while the hidden folders show also, in a dim colour. */
  showHidden: boolean;
  onSelectFolder: (folder: Folder) => void;
  /** A right-click on a folder: open its menu at the mouse. */
  onFolderMenu: (folder: Folder, position: { left: number; top: number }) => void;
  onAddAccount: () => void;
  onNewMessage: () => void;
  onRefreshAccount: (account: Account) => void;
  onRemoveAccount: (account: Account) => void;
}

const ROLE_ICONS: Record<FolderRole, ReactNode> = {
  inbox: <Tray size={15} />,
  flagged: <Star size={15} />,
  drafts: <File size={15} />,
  sent: <PaperPlaneTilt size={15} />,
  archive: <TrayArrowDown size={15} />,
  all: <TrayArrowDown size={15} />,
  junk: <WarningOctagon size={15} />,
  trash: <Trash size={15} />,
  other: <FolderIcon size={15} />
};

/** The left pane: one group for each account, with its folders and unread counts. */
export function AccountsPane({ accounts, selectedFolderId, busyAccountIds, accountErrors, accountLabels, onAccountMenu, hiddenFolders, showHidden, onSelectFolder, onFolderMenu, onAddAccount, onNewMessage, onRefreshAccount, onRemoveAccount }: Props) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<number>>(new Set());

  /** The folders that the user closed: the folders under them do not show. This is saved. */
  const [closedFolders, setClosedFolders] = useState<ReadonlySet<string>>(loadClosedFolders);
  const toggleFolder = (folder: Folder) => {
    setClosedFolders((current) => {
      const next = new Set(current);
      const key = folderKey(folder);
      if (!next.delete(key)) next.add(key);
      saveClosedFolders(next);
      return next;
    });
  };

  const toggle = (accountId: number) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (!next.delete(accountId)) next.add(accountId);
      return next;
    });
  };

  return (
    <aside className="pane accounts-pane" aria-label="Accounts">
      <div className="pane-heading-row">
        <h2 className="pane-heading">Accounts</h2>
        <span className="pane-heading-buttons">
          {accounts.length > 0 && (
            <button className="icon-button" onClick={onNewMessage} aria-label="New message" title="New message (Ctrl+N)">
              <NotePencil size={15} />
            </button>
          )}
          <button className="icon-button" onClick={onAddAccount} aria-label="Add account" title="Add account">
            <Plus size={15} />
          </button>
        </span>
      </div>
      {accounts.length === 0 ? (
        <div className="pane-empty">
          <Tray size={30} />
          <p>No accounts yet.</p>
          <button className="button primary" onClick={onAddAccount}><Plus size={14} weight="bold" /> Add account</button>
        </div>
      ) : (
        <div className="account-list">
          {accounts.map((account) => {
            const open = !collapsed.has(account.id);
            const busy = busyAccountIds.has(account.id);
            const error = accountErrors.get(account.id);
            return (
              <section className="account-group" key={account.id}>
                <header
                  className="account-header"
                  onContextMenu={(event) => {
                    event.preventDefault();
                    onAccountMenu(account, { left: event.clientX, top: event.clientY });
                  }}
                >
                  <button className="account-toggle" onClick={() => toggle(account.id)} aria-expanded={open} title={account.email}>
                    {open ? <CaretDown size={12} /> : <CaretRight size={12} />}
                    <span>{accountLabels[account.id] ?? account.email}</span>
                  </button>
                  {busy
                    ? <CircleNotch className="spin account-busy" size={14} />
                    : (
                      <>
                        <button className="icon-button account-action" onClick={() => onRefreshAccount(account)} aria-label={`Refresh ${account.email}`} title="Refresh folders">
                          <ArrowsClockwise size={14} />
                        </button>
                        <button className="icon-button account-action" onClick={() => onRemoveAccount(account)} aria-label={`Remove ${account.email}`} title="Remove account">
                          <X size={14} />
                        </button>
                      </>
                    )}
                </header>
                {open && error && <p className="account-error selectable">{error}</p>}
                {open && folderRows(account.folders, closedFolders, hiddenFolders, showHidden).map(({ folder, depth, hasChildren, closed, hidden }) => (
                  <div className="folder-entry" key={folder.id}>
                    {hasChildren && (
                      <button
                        className="folder-expand"
                        style={{ left: 7 + depth * 14 }}
                        onClick={() => toggleFolder(folder)}
                        aria-expanded={!closed}
                        aria-label={closed ? `Show the folders in ${folder.name}` : `Hide the folders in ${folder.name}`}
                      >
                        {closed ? <CaretRight size={11} weight="bold" /> : <CaretDown size={11} weight="bold" />}
                      </button>
                    )}
                    <button
                      className={`folder-row ${folder.id === selectedFolderId ? "selected" : ""} ${folder.selectable ? "" : "no-mail"} ${hidden ? "hidden-folder" : ""}`}
                      style={{ paddingLeft: 26 + depth * 14 }}
                      /* A folder that cannot hold messages only opens and closes. */
                      onClick={() => folder.selectable ? onSelectFolder(folder) : toggleFolder(folder)}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        onFolderMenu(folder, { left: event.clientX, top: event.clientY });
                      }}
                    >
                      {ROLE_ICONS[folder.role] ?? ROLE_ICONS.other}
                      <span className="folder-name">{folder.name}</span>
                      {folder.unread > 0 && <span className="folder-unread">{folder.unread}</span>}
                    </button>
                  </div>
                ))}
              </section>
            );
          })}
        </div>
      )}
    </aside>
  );
}
