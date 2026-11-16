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
/** Reads the next older messages of a folder from the server: those before the message `beforeUid`. Resolves with only the new ones. */
export const loadOlderMessages = (folderId: number, beforeUid: number) => invoke<MessageHeader[]>("load_older_messages", { folderId, beforeUid });
/** Searches the full folder on the server by sender and subject. Resolves with the newest messages that match. */
export const searchFolder = (folderId: number, search: string) => invoke<MessageHeader[]>("search_messages", { folderId, search });
export const syncMessages = (folderId: number) => invoke<MessageHeader[]>("sync_messages", { folderId });

/** Messages with the same key are from the same sender: the address, or the name when there is no address. */
export const senderKey = (message: MessageHeader) => (message.from_address || message.from_name).trim().toLowerCase();

/** The messages in the list that are from the same sender as `message` and are older than it. */
export function olderFromSender(messages: MessageHeader[], message: MessageHeader): MessageHeader[] {
  const key = senderKey(message);
  if (!key) return [];
  return messages.filter((other) => other.uid !== message.uid
    && senderKey(other) === key
    && (other.date < message.date || (other.date === message.date && other.uid < message.uid)));
}

/**
 * The messages that match the search text. A message matches when its
 * sender (name or address) or its subject contains each word of the text.
 * Capital letters make no difference. An empty text gives all messages.
 */
export function searchMessages(messages: MessageHeader[], text: string): MessageHeader[] {
  const words = text.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return messages;
  return messages.filter((message) => {
    const content = `${message.from_name}\n${message.from_address}\n${message.subject}`.toLowerCase();
    return words.every((word) => content.includes(word));
  });
}

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

const startOfDay = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();

/**
 * The heading of the group that a message is in: "Today", "Yesterday", the
 * weekday for the 5 days before that, then the number of weeks, then "Older".
 */
export function messageDayGroup(seconds: number, now = new Date()): string {
  if (seconds <= 0) return "Older";
  const date = new Date(seconds * 1000);
  /* Days are counted on the calendar, so a change of the clock for summer time makes no error. */
  const days = Math.round((startOfDay(now) - startOfDay(date)) / (DAY_SECONDS * 1000));
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days <= 6) return date.toLocaleDateString(undefined, { weekday: "long" });
  if (days <= 13) return "Last week";
  if (days <= 20) return "Two weeks ago";
  if (days <= 27) return "Three weeks ago";
  return "Older";
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
/** Loads messages into memory ahead of time, so that their preview shows at once. It never fails. */
export const prefetchMessages = (folderId: number, uids: number[]) => invoke<void>("prefetch_messages", { folderId, uids });
export const setSenderTrusted = (address: string, trusted: boolean) => invoke<void>("set_sender_trusted", { address, trusted });
export const checkLink = (href: string, text: string) => invoke<LinkCheck>("check_link", { href, text });
export const openLink = (url: string) => invoke<void>("open_link", { url });

export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** The starting content of the compose window. */
export type Draft = {
  to: string;
  cc: string;
  subject: string;
  body: string;
  in_reply_to: string | null;
  references: string | null;
};

export type ReplyMode = "reply" | "reply_all" | "forward";

export type PickedFile = { path: string; name: string; size: number };

export type Outgoing = Draft & { account_id: number; bcc: string; attachments: string[] };

/** Marks messages as read or unread on the server. Resolves with the account's folders and their new counts. */
export const setMessagesSeen = (folderId: number, uids: number[], seen: boolean) => invoke<Folder[]>("set_messages_seen", { folderId, uids, seen });
/** Moves messages to the trash; in the trash folder, deletes them for good. Resolves with the account's folders. */
export const deleteMessages = (folderId: number, uids: number[]) => invoke<Folder[]>("delete_messages", { folderId, uids });
/** Marks all messages of a folder as read on the server. Resolves with the account's folders. */
export const markFolderSeen = (folderId: number) => invoke<Folder[]>("mark_folder_seen", { folderId });
/** Moves all messages of a folder to the trash; for the trash folder, deletes them for good. Resolves with the account's folders. */
export const deleteFolderMessages = (folderId: number) => invoke<Folder[]>("delete_folder_messages", { folderId });
export const replyDraft = (folderId: number, uid: number, mode: ReplyMode) => invoke<Draft>("reply_draft", { folderId, uid, mode });
/** Opens the "Attach" dialog. Only files picked there can be sent. */
export const pickAttachments = () => invoke<PickedFile[]>("pick_attachments");
export const sendMessage = (outgoing: Outgoing) => invoke<void>("send_message", { outgoing });
