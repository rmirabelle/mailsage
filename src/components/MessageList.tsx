import { ArrowsClockwise, CircleNotch, EnvelopeSimple, Star, WarningCircle } from "@phosphor-icons/react";
import { formatMessageDate, type Folder, type MessageHeader } from "../lib/mail";

interface Props {
  folder: Folder | null;
  messages: MessageHeader[];
  /** True while the folder's messages are read from the server. */
  busy: boolean;
  error: string | null;
  selectedUid: number | null;
  onSelect: (message: MessageHeader) => void;
  onRefresh: () => void;
}

/** The middle pane: the newest messages of the selected folder. */
export function MessageList({ folder, messages, busy, error, selectedUid, onSelect, onRefresh }: Props) {
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
      {messages.length === 0 ? (
        <div className="pane-empty">
          <EnvelopeSimple size={30} />
          <p>{busy ? "Reading messages…" : "This folder has no messages."}</p>
        </div>
      ) : (
        <div className="message-list" role="listbox" aria-label={`Messages in ${folder.name}`}>
          {messages.map((message) => (
            <button
              key={message.uid}
              role="option"
              aria-selected={message.uid === selectedUid}
              className={`message-row ${message.seen ? "" : "unread"} ${message.uid === selectedUid ? "selected" : ""}`}
              onClick={() => onSelect(message)}
            >
              <span className="message-sender">{message.from_name || message.from_address || "(no sender)"}</span>
              <span className="message-date">{formatMessageDate(message.date)}</span>
              <span className="message-subject">{message.subject || "(no subject)"}</span>
              {message.flagged && <Star className="message-flag" size={13} weight="fill" />}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
