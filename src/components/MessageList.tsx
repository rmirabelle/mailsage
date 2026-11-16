import { ArrowsClockwise, CaretDown, CaretRight, CircleNotch, EnvelopeSimple, MagnifyingGlass, Star, WarningCircle, X } from "@phosphor-icons/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { formatMessageDate, messageDayGroup, searchMessages, senderKey, type Folder, type MessageHeader } from "../lib/mail";

interface Props {
  folder: Folder | null;
  messages: MessageHeader[];
  /** True while the folder's messages are read from the server. */
  busy: boolean;
  error: string | null;
  selectedUid: number | null;
  /** The other messages that are selected with the Shift key or the Ctrl key. The preview shows `selectedUid` only. */
  selectedUids: ReadonlySet<number>;
  /** The message whose right-click menu is open. */
  menuUid: number | null;
  /** The setting: only the most recent day group starts open. */
  collapsePreviousDays: boolean;
  /** True when the server has messages that are older than those in the list. */
  hasOlder: boolean;
  /** True while the older messages are read from the server. */
  olderBusy: boolean;
  /** True while the server does a search. */
  searchBusy: boolean;
  onLoadOlder: () => void;
  /** The user typed a search text: search the full folder on the server also. */
  onSearch: (text: string) => void;
  /** A click on a message. With "toggle" (the Ctrl key), the message goes into the selection or out of it. */
  onSelect: (message: MessageHeader, mode: "single" | "toggle") => void;
  /** A click with the Shift key: select these messages, from the selected one to the clicked one. */
  onSelectRange: (messages: MessageHeader[]) => void;
  /** A click on the star of a message: remove the star. */
  onUnstar: (message: MessageHeader) => void;
  /** A right-click on a message: open its menu at the mouse. */
  onMenu: (message: MessageHeader, position: { left: number; top: number }) => void;
  onRefresh: () => void;
}

/** The server search starts this long after the last key, and only for a text of this length or more. */
const SERVER_SEARCH_DELAY = 450;
const MIN_SERVER_SEARCH_LENGTH = 2;

/**
 * The middle pane: the newest messages of the selected folder.
 *
 * The list shows only the latest message of each sender. When the sender
 * has older messages in the list, the latest one shows a chevron; a click on
 * it opens a block under the message with those older messages. An older
 * message shows only there, and not at its own date.
 */
