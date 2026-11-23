import { isTauri } from "@tauri-apps/api/core";
import { ArrowBendDoubleUpLeft, ArrowBendUpLeft, ArrowBendUpRight, ArrowsClockwise, Checks, Envelope, EnvelopeOpen, Eye, EyeSlash, FolderMinus, FolderSimple, PencilSimple, Prohibit, ShieldCheck, Star, Trash, Tray, X } from "@phosphor-icons/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AboutDialog } from "./components/AboutDialog";
import { AccountsPane } from "./components/AccountsPane";
import { AddAccountDialog } from "./components/AddAccountDialog";
import { BlockDialog } from "./components/BlockDialog";
import { ComposeDialog } from "./components/ComposeDialog";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { DeleteDialog } from "./components/DeleteDialog";
import { ContextMenu, type MenuItem } from "./components/ContextMenu";
import { MenuBar, type Menu } from "./components/MenuBar";
import { MessageList } from "./components/MessageList";
import { LabelDialog } from "./components/LabelDialog";
import { MoveDialog } from "./components/MoveDialog";
import { MessagePreview } from "./components/MessagePreview";
import { PaneDivider } from "./components/PaneDivider";
import { loadAccountLabels, saveAccountLabels, withAccountLabel, type AccountLabels } from "./lib/accountLabels";
import { folderKey, loadHiddenFolders, saveHiddenFolders } from "./lib/folders";
import { SettingsDialog, type SettingsCategory } from "./components/SettingsDialog";
import { TitleBar } from "./components/TitleBar";
import {
  deleteFolderMessages,
  deleteMessages,
  listAccounts,
  blockSender,
  deleteFolder,
  listMessages,
  messagesFromBlocked,
  moveMessagesTo,
  moveToInbox,
  isGmail,
  loadOlderMessages,
  markFolderSeen,
  olderFromSender,
  prefetchMessages,
  removeAccount,
  replyDraft,
  searchFolder,
  setMessagesFlagged,
  senderDomain,
  setMessagesSeen,
  syncFolders,
  syncMessages,
  type Account,
  type Draft,
  type Folder,
  type MessageHeader,
  type ReplyMode
} from "./lib/mail";
import { DEFAULT_PANE_WIDTHS, fitPaneWidths, loadPaneWidths, PANE_LIMITS, savePaneWidths, type PaneWidths } from "./lib/panes";
import { loadSettings, saveSettings, type Settings } from "./lib/settings";
import { checkForUpdate, getAppVersion, type UpdateInfo } from "./lib/updater";

/** How many of a folder's newest messages load into memory when the folder opens. */
const PREFETCH_AT_OPEN = 15;
/** How many of the newest messages of each inbox load into memory at start. */
const PREFETCH_AT_START = 5;
/** How many messages under and above the selected one load into memory. */
const PREFETCH_BELOW = 6;
const PREFETCH_ABOVE = 2;

/** "1 email", "25 emails". */
const emailCount = (count: number) => count === 1 ? "1 email" : `${count} emails`;

const EMPTY_DRAFT: Draft = { to: "", cc: "", subject: "", body: "", in_reply_to: null, references: null };
const COMPOSE_TITLES: Record<ReplyMode, string> = { reply: "Reply", reply_all: "Reply to all", forward: "Forward" };

type Compose = { accountId: number; title: string; draft: Draft };

