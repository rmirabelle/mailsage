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
import type { Account, Folder, FolderRole } from "../lib/mail";

interface Props {
  accounts: Account[];
  selectedFolderId: number | null;
  /** The accounts whose folder list is being read from the server. */
  busyAccountIds: ReadonlySet<number>;
  /** The last folder-list error of each account. */
  accountErrors: ReadonlyMap<number, string>;
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
export function AccountsPane({ accounts, selectedFolderId, busyAccountIds, accountErrors, onSelectFolder, onFolderMenu, onAddAccount, onNewMessage, onRefreshAccount, onRemoveAccount }: Props) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<number>>(new Set());

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
                <header className="account-header">
                  <button className="account-toggle" onClick={() => toggle(account.id)} aria-expanded={open} title={account.email}>
                    {open ? <CaretDown size={12} /> : <CaretRight size={12} />}
                    <span>{account.email}</span>
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
                {open && account.folders.map((folder) => (
                  <button
                    key={folder.id}
                    className={`folder-row ${folder.id === selectedFolderId ? "selected" : ""}`}
                    style={{ paddingLeft: 26 + folder.depth * 14 }}
                    disabled={!folder.selectable}
                    onClick={() => onSelectFolder(folder)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      if (folder.selectable) onFolderMenu(folder, { left: event.clientX, top: event.clientY });
                    }}
                  >
                    {ROLE_ICONS[folder.role] ?? ROLE_ICONS.other}
                    <span className="folder-name">{folder.name}</span>
                    {folder.unread > 0 && <span className="folder-unread">{folder.unread}</span>}
                  </button>
                ))}
              </section>
            );
          })}
        </div>
      )}
    </aside>
  );
}