export function MessageList({ folder, messages, busy, error, selectedUid, selectedUids, menuUid, collapsePreviousDays, hasOlder, olderBusy, searchBusy, onLoadOlder, onSearch, onSelect, onSelectRange, onUnstar, onMenu, onRefresh }: Props) {
  /** The messages whose sender block is open. */
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(new Set());
  const folderId = folder?.id ?? null;
  /* Message numbers belong to one folder, so a different folder starts with all blocks closed. */
  useEffect(() => setExpanded(new Set()), [folderId]);

  /** The search text. While it is not empty, the list shows only the messages that match it. */
  const [search, setSearch] = useState("");
  useEffect(() => setSearch(""), [folderId]);
  const searching = search.trim() !== "";
  /**
   * The filter for unread messages. While it is on, the list shows only the
   * unread messages, and the selected one: a message becomes read when the
   * user opens it, and it must not go out of the list at that moment.
   */
  const [unreadOnly, setUnreadOnly] = useState(false);
  useEffect(() => setUnreadOnly(false), [folderId]);
  const shown = useMemo(() => {
    const found = searchMessages(messages, search);
    return unreadOnly ? found.filter((message) => !message.seen || message.uid === selectedUid) : found;
  }, [messages, search, unreadOnly, selectedUid]);
  /** True while the search or the unread filter makes the list shorter. */
  const narrowed = searching || unreadOnly;

  /**
   * The list holds only a part of the folder, so the server searches the
   * full folder also. That starts a short time after the last key, and only
   * for a text of 2 characters or more. The results come into `messages`.
   */
  const onSearchRef = useRef(onSearch);
  onSearchRef.current = onSearch;
  const searchText = search.trim();
  useEffect(() => {
    if (searchText.length < MIN_SERVER_SEARCH_LENGTH) return;
    const timer = window.setTimeout(() => onSearchRef.current(searchText), SERVER_SEARCH_DELAY);
    return () => window.clearTimeout(timer);
  }, [searchText, folderId]);

  const bySender = useMemo(() => {
    const groups = new Map<string, MessageHeader[]>();
    for (const message of shown) {
      const key = senderKey(message);
      if (!key) continue;
      const group = groups.get(key);
      if (group) group.push(message); else groups.set(key, [message]);
    }
    return groups;
  }, [shown]);

  /** The messages in groups by day, in the order of the list: "Today", "Yesterday", and so on. */
  const dayGroups = useMemo(() => {
    const now = new Date();
    const groups = new Map<string, MessageHeader[]>();
    for (const message of shown) {
      /* The list is in order of date, newest first, so the first message of a sender is the latest one. Only that one gets a place of its own. */
      const fromSender = bySender.get(senderKey(message));
      if (fromSender && fromSender[0].uid !== message.uid) continue;
      const label = messageDayGroup(message.date, now);
      const group = groups.get(label);
      if (group) group.push(message); else groups.set(label, [message]);
    }
    return [...groups].map(([label, grouped]) => ({ label, messages: grouped }));
  }, [shown, bySender]);

  /**
   * The day groups that the user opened or closed by hand: each one is the
   * opposite of its default. A different folder, a change of the setting,
   * or the start or end of a search starts with the defaults again.
   */
  const [toggledDays, setToggledDays] = useState<ReadonlySet<string>>(new Set());
  useEffect(() => setToggledDays(new Set()), [folderId, collapsePreviousDays, narrowed]);
  /** By default all groups are open, or with the setting, only the most recent one. A search or the unread filter opens all groups. */
  const closedByDefault = (index: number) => collapsePreviousDays && !narrowed && index > 0;
  const isDayClosed = (label: string, index: number) => closedByDefault(index) !== toggledDays.has(label);
  /** Opens all day groups, or closes all of them. */
  const setAllDays = (closed: boolean) => {
    setToggledDays(new Set(dayGroups.filter((_, index) => closedByDefault(index) !== closed).map((group) => group.label)));
  };
  const toggleDay = (label: string) => {
    setToggledDays((current) => {
      const next = new Set(current);
      if (!next.delete(label)) next.add(label);
      return next;
    });
  };

  const toggle = (uid: number) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(uid)) next.add(uid);
      return next;
    });
  };

  /** The messages that show now, from top to bottom: not those of a closed day, and not those of a closed sender block. */
  const visibleOrder = () => {
    const order: MessageHeader[] = [];
    dayGroups.forEach((group, index) => {
      if (isDayClosed(group.label, index)) return;
      for (const message of group.messages) {
        order.push(message);
        if (expanded.has(message.uid)) order.push(...(bySender.get(senderKey(message)) ?? []).slice(1));
      }
    });
    return order;
  };

  /** A click with Shift selects from the selected message to this one. A click with Ctrl adds or removes this one. */
  const select = (message: MessageHeader, event: { shiftKey: boolean; ctrlKey: boolean }) => {
    if (event.shiftKey && selectedUid !== null) {
      const order = visibleOrder();
      const from = order.findIndex((entry) => entry.uid === selectedUid);
      const to = order.findIndex((entry) => entry.uid === message.uid);
      if (from >= 0 && to >= 0) {
        onSelectRange(order.slice(Math.min(from, to), Math.max(from, to) + 1));
        return;
      }
    }
    onSelect(message, event.ctrlKey ? "toggle" : "single");
  };

  const row = (message: MessageHeader, nested: boolean) => (
    <button
      key={message.uid}
      role="option"
      aria-selected={message.uid === selectedUid || selectedUids.has(message.uid)}
      className={`message-row ${nested ? "nested" : ""} ${message.seen ? "" : "unread"} ${message.uid === selectedUid || selectedUids.has(message.uid) ? "selected" : ""} ${message.uid === menuUid ? "context" : ""}`}
      onClick={(event) => select(message, event)}
      onContextMenu={(event) => {
        event.preventDefault();
        onMenu(message, { left: event.clientX, top: event.clientY });
      }}
    >
      <span className="message-sender">{message.from_name || message.from_address || "(no sender)"}</span>
      <span className="message-date">{formatMessageDate(message.date)}</span>
      <span className="message-subject">{message.subject || "(no subject)"}</span>
      {message.flagged && (
        /* The row is a button, and a button cannot hold a second button. So the star is a span that takes the click before the row gets it. */
        <span
          className="message-flag"
          role="button"
          aria-label="Remove star"
          title="Remove star"
          onClick={(event) => {
            event.stopPropagation();
            onUnstar(message);
          }}
        >
          <Star size={13} weight="fill" />
        </span>
      )}
    </button>
  );

  if (!folder) {
    return (
      <section className="pane list-pane" aria-label="Messages">
        <div className="pane-empty">
          <EnvelopeSimple size={30} />
          <p>Select a folder to see its messages.</p>
        </div>
      </section>
    );
  }

  return (
    <section className="pane list-pane" aria-label="Messages">
      <div className="pane-heading-row">
        <h2 className="pane-heading">{folder.name}</h2>
        {busy
          ? <CircleNotch className="spin list-busy" size={14} />
          : (
            <button className="icon-button" onClick={onRefresh} aria-label="Refresh messages" title="Refresh messages">
              <ArrowsClockwise size={14} />
            </button>
          )}
      </div>
      {error && (
        <p className="list-error"><WarningCircle size={15} weight="fill" /> <span className="selectable">{error}</span></p>
      )}
      <div className="list-toolbar">
        <div className={`search-box ${searching ? "active" : ""}`}>
          <MagnifyingGlass size={14} />
          <input
            type="text"
            value={search}
            placeholder="Search sender or subject"
            aria-label="Search the messages of this folder by sender or subject"
            spellCheck={false}
            onChange={(event) => setSearch(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape" && search !== "") {
                event.stopPropagation();
                setSearch("");
              }
            }}
          />
          {search !== "" && (
            <button className="search-clear" onClick={() => setSearch("")} aria-label="Clear the search" title="Clear the search">
              <X size={12} weight="bold" />
            </button>
          )}
        </div>
        <button
          className={`filter-button ${unreadOnly ? "active" : ""}`}
          onClick={() => setUnreadOnly((current) => !current)}
          aria-pressed={unreadOnly}
          title={unreadOnly ? "Show all emails" : "Show only unread emails"}
        >
          <EnvelopeSimple size={14} weight={unreadOnly ? "fill" : "regular"} /> Unread
        </button>
      </div>
      {shown.length === 0 ? (
        <div className="pane-empty">
          {searching ? <MagnifyingGlass size={30} /> : <EnvelopeSimple size={30} />}
          <p>{searching
            ? (searchBusy ? "Searching the full folder…" : unreadOnly ? "No unread message matches the search." : "No message matches the search.")
            : busy ? "Reading messages…" : unreadOnly ? "No unread messages in the list." : "This folder has no messages."}</p>
          {!searching && unreadOnly && hasOlder && (
            <button className="list-more" disabled={olderBusy} onClick={onLoadOlder}>
              {olderBusy ? <><CircleNotch className="spin" size={13} /> Reading older emails…</> : "Load older emails"}
            </button>
          )}
        </div>
      ) : (
        <div className="message-list" role="listbox" aria-multiselectable="true" aria-label={`Messages in ${folder.name}`}>
          {dayGroups.map((group, index) => {
            const closed = isDayClosed(group.label, index);
            return (
              <div className="day-group" key={group.label} role="group" aria-label={group.label}>
                <button
                  className="day-heading"
                  /* Ctrl+click does to all groups what a click does to this one: it opens all of them, or closes all of them. */
                  onClick={(event) => event.ctrlKey ? setAllDays(!closed) : toggleDay(group.label)}
                  aria-expanded={!closed}
                  title="Ctrl+click opens or closes all days"
                >
                  {closed ? <CaretRight size={11} weight="bold" /> : <CaretDown size={11} weight="bold" />}
                  <span className="day-name">{group.label}</span>
                  <span className="day-count">{group.messages.length}</span>
                </button>
                {!closed && group.messages.map((message) => {
                  const others = (bySender.get(senderKey(message)) ?? []).slice(1);
                  const unreadOthers = others.filter((other) => !other.seen).length;
                  const open = others.length > 0 && expanded.has(message.uid);
                  return (
                    <div className="message-entry" key={message.uid}>
                      {others.length > 0 && (
                        <button
                          className={`message-expand ${unreadOthers > 0 ? "has-unread" : ""}`}
                          onClick={() => toggle(message.uid)}
                          aria-expanded={open}
                          aria-label={open ? "Hide the older messages from this sender" : "Show the older messages from this sender"}
                          title={`${others.length} older from this sender${unreadOthers > 0 ? `, ${unreadOthers} unread` : ""}`}
                        >
                          {open ? <CaretDown size={12} weight="bold" /> : <CaretRight size={12} weight="bold" />}
                        </button>
                      )}
                      {row(message, false)}
                      {open && (
                        <div className="sender-block" role="group" aria-label="Older messages from this sender">
                          {others.map((other) => row(other, true))}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            );
          })}
          {searching && searchBusy && (
            <p className="list-status"><CircleNotch className="spin" size={13} /> Searching the full folder…</p>
          )}
          {!searching && hasOlder && (
            <button className="list-more" disabled={olderBusy} onClick={onLoadOlder}>
              {olderBusy ? <><CircleNotch className="spin" size={13} /> Reading older emails…</> : "Load older emails"}
            </button>
          )}
        </div>
      )}
    </section>
  );
}