/** The window: the title bar, the menu bar, then the three panes (accounts, message list, secure preview). */
export default function App() {
  const [version, setVersion] = useState("");
  const [startupUpdate, setStartupUpdate] = useState<UpdateInfo | null>(null);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  /** The settings category that shows when the settings open. */
  const [settingsCategory, setSettingsCategory] = useState<SettingsCategory>("emails");
  const openSettings = (category: SettingsCategory) => { setSettingsCategory(category); setSettingsOpen(true); };
  const [settings, setSettings] = useState<Settings>(loadSettings);
  const changeSettings = (next: Settings) => {
    setSettings(next);
    saveSettings(next);
  };
  const [addAccountOpen, setAddAccountOpen] = useState(false);
  const [accountToRemove, setAccountToRemove] = useState<Account | null>(null);
  const [compose, setCompose] = useState<Compose | null>(null);
  const [messageMenu, setMessageMenu] = useState<{ message: MessageHeader; position: { left: number; top: number } } | null>(null);
  const [folderMenu, setFolderMenu] = useState<{ folder: Folder; position: { left: number; top: number } } | null>(null);
  /** A folder that waits for the "delete all" answer of the user. */
  const [folderToEmpty, setFolderToEmpty] = useState<Folder | null>(null);
  /** The labels that the user gave to accounts in the accounts list. This is saved. */
  const [accountLabels, setAccountLabels] = useState<AccountLabels>(loadAccountLabels);
  const [accountMenu, setAccountMenu] = useState<{ account: Account; position: { left: number; top: number } } | null>(null);
  /** The account whose label the user changes. */
  const [accountToLabel, setAccountToLabel] = useState<Account | null>(null);
  const setAccountLabel = (account: Account, label: string) => {
    setAccountLabels((current) => {
      const next = withAccountLabel(current, account.id, label);
      saveAccountLabels(next);
      return next;
    });
  };
  /** The folder that the user wants to delete from the server. The user must confirm it first. */
  const [folderToDelete, setFolderToDelete] = useState<Folder | null>(null);
  /** The folders that the user hid in the accounts column. This is saved. The folders stay on the server. */
  const [hiddenFolders, setHiddenFolders] = useState<ReadonlySet<string>>(loadHiddenFolders);
  /** True while the hidden folders show also, so that the user can show one again. */
  const [showHidden, setShowHidden] = useState(false);
  const setFolderHidden = (folder: Folder, hidden: boolean) => {
    setHiddenFolders((current) => {
      const next = new Set(current);
      if (hidden) next.add(folderKey(folder)); else next.delete(folderKey(folder));
      saveHiddenFolders(next);
      return next;
    });
  };
  /** The sender address, or the domain with the at sign in front, that the user wants to block. The user must confirm it first. */
  const [entryToBlock, setEntryToBlock] = useState<string | null>(null);
  /** A message in the trash that waits for the user's "delete for good" answer. */
  const [messageToDestroy, setMessageToDestroy] = useState<MessageHeader | null>(null);
  /** A message that waits for the answer of the user, because its sender has older messages in the folder. */
  const [deleteQuestion, setDeleteQuestion] = useState<{ message: MessageHeader; older: MessageHeader[] } | null>(null);

  /** The column widths that the user set. The window shows them fitted to its size. */
  const [paneWidths, setPaneWidths] = useState<PaneWidths>(loadPaneWidths);
  const paneWidthsRef = useRef(paneWidths);
  paneWidthsRef.current = paneWidths;
  const panesRef = useRef<HTMLElement>(null);
  const [panesTotal, setPanesTotal] = useState(0);
  const shownWidths = fitPaneWidths(paneWidths, panesTotal);

  useEffect(() => {
    const panes = panesRef.current;
    if (!panes) return;
    const observer = new ResizeObserver(() => setPanesTotal(panes.clientWidth));
    observer.observe(panes);
    return () => observer.disconnect();
  }, []);

  const resizePanes = (next: PaneWidths) => setPaneWidths(fitPaneWidths(next, panesTotal));
  const savePanes = () => savePaneWidths(paneWidthsRef.current);
  const resetPane = (pane: keyof PaneWidths) => {
    const next = fitPaneWidths({ ...shownWidths, [pane]: DEFAULT_PANE_WIDTHS[pane] }, panesTotal);
    setPaneWidths(next);
    savePaneWidths(next);
  };

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
  /**
   * The oldest message that the list has with no gap to the newest one.
   * "Load older emails" continues from here. Search results are not counted,
   * because they can be much older, with messages between that the list
   * does not have.
   */
  const oldestListedUid = useRef<number | null>(null);
  const [hasOlder, setHasOlder] = useState(false);
  const [olderBusy, setOlderBusy] = useState(false);
  const [searchBusy, setSearchBusy] = useState(false);
  /** The number of the last server search. An answer for an older search is ignored. */
  const searchNumber = useRef(0);

  const selectedFolder = useMemo(
    () => accounts.flatMap((account) => account.folders).find((folder) => folder.id === selectedFolderId) ?? null,
    [accounts, selectedFolderId]
  );
  const selectedMessage = useMemo(() => messages.find((message) => message.uid === selectedUid) ?? null, [messages, selectedUid]);
  /**
   * The other selected messages (Shift or Ctrl with a click). The preview
   * shows only `selectedUid`. An action on the selection uses the two.
   */
  const [extraUids, setExtraUids] = useState<ReadonlySet<number>>(new Set());
  /** All selected messages, in the order of the list. With one selected message, this has that one. */
  const selection = useMemo(() => messages.filter((message) => message.uid === selectedUid || extraUids.has(message.uid)), [messages, selectedUid, extraUids]);
  /* A message that left the list, and the selection of a different folder, go out of the selection. */
  useEffect(() => {
    setExtraUids((current) => {
      if (current.size === 0) return current;
      const present = new Set(messages.map((message) => message.uid));
      const next = new Set([...current].filter((uid) => present.has(uid) && uid !== selectedUid));
      return next.size === current.size ? current : next;
    });
  }, [messages, selectedUid]);
  useEffect(() => setExtraUids(new Set()), [selectedFolderId]);
  /** The messages that the user wants to delete as a group. The user must confirm it first. */
  const [messagesToDelete, setMessagesToDelete] = useState<MessageHeader[] | null>(null);
  /** The messages that the user wants to move. The user selects the folder in a dialog. */
  const [messagesToMove, setMessagesToMove] = useState<MessageHeader[] | null>(null);
  /** The message list as it is now, for actions that must not wait for the next render. */
  const messagesRef = useRef(messages);
  messagesRef.current = messages;

  /* When the selection moves, the messages near it load into memory, so that the next preview shows at once. */
  useEffect(() => {
    if (selectedFolderId === null || selectedUid === null) return;
    const index = messagesRef.current.findIndex((message) => message.uid === selectedUid);
    if (index < 0) return;
    const near = messagesRef.current.slice(Math.max(0, index - PREFETCH_ABOVE), index + PREFETCH_BELOW + 1).filter((message) => message.uid !== selectedUid);
    if (near.length > 0) void prefetchMessages(selectedFolderId, near.map((message) => message.uid));
  }, [selectedFolderId, selectedUid]);

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

  const setAccountFolders = (accountId: number, folders: Folder[]) => {
    setAccounts((current) => current.map((account) => account.id === accountId ? { ...account, folders } : account));
  };

  /** Shows the stored messages of a folder at once, then reads the newest ones from the server. */
  const openFolder = useCallback(async (folder: Folder) => {
    shownFolderId.current = folder.id;
    setSelectedFolderId(folder.id);
    setSelectedUid(null);
    setMessagesError(null);
    setMessagesBusy(true);
    oldestListedUid.current = null;
    setHasOlder(false);
    setOlderBusy(false);
    searchNumber.current += 1;
    setSearchBusy(false);
    const current = () => shownFolderId.current === folder.id;
    try {
      const stored = await listMessages(folder.id);
      if (current()) setMessages(stored);
      const fresh = await syncMessages(folder.id);
      if (current()) {
        setMessages(fresh);
        oldestListedUid.current = fresh.length > 0 ? Math.min(...fresh.map((message) => message.uid)) : null;
        setHasOlder(fresh.length > 0 && fresh.length < folder.total);
        void prefetchMessages(folder.id, fresh.slice(0, PREFETCH_AT_OPEN).map((message) => message.uid));
      }
    } catch (error) {
      if (current()) setMessagesError(String(error));
    } finally {
      if (current()) setMessagesBusy(false);
    }
  }, []);

  /** Adds messages from the server to the list. The list stays in order of date, newest first, and has each message one time. */
  const addMessages = (added: MessageHeader[]) => {
    if (added.length === 0) return;
    setMessages((current) => {
      const addedUids = new Set(added.map((message) => message.uid));
      return [...current.filter((message) => !addedUids.has(message.uid)), ...added]
        .sort((first, second) => second.date - first.date || second.uid - first.uid);
    });
  };

  /** Reads the next older messages of the shown folder from the server, and adds them to the end of the list. */
  const loadOlder = useCallback(async () => {
    const folderId = shownFolderId.current;
    const beforeUid = oldestListedUid.current;
    if (folderId === null || beforeUid === null) return;
    setOlderBusy(true);
    setMessagesError(null);
    try {
      const older = await loadOlderMessages(folderId, beforeUid);
      if (shownFolderId.current !== folderId) return;
      addMessages(older);
      if (older.length > 0) oldestListedUid.current = Math.min(...older.map((message) => message.uid));
      setHasOlder(older.length > 0 && (oldestListedUid.current ?? 1) > 1);
    } catch (error) {
      if (shownFolderId.current === folderId) setMessagesError(String(error));
    } finally {
      if (shownFolderId.current === folderId) setOlderBusy(false);
    }
  }, []);

  /** Searches the full shown folder on the server, and adds the messages that match to the list. */
  const searchServer = useCallback(async (searchText: string) => {
    const folderId = shownFolderId.current;
    if (folderId === null) return;
    const number = ++searchNumber.current;
    const current = () => shownFolderId.current === folderId && searchNumber.current === number;
    setSearchBusy(true);
    try {
      const found = await searchFolder(folderId, searchText);
      if (current()) addMessages(found);
    } catch (error) {
      if (current()) setMessagesError(String(error));
    } finally {
      if (current()) setSearchBusy(false);
    }
  }, []);

  /** Reads an account's folder list from the server. Returns the folders, or null after an error. */
  const refreshFolders = useCallback(async (accountId: number): Promise<Folder[] | null> => {
    setAccountBusy(accountId, true);
    setAccountError(accountId, null);
    try {
      const folders = await syncFolders(accountId);
      setAccountFolders(accountId, folders);
      return folders;
    } catch (error) {
      setAccountError(accountId, String(error));
      return null;
    } finally {
      setAccountBusy(accountId, false);
    }
  }, []);

  /**
   * Reads the new mail of an account's inbox into the store, and loads the
   * newest few messages into memory. Then a click on the inbox shows the
   * list at once, and the first message opens at once. An error here shows
   * when the user opens the inbox.
   */
  const readInbox = useCallback(async (folders: Folder[]) => {
    const inbox = folders.find((folder) => folder.role === "inbox");
    if (!inbox) return;
    try {
      const fresh = await syncMessages(inbox.id);
      void prefetchMessages(inbox.id, fresh.slice(0, PREFETCH_AT_START).map((message) => message.uid));
    } catch {
      /* The user sees the error when the inbox opens. */
    }
  }, []);

  /* At start: one quiet update check, then the stored accounts, then a fresh folder list and the new inbox mail for each. */
  useEffect(() => {
    if (!isTauri()) return;
    void getAppVersion().then(setVersion).catch(() => undefined);
    void checkForUpdate().then(setStartupUpdate).catch(() => undefined);
    void listAccounts().then((stored) => {
      setAccounts(stored);
      for (const account of stored) {
        void refreshFolders(account.id).then((folders) => folders && readInbox(folders));
      }
    }).catch(() => undefined);
  }, [refreshFolders, readInbox]);

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

  /**
   * Marks a message as read or unread. The list changes at once; if the
   * server refuses, the list goes back and the error shows.
   */
  const markSeen = useCallback(async (folder: Folder, message: MessageHeader, seen: boolean) => {
    if (message.seen === seen) return;
    const setSeen = (value: boolean) => {
      if (shownFolderId.current !== folder.id) return;
      setMessages((current) => current.map((entry) => entry.uid === message.uid ? { ...entry, seen: value } : entry));
    };
    setSeen(seen);
    try {
      setAccountFolders(folder.account_id, await setMessagesSeen(folder.id, [message.uid], seen));
    } catch (error) {
      setSeen(!seen);
      if (shownFolderId.current === folder.id) setMessagesError(String(error));
    }
  }, []);

  /**
   * Puts the star on a message, or removes it. The list changes at once; if
   * the server refuses, the list goes back and the error shows. In the
   * folder of starred messages, a message without the star leaves the list.
   */
  const setStar = useCallback(async (folder: Folder, message: MessageHeader, flagged: boolean) => {
    if (message.flagged === flagged) return;
    setMessagesError(null);
    const before = messagesRef.current;
    if (shownFolderId.current === folder.id) {
      const leaves = folder.role === "flagged" && !flagged;
      setMessages(leaves ? before.filter((entry) => entry.uid !== message.uid) : before.map((entry) => entry.uid === message.uid ? { ...entry, flagged } : entry));
      if (leaves) setSelectedUid((selected) => selected === message.uid ? null : selected);
    }
    try {
      setAccountFolders(folder.account_id, await setMessagesFlagged(folder.id, [message.uid], flagged));
    } catch (error) {
      if (shownFolderId.current !== folder.id) return;
      setMessages(before);
      setMessagesError(String(error));
    }
  }, []);

  /**
   * Deletes a message, and with it the `others` (older messages from the
   * same sender). The messages leave the list at once and the message under
   * the deleted one becomes selected; the server does the delete after that.
   * If the server refuses, the messages come back to their places and the
   * error shows.
   */
  const deleteMessage = useCallback(async (folder: Folder, message: MessageHeader, others: MessageHeader[] = []) => {
    setMessagesError(null);
    const removed = [message, ...others];
    const removedUids = new Set(removed.map((entry) => entry.uid));
    const inList = shownFolderId.current === folder.id;
    if (inList) {
      const index = messagesRef.current.findIndex((entry) => entry.uid === message.uid);
      const next = messagesRef.current.filter((entry) => !removedUids.has(entry.uid));
      setMessages(next);
      setSelectedUid((selected) => selected !== null && removedUids.has(selected) ? (next[Math.min(Math.max(index, 0), next.length - 1)]?.uid ?? null) : selected);
    }
    try {
      setAccountFolders(folder.account_id, await deleteMessages(folder.id, [...removedUids]));
    } catch (error) {
      if (shownFolderId.current !== folder.id) return;
      setMessagesError(String(error));
      if (inList) {
        /* Back to their places: the list is in order of date, newest first. */
        setMessages((current) => {
          const present = new Set(current.map((entry) => entry.uid));
          return [...current, ...removed.filter((entry) => !present.has(entry.uid))]
            .sort((first, second) => second.date - first.date || second.uid - first.uid);
        });
      }
    }
  }, []);

  /**
   * Does a change to all messages of a folder (mark all as read, delete
   * all). The server does the work first, because the folder can hold more
   * messages than the list shows. Then the list shows the result.
   */
  const changeFolder = useCallback(async (folder: Folder, change: (folderId: number) => Promise<Folder[]>, afterwards: (current: MessageHeader[]) => MessageHeader[]) => {
    setAccountBusy(folder.account_id, true);
    setAccountError(folder.account_id, null);
    try {
      setAccountFolders(folder.account_id, await change(folder.id));
      if (shownFolderId.current === folder.id) {
        const next = afterwards(messagesRef.current);
        setMessages(next);
        setSelectedUid((selected) => next.some((message) => message.uid === selected) ? selected : null);
      }
    } catch (error) {
      setAccountError(folder.account_id, String(error));
    } finally {
      setAccountBusy(folder.account_id, false);
    }
  }, []);

  /** Deletes a folder on the server. When the list shows that folder, the list becomes empty. */
  const removeFolder = async (folder: Folder) => {
    setAccountBusy(folder.account_id, true);
    setAccountError(folder.account_id, null);
    try {
      setAccountFolders(folder.account_id, await deleteFolder(folder.id));
      setFolderHidden(folder, false);
      if (shownFolderId.current === folder.id) {
        shownFolderId.current = null;
        setSelectedFolderId(null);
        setMessages([]);
        setSelectedUid(null);
        setMessagesError(null);
      }
    } catch (error) {
      setAccountError(folder.account_id, String(error));
    } finally {
      setAccountBusy(folder.account_id, false);
    }
  };

  const markAllRead = (folder: Folder) => void changeFolder(folder, markFolderSeen, (current) => current.map((message) => message.seen ? message : { ...message, seen: true }));
  const deleteAll = (folder: Folder) => void changeFolder(folder, deleteFolderMessages, () => []);

  /** Moves messages to a different folder of the account. The server does the move, and then the messages leave the list. */
  const moveMessages = useCallback(async (folder: Folder, moved: MessageHeader[], target: Folder) => {
    setMessagesError(null);
    const uids = new Set(moved.map((message) => message.uid));
    try {
      setAccountFolders(folder.account_id, await moveMessagesTo(folder.id, [...uids], target.id));
      if (shownFolderId.current !== folder.id) return;
      setMessages(messagesRef.current.filter((message) => !uids.has(message.uid)));
      setSelectedUid((selected) => selected !== null && uids.has(selected) ? null : selected);
    } catch (error) {
      setMessagesError(String(error));
    }
  }, []);

  /** Marks a group of messages as read or as unread. The server does the change, and then the list shows it. */
  const markManySeen = useCallback(async (folder: Folder, marked: MessageHeader[], seen: boolean) => {
    setMessagesError(null);
    const uids = new Set(marked.filter((message) => message.seen !== seen).map((message) => message.uid));
    if (uids.size === 0) return;
    try {
      setAccountFolders(folder.account_id, await setMessagesSeen(folder.id, [...uids], seen));
      if (shownFolderId.current === folder.id) setMessages(messagesRef.current.map((message) => uids.has(message.uid) ? { ...message, seen } : message));
    } catch (error) {
      setMessagesError(String(error));
    }
  }, []);

  /** "Not junk": the server moves a message from the Junk folder to the inbox, and then the message leaves the list. */
  const notJunk = useCallback(async (folder: Folder, message: MessageHeader) => {
    setMessagesError(null);
    try {
      setAccountFolders(folder.account_id, await moveToInbox(folder.id, [message.uid]));
      if (shownFolderId.current !== folder.id) return;
      const index = messagesRef.current.findIndex((entry) => entry.uid === message.uid);
      const next = messagesRef.current.filter((entry) => entry.uid !== message.uid);
      setMessages(next);
      setSelectedUid((selected) => selected === message.uid ? (next[Math.min(Math.max(index, 0), next.length - 1)]?.uid ?? null) : selected);
    } catch (error) {
      setMessagesError(String(error));
    }
  }, []);

  /**
   * Blocks a sender address or a sender domain. With `deleteEmails`, the
   * messages from that sender in the list are deleted first (they move to
   * the trash). Then, in an inbox, the server moves the messages from that
   * sender that remain to the Junk folder, and they leave the list.
   */
  const block = useCallback(async (folder: Folder, entry: string, deleteEmails: boolean) => {
    setMessagesError(null);
    try {
      if (deleteEmails && shownFolderId.current === folder.id) {
        const uids = new Set(messagesFromBlocked(messagesRef.current, entry).map((message) => message.uid));
        if (uids.size > 0) {
          setAccountFolders(folder.account_id, await deleteMessages(folder.id, [...uids]));
          if (shownFolderId.current === folder.id) {
            setMessages(messagesRef.current.filter((message) => !uids.has(message.uid)));
            setSelectedUid((selected) => selected !== null && uids.has(selected) ? null : selected);
          }
        }
      }
      const outcome = await blockSender(entry, folder.id);
      setAccountFolders(folder.account_id, outcome.folders);
      if (shownFolderId.current !== folder.id || outcome.removed.length === 0) return;
      const removed = new Set(outcome.removed);
      const next = messagesRef.current.filter((message) => !removed.has(message.uid));
      setMessages(next);
      setSelectedUid((selected) => selected !== null && removed.has(selected) ? null : selected);
    } catch (error) {
      setMessagesError(String(error));
    }
  }, []);

  /**
   * Starts a delete. When the sender has older messages in the folder, the
   * user gets a question first, and can delete those also. A message in the
   * trash is deleted for good, so the user must always confirm it first.
   */
  const requestDelete = useCallback((message: MessageHeader) => {
    if (!selectedFolder) return;
    const older = olderFromSender(messagesRef.current, message);
    if (older.length > 0) setDeleteQuestion({ message, older });
    else if (selectedFolder.role === "trash") setMessageToDestroy(message); else void deleteMessage(selectedFolder, message);
  }, [selectedFolder, deleteMessage]);

  const startCompose = useCallback(() => {
    const accountId = selectedFolder?.account_id ?? accounts[0]?.id;
    if (accountId !== undefined) setCompose({ accountId, title: "New message", draft: EMPTY_DRAFT });
  }, [selectedFolder, accounts]);

  const startReply = useCallback(async (message: MessageHeader, mode: ReplyMode) => {
    if (!selectedFolder) return;
    const folder = selectedFolder;
    setMessagesError(null);
    try {
      const draft = await replyDraft(folder.id, message.uid, mode);
      setCompose({ accountId: folder.account_id, title: COMPOSE_TITLES[mode], draft });
    } catch (error) {
      setMessagesError(String(error));
    }
  }, [selectedFolder]);

  /* Keyboard: Delete removes the selected message; Ctrl+N starts a new message; Ctrl+comma opens the settings. Not while a dialog or a text field has the keys. */
  const dialogOpen = aboutOpen || settingsOpen || addAccountOpen || accountToRemove !== null || compose !== null || messageToDestroy !== null || deleteQuestion !== null || folderToEmpty !== null || folderToDelete !== null || entryToBlock !== null || accountToLabel !== null || messagesToDelete !== null || messagesToMove !== null;
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (dialogOpen) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, select, [contenteditable='true']")) return;
      if (event.key === "Delete" && selection.length > 1) {
        event.preventDefault();
        setMessagesToDelete(selection);
      } else if (event.key === "Delete" && selectedMessage) {
        event.preventDefault();
        requestDelete(selectedMessage);
      } else if (event.ctrlKey && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        startCompose();
      } else if (event.ctrlKey && !event.shiftKey && !event.altKey && event.key === ",") {
        event.preventDefault();
        openSettings("emails");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dialogOpen, selectedMessage, selection, requestDelete, startCompose]);

  const menus: Menu[] = [
    { id: "file", label: "File", items: [
      { label: "New message", shortcut: "Ctrl+N", disabled: accounts.length === 0, run: startCompose },
      { label: "Add account…", run: () => setAddAccountOpen(true) },
      { label: "Blocked senders…", run: () => openSettings("blocked") },
      { label: "Settings…", shortcut: "Ctrl+,", run: () => openSettings("emails") }
    ] },
    { id: "view", label: "View", items: [
      { label: showHidden ? "Do not show hidden folders" : "Show hidden folders", disabled: hiddenFolders.size === 0 && !showHidden, run: () => setShowHidden((current) => !current) }
    ] },
    { id: "help", label: "Help", items: [{ label: "About Mail Sage", run: () => setAboutOpen(true) }] }
  ];

  const messageMenuItems = (message: MessageHeader): MenuItem[] => {
    const folder = selectedFolder;
    if (!folder) return [];
    /* A right-click on one of many selected messages gives the actions for the full selection. */
    if (selection.length > 1 && selection.some((entry) => entry.uid === message.uid)) {
      const count = selection.length;
      return [
        { label: `Mark ${count} emails as read`, icon: <EnvelopeOpen size={15} />, disabled: selection.every((entry) => entry.seen), run: () => void markManySeen(folder, selection, true) },
        { label: `Mark ${count} emails as unread`, icon: <Envelope size={15} />, disabled: selection.every((entry) => !entry.seen), run: () => void markManySeen(folder, selection, false) },
        { label: `Move ${count} emails to folder…`, icon: <FolderSimple size={15} />, run: () => setMessagesToMove(selection) },
        { label: folder.role === "trash" ? `Delete ${count} emails for good…` : `Delete ${count} emails…`, icon: <Trash size={15} />, run: () => setMessagesToDelete(selection) }
      ];
    }
    const address = message.from_address.trim().toLowerCase();
    const domain = senderDomain(address);
    /* The block uses the real address of the sender, so a message with no address has no block items. */
    const blockItems: MenuItem[] = domain === "" ? [] : [
      { label: `Block sender ${address}`, icon: <Prohibit size={15} />, run: () => setEntryToBlock(address) },
      { label: `Block domain ${domain}`, icon: <Prohibit size={15} />, run: () => setEntryToBlock(domain) }
    ];
    const junkItems: MenuItem[] = folder.role === "junk"
      ? [{ label: "Not junk", icon: <Tray size={15} />, run: () => void notJunk(folder, message) }]
      : [];
    return [
      ...junkItems,
      message.seen
        ? { label: "Mark as unread", icon: <Envelope size={15} />, run: () => void markSeen(folder, message, false) }
        : { label: "Mark as read", icon: <EnvelopeOpen size={15} />, run: () => void markSeen(folder, message, true) },
      message.flagged
        ? { label: "Remove star", icon: <Star size={15} />, run: () => void setStar(folder, message, false) }
        : { label: "Add star", icon: <Star size={15} weight="fill" />, run: () => void setStar(folder, message, true) },
      { label: "Reply", icon: <ArrowBendUpLeft size={15} />, run: () => void startReply(message, "reply") },
      { label: "Reply to all", icon: <ArrowBendDoubleUpLeft size={15} />, run: () => void startReply(message, "reply_all") },
      { label: "Forward", icon: <ArrowBendUpRight size={15} />, run: () => void startReply(message, "forward") },
      { label: "Move to folder…", icon: <FolderSimple size={15} />, run: () => setMessagesToMove([message]) },
      { label: folder.role === "trash" ? "Delete for good" : "Delete", icon: <Trash size={15} />, run: () => requestDelete(message) },
      ...blockItems
    ];
  };

  const accountMenuItems = (account: Account): MenuItem[] => [
    { label: "Set label…", icon: <PencilSimple size={15} />, run: () => setAccountToLabel(account) },
    { label: "Refresh folders", icon: <ArrowsClockwise size={15} />, run: () => void refreshFolders(account.id) },
    { label: "Remove account…", icon: <X size={15} />, run: () => setAccountToRemove(account) }
  ];

  const folderMenuItems = (folder: Folder): MenuItem[] => {
    const items: MenuItem[] = folder.selectable ? [
      { label: "Mark all as read", icon: <Checks size={15} />, disabled: folder.unread === 0, run: () => markAllRead(folder) },
      { label: folder.role === "trash" ? "Empty Trash…" : "Delete all…", icon: <Trash size={15} />, disabled: folder.total === 0, run: () => setFolderToEmpty(folder) }
    ] : [];
    /* The inbox always shows. Each other folder can go out of the list; it stays on the server. */
    if (hiddenFolders.has(folderKey(folder))) items.push({ label: "Show folder", icon: <Eye size={15} />, run: () => setFolderHidden(folder, false) });
    else if (folder.role !== "inbox") items.push({ label: "Hide folder", icon: <EyeSlash size={15} />, run: () => setFolderHidden(folder, true) });
    /* Only a folder that the user made can be deleted. The well-known folders belong to the mail service. */
    if (folder.selectable && folder.role === "other") items.push({ label: "Delete folder…", icon: <FolderMinus size={15} />, run: () => setFolderToDelete(folder) });
    return items;
  };

  return (
    <div className="app">
      <TitleBar updateAvailable={startupUpdate !== null} onAbout={() => setAboutOpen(true)} />
      <MenuBar menus={menus} />
      <main
        className="panes"
        ref={panesRef}
        style={{ gridTemplateColumns: `${shownWidths.accounts}px ${shownWidths.list}px minmax(0, 1fr)` }}
      >
        <AccountsPane
          accounts={accounts}
          selectedFolderId={selectedFolderId}
          busyAccountIds={busyAccountIds}
          accountErrors={accountErrors}
          accountLabels={accountLabels}
          onAccountMenu={(account, position) => setAccountMenu({ account, position })}
          hiddenFolders={hiddenFolders}
          showHidden={showHidden}
          onSelectFolder={(folder) => void openFolder(folder)}
          onFolderMenu={(folder, position) => setFolderMenu({ folder, position })}
          onAddAccount={() => setAddAccountOpen(true)}
          onNewMessage={startCompose}
          onRefreshAccount={(account) => void refreshFolders(account.id)}
          onRemoveAccount={setAccountToRemove}
        />
        <MessageList
          folder={selectedFolder}
          messages={messages}
          busy={messagesBusy}
          error={messagesError}
          selectedUid={selectedUid}
          selectedUids={extraUids}
          menuUid={messageMenu?.message.uid ?? null}
          collapsePreviousDays={settings.collapsePreviousDays}
          hasOlder={hasOlder}
          olderBusy={olderBusy}
          searchBusy={searchBusy}
          onLoadOlder={() => void loadOlder()}
          onSearch={(searchText) => void searchServer(searchText)}
          onSelect={(message, mode) => {
            if (mode === "single" || selectedUid === null) {
              setExtraUids(new Set());
              setSelectedUid(message.uid);
            } else if (message.uid !== selectedUid) {
              setExtraUids((current) => {
                const next = new Set(current);
                if (!next.delete(message.uid)) next.add(message.uid);
                return next;
              });
            }
          }}
          onSelectRange={(range) => setExtraUids(new Set(range.map((message) => message.uid).filter((uid) => uid !== selectedUid)))}
          onUnstar={(message) => selectedFolder && void setStar(selectedFolder, message, false)}
          onMenu={(message, position) => setMessageMenu({ message, position })}
          onRefresh={() => selectedFolder && void openFolder(selectedFolder)}
        />
        {selectedFolder && selectedMessage ? (
          <MessagePreview
            folderId={selectedFolder.id}
            message={selectedMessage}
            inTrash={selectedFolder.role === "trash"}
            onOpened={() => void markSeen(selectedFolder, selectedMessage, true)}
            onMarkUnread={() => void markSeen(selectedFolder, selectedMessage, false)}
            onStar={(flagged) => void setStar(selectedFolder, selectedMessage, flagged)}
            onReply={(mode) => void startReply(selectedMessage, mode)}
            onDelete={() => requestDelete(selectedMessage)}
          />
        ) : (
          <section className="pane preview-pane" aria-label="Secure preview">
            <div className="pane-empty">
              <ShieldCheck size={30} />
              <p>Select a message to read it here.</p>
            </div>
          </section>
        )}
        <PaneDivider
          label="Resize the accounts column"
          position={shownWidths.accounts}
          width={shownWidths.accounts}
          min={PANE_LIMITS.accounts.min}
          max={PANE_LIMITS.accounts.max}
          onResize={(width) => resizePanes({ ...shownWidths, accounts: width })}
          onDone={savePanes}
          onReset={() => resetPane("accounts")}
        />
        <PaneDivider
          label="Resize the message list column"
          position={shownWidths.accounts + shownWidths.list}
          width={shownWidths.list}
          min={PANE_LIMITS.list.min}
          max={PANE_LIMITS.list.max}
          onResize={(width) => resizePanes({ ...shownWidths, list: width })}
          onDone={savePanes}
          onReset={() => resetPane("list")}
        />
      </main>
      {messageMenu && (
        <ContextMenu position={messageMenu.position} items={messageMenuItems(messageMenu.message)} onClose={() => setMessageMenu(null)} />
      )}
      {accountMenu && (
        <ContextMenu position={accountMenu.position} items={accountMenuItems(accountMenu.account)} onClose={() => setAccountMenu(null)} />
      )}
      {accountToLabel && (
        <LabelDialog
          email={accountToLabel.email}
          label={accountLabels[accountToLabel.id] ?? ""}
          onSave={(label) => { const account = accountToLabel; setAccountToLabel(null); setAccountLabel(account, label); }}
          onCancel={() => setAccountToLabel(null)}
        />
      )}
      {folderMenu && (
        <ContextMenu position={folderMenu.position} items={folderMenuItems(folderMenu.folder)} onClose={() => setFolderMenu(null)} />
      )}
      {folderToEmpty && (
        <ConfirmDialog
          title={folderToEmpty.role === "trash" ? "Empty Trash?" : `Delete all in ${folderToEmpty.name}?`}
          confirmLabel={folderToEmpty.role === "trash" ? "Empty Trash" : "Delete all"}
          onConfirm={() => { const folder = folderToEmpty; setFolderToEmpty(null); deleteAll(folder); }}
          onCancel={() => setFolderToEmpty(null)}
        >
          {folderToEmpty.role === "trash"
            ? `This deletes ${emailCount(folderToEmpty.total)} from the server for good. They cannot come back.`
            : `This moves ${emailCount(folderToEmpty.total)} to the trash. This includes the emails that the list does not show.`}
        </ConfirmDialog>
      )}
      {entryToBlock && selectedFolder && (
        <BlockDialog
          entry={entryToBlock}
          emailCount={messagesFromBlocked(messages, entryToBlock).length}
          inTrash={selectedFolder.role === "trash"}
          onConfirm={(deleteEmails) => { const entry = entryToBlock; setEntryToBlock(null); void block(selectedFolder, entry, deleteEmails); }}
          onCancel={() => setEntryToBlock(null)}
        />
      )}
      {folderToDelete && (
        <ConfirmDialog
          title={`Delete the folder ${folderToDelete.name}?`}
          confirmLabel="Delete folder"
          onConfirm={() => { const folder = folderToDelete; setFolderToDelete(null); void removeFolder(folder); }}
          onCancel={() => setFolderToDelete(null)}
        >
          {isGmail(accounts.find((account) => account.id === folderToDelete.account_id)?.kind)
            ? "This deletes the folder from Gmail. Gmail keeps its emails in All Mail."
            : `This deletes the folder and its ${emailCount(folderToDelete.total)} from the server for good. They cannot come back.`}
          {" "}To only take the folder out of the list, cancel and use "Hide folder".
        </ConfirmDialog>
      )}
      {messagesToDelete && selectedFolder && (
        <ConfirmDialog
          title={selectedFolder.role === "trash" ? `Delete ${emailCount(messagesToDelete.length)} for good?` : `Delete ${emailCount(messagesToDelete.length)}?`}
          confirmLabel="Delete"
          onConfirm={() => { const [first, ...others] = messagesToDelete; setMessagesToDelete(null); if (first) void deleteMessage(selectedFolder, first, others); }}
          onCancel={() => setMessagesToDelete(null)}
        >
          {selectedFolder.role === "trash"
            ? "This deletes the selected emails from the server, and they cannot come back."
            : "The selected emails move to the trash."}
        </ConfirmDialog>
      )}
      {messagesToMove && selectedFolder && (
        <MoveDialog
          count={messagesToMove.length}
          folders={(accounts.find((account) => account.id === selectedFolder.account_id)?.folders ?? []).filter((folder) => folder.selectable && folder.id !== selectedFolder.id)}
          onMove={(target) => { const moved = messagesToMove; setMessagesToMove(null); void moveMessages(selectedFolder, moved, target); }}
          onCancel={() => setMessagesToMove(null)}
        />
      )}
      {aboutOpen && (
        <AboutDialog version={version} initialUpdateInfo={startupUpdate} onClose={() => setAboutOpen(false)} />
      )}
      {settingsOpen && (
        <SettingsDialog initialCategory={settingsCategory} settings={settings} onChange={changeSettings} onClose={() => setSettingsOpen(false)} />
      )}
      {addAccountOpen && (
        <AddAccountDialog onClose={() => setAddAccountOpen(false)} onAdded={(accountId) => void accountAdded(accountId)} />
      )}
      {compose && (
        <ComposeDialog
          accounts={accounts}
          accountId={compose.accountId}
          title={compose.title}
          draft={compose.draft}
          onClose={() => setCompose(null)}
          onSent={() => setCompose(null)}
        />
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
      {deleteQuestion && selectedFolder && (
        <DeleteDialog
          subject={deleteQuestion.message.subject}
          olderCount={deleteQuestion.older.length}
          forGood={selectedFolder.role === "trash"}
          onConfirm={(deleteOlder) => {
            const { message, older } = deleteQuestion;
            setDeleteQuestion(null);
            void deleteMessage(selectedFolder, message, deleteOlder ? older : []);
          }}
          onCancel={() => setDeleteQuestion(null)}
        />
      )}
      {messageToDestroy && selectedFolder && (
        <ConfirmDialog
          title="Delete this message for good?"
          confirmLabel="Delete for good"
          onConfirm={() => { const message = messageToDestroy; setMessageToDestroy(null); void deleteMessage(selectedFolder, message); }}
          onCancel={() => setMessageToDestroy(null)}
        >
          "{messageToDestroy.subject || "(no subject)"}" is in the trash. This deletes it from the server, and it cannot come back.
        </ConfirmDialog>
      )}
    </div>
  );
}
