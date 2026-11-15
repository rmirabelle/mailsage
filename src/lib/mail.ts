import { invoke } from "@tauri-apps/api/core";

export type FolderRole = "inbox" | "flagged" | "drafts" | "sent" | "archive" | "all" | "junk" | "trash" | "other";

export type Folder = {
  id: number;
  account_id: number;
  path: string;
  name: string;
  depth: number;
  role: FolderRole;
  selectable: boolean;
  unread: number;
  total: number;
};

export type Account = {
  id: number;
  kind: string;
  email: string;
  folders: Folder[];
};

export type MessageHeader = {
  uid: number;
  subject: string;
  from_name: string;
  from_address: string;
  /** Seconds since 1970. */
  date: number;
  seen: boolean;
  flagged: boolean;
  size: number;
};

export const googleSignInReady = () => invoke<boolean>("google_sign_in_ready");
export const listAccounts = () => invoke<Account[]>("list_accounts");
/** Opens the Google sign-in in the browser. It resolves when the sign-in is complete. */
export const addGmailAccount = () => invoke<Omit<Account, "folders">>("add_gmail_account");
export const cancelSignIn = () => invoke<void>("cancel_sign_in");
export const removeAccount = (accountId: number) => invoke<void>("remove_account", { accountId });
export const syncFolders = (accountId: number) => invoke<Folder[]>("sync_folders", { accountId });
export const listMessages = (folderId: number) => invoke<MessageHeader[]>("list_messages", { folderId });
export const syncMessages = (folderId: number) => invoke<MessageHeader[]>("sync_messages", { folderId });

const DAY_SECONDS = 86400;

/** The time for a message of today, the weekday for the last week, and the date for older ones. */
export function formatMessageDate(seconds: number, now = new Date()): string {
  if (seconds <= 0) return "";
  const date = new Date(seconds * 1000);
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000;
  if (seconds >= startOfToday) return date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  if (seconds >= startOfToday - 6 * DAY_SECONDS) return date.toLocaleDateString(undefined, { weekday: "short" });
  const sameYear = date.getFullYear() === now.getFullYear();
  return date.toLocaleDateString(undefined, sameYear ? { month: "short", day: "numeric" } : { year: "numeric", month: "short", day: "numeric" });
}
