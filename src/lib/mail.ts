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
/** True when this build can sign in to Microsoft: with `work`, to a work or school account (Microsoft 365); without it, to a personal account. */
export const microsoftSignInReady = (work: boolean) => invoke<boolean>("microsoft_sign_in_ready", { work });
/** Opens the Microsoft sign-in in the browser. It resolves when the sign-in is complete. */
export const addMicrosoftAccount = (work: boolean) => invoke<Omit<Account, "folders">>("add_microsoft_account", { work });
export const listAccounts = () => invoke<Account[]>("list_accounts");
/** Opens the Google sign-in in the browser. It resolves when the sign-in is complete. */
export const addGmailAccount = () => invoke<Omit<Account, "folders">>("add_gmail_account");
/** Tests a Gmail app password on Google's server and adds the account, or changes a Gmail account with the same address to it. */
export const addGmailAppPassword = (email: string, appPassword: string) => invoke<Omit<Account, "folders">>("add_gmail_app_password", { email, appPassword });
/** True for a Gmail account, with the Google sign-in or with an app password. */
export const isGmail = (kind: string | undefined) => kind === "gmail" || kind === "gmail-password";
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

/** Puts the star on messages, or removes it. Resolves with the account's folders and their new counts. */
export const setMessagesFlagged = (folderId: number, uids: number[], flagged: boolean) => invoke<Folder[]>("set_messages_flagged", { folderId, uids, flagged });
/** Moves messages to a different folder of the same account. Resolves with the account's folders and their new counts. */
export const moveMessagesTo = (folderId: number, uids: number[], targetId: number) => invoke<Folder[]>("move_messages_to", { folderId, uids, targetId });
/** Deletes a folder that the user made, on the server. Resolves with the account's folders after that. */
export const deleteFolder = (folderId: number) => invoke<Folder[]>("delete_folder", { folderId });
/** Moves messages from the Junk folder to the inbox ("Not junk"). Resolves with the account's folders and their new counts. */
export const moveToInbox = (folderId: number, uids: number[]) => invoke<Folder[]>("move_to_inbox", { folderId, uids });
/** The block list: addresses, and domains with the at sign in front. */
export const listBlocked = () => invoke<string[]>("list_blocked");
/**
 * Blocks a sender address (`ann@example.com`) or a sender domain (`@example.com`).
 * When the folder is an inbox, its messages from that sender move to the Junk
 * folder at once; `removed` has their numbers.
 */
export const blockSender = (entry: string, folderId: number) => invoke<{ folders: Folder[]; removed: number[] }>("block_sender", { entry, folderId });
/** Adds an entry to the block list from the settings. Resolves with the entry in its clean form. */
export const addBlocked = (entry: string) => invoke<string>("add_blocked", { entry });
export const unblockSender = (entry: string) => invoke<void>("unblock_sender", { entry });

/** The domain of an address with the at sign in front (`@example.com`), or an empty text when the address has no domain. */
export function senderDomain(address: string): string {
  const at = address.lastIndexOf("@");
  return at >= 0 && at < address.length - 1 ? address.slice(at).toLowerCase() : "";
}

/** The messages from a block list entry: a sender address, or a domain with the at sign in front. */
export function messagesFromBlocked(messages: MessageHeader[], entry: string): MessageHeader[] {
  const wanted = entry.trim().toLowerCase();
  return messages.filter((message) => {
    const address = message.from_address.trim().toLowerCase();
    return wanted.startsWith("@") ? senderDomain(address) === wanted : address === wanted;
  });
}

/**
 * The number of addresses in a list such as `Ann <ann@example.com>, bob@example.com`.
 * A comma or a semicolon that is in quotes or in angle brackets does not divide the list.
 */
export function countAddresses(list: string): number {
  let count = 0;
  let current = "";
  let inQuotes = false;
  let inBrackets = false;
  for (const character of `${list},`) {
    if (character === '"') inQuotes = !inQuotes;
    else if (character === "<" && !inQuotes) inBrackets = true;
    else if (character === ">" && !inQuotes) inBrackets = false;
    else if ((character === "," || character === ";") && !inQuotes && !inBrackets) {
      if (current.trim() !== "") count += 1;
      current = "";
      continue;
    }
    current += character;
  }
  return count;
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
