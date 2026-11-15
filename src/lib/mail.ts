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

export type AttachmentInfo = {
  index: number;
  name: string;
  mime: string;
  size: number;
  /** True for a file type that can run code or hold macros. */
  risky: boolean;
};

export type MessageView = {
  subject: string;
  from_name: string;
  from_address: string;
  to: string;
  cc: string;
  /** The full, cleaned page for the preview frame. */
  html: string;
  /** Remote images that are not shown. The user can ask for these. */
  blocked_images: number;
  /** Tracking pixel images that Mail Sage removed. These never load. */
  blocked_trackers: number;
  /** True when images from this sender always load. */
  sender_trusted: boolean;
  remote_loaded: boolean;
  attachments: AttachmentInfo[];
};

export type LinkCheck = {
  /** The address that opens, with tracking parameters removed. */
  url: string;
  host: string;
  tracking_removed: boolean;
  text_mismatch: boolean;
  lookalike_risk: boolean;
  had_user_part: boolean;
  not_encrypted: boolean;
};

export const openMessage = (folderId: number, uid: number, loadRemote: boolean) => invoke<MessageView>("open_message", { folderId, uid, loadRemote });
/** Opens a "Save as" dialog. Resolves with the saved path, or null when the user cancelled. */
export const saveAttachment = (folderId: number, uid: number, index: number) => invoke<string | null>("save_attachment", { folderId, uid, index });
export const setSenderTrusted = (address: string, trusted: boolean) => invoke<void>("set_sender_trusted", { address, trusted });
export const checkLink = (href: string, text: string) => invoke<LinkCheck>("check_link", { href, text });
export const openLink = (url: string) => invoke<void>("open_link", { url });

export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
