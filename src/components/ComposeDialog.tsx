import { CircleNotch, PaperPlaneTilt, Paperclip, WarningCircle, X } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { formatFileSize, pickAttachments, sendMessage, type Account, type Draft, type PickedFile } from "../lib/mail";
import { ConfirmDialog } from "./ConfirmDialog";

interface Props {
  accounts: Account[];
  /** The account that sends, at the start. */
  accountId: number;
  /** "New message", "Reply", or "Forward". */
  title: string;
  /** The starting content: empty for a new message, filled for a reply or a forward. */
  draft: Draft;
  onClose: () => void;
  onSent: () => void;
}

/** The window to write a message. Messages are plain text. */
export function ComposeDialog({ accounts, accountId, title, draft, onClose, onSent }: Props) {
  const [from, setFrom] = useState(accountId);
  const [to, setTo] = useState(draft.to);
  const [cc, setCc] = useState(draft.cc);
  const [bcc, setBcc] = useState("");
  const [showCopies, setShowCopies] = useState(draft.cc !== "");
  const [subject, setSubject] = useState(draft.subject);
  const [body, setBody] = useState(draft.body);
  const [files, setFiles] = useState<PickedFile[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const toRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  const changed = to !== draft.to || cc !== draft.cc || bcc !== "" || subject !== draft.subject || body !== draft.body || files.length > 0;

  /* A reply starts in the text, above the quoted message; a new message or a forward starts in the To line. */
  useEffect(() => {
    if (draft.to) {
      bodyRef.current?.focus();
      bodyRef.current?.setSelectionRange(0, 0);
      if (bodyRef.current) bodyRef.current.scrollTop = 0;
    } else {
      toRef.current?.focus();
    }
  }, [draft.to]);

  const close = () => {
    if (sending) return;
    if (changed) setConfirmDiscard(true); else onClose();
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !confirmDiscard) close();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const attach = async () => {
    setError(null);
    try {
      const picked = await pickAttachments();
      setFiles((current) => [...current, ...picked.filter((file) => !current.some((known) => known.path === file.path))]);
    } catch (problem) {
      setError(String(problem));
    }
  };

  const send = async () => {
    setSending(true);
    setError(null);
    try {
      await sendMessage({
        account_id: from,
        to,
        cc,
        bcc,
        subject,
        body,
        in_reply_to: draft.in_reply_to,
        references: draft.references,
        attachments: files.map((file) => file.path)
      });
      onSent();
    } catch (problem) {
      setError(String(problem));
      setSending(false);
    }
  };

  return (
    <div className="about-dialog-overlay" role="presentation">
      <div className="about-dialog compose-dialog" role="dialog" aria-modal="true" aria-labelledby="compose-title">
        <button className="about-dialog-close" onClick={close} aria-label="Close" title="Close" disabled={sending}>
          <X size={18} />
        </button>
        <div className="compose-body">
          <h2 id="compose-title">{title}</h2>
          <div className="compose-row">
            <label htmlFor="compose-from">From</label>
            <select id="compose-from" value={from} onChange={(event) => setFrom(Number(event.target.value))} disabled={sending}>
              {accounts.map((account) => <option key={account.id} value={account.id}>{account.email}</option>)}
            </select>
          </div>
          <div className="compose-row">
            <label htmlFor="compose-to">To</label>
            <input id="compose-to" ref={toRef} type="text" value={to} onChange={(event) => setTo(event.target.value)} disabled={sending} spellCheck={false} autoComplete="off" />
            {!showCopies && <button className="compose-toggle" onClick={() => setShowCopies(true)}>Cc Bcc</button>}
          </div>
          {showCopies && (
            <>
              <div className="compose-row">
                <label htmlFor="compose-cc">Cc</label>
                <input id="compose-cc" type="text" value={cc} onChange={(event) => setCc(event.target.value)} disabled={sending} spellCheck={false} autoComplete="off" />
              </div>
              <div className="compose-row">
                <label htmlFor="compose-bcc">Bcc</label>
                <input id="compose-bcc" type="text" value={bcc} onChange={(event) => setBcc(event.target.value)} disabled={sending} spellCheck={false} autoComplete="off" />
              </div>
            </>
          )}
          <div className="compose-row">
            <label htmlFor="compose-subject">Subject</label>
            <input id="compose-subject" type="text" value={subject} onChange={(event) => setSubject(event.target.value)} disabled={sending} autoComplete="off" />
          </div>
          <textarea ref={bodyRef} className="compose-text" aria-label="Message text" value={body} onChange={(event) => setBody(event.target.value)} disabled={sending} />
          {files.length > 0 && (
            <div className="compose-files">
              {files.map((file) => (
                <span className="compose-file" key={file.path} title={file.path}>
                  <Paperclip size={13} /> {file.name} <small>{formatFileSize(file.size)}</small>
                  <button className="icon-button" onClick={() => setFiles((current) => current.filter((known) => known.path !== file.path))} aria-label={`Remove ${file.name}`} title="Remove" disabled={sending}>
                    <X size={12} />
                  </button>
                </span>
              ))}
            </div>
          )}
          {error && <p className="compose-error"><WarningCircle size={15} weight="fill" /> <span className="selectable">{error}</span></p>}
        </div>
        <footer className="about-dialog-actions compose-actions">
          <button className="button secondary" onClick={() => void attach()} disabled={sending}><Paperclip size={15} /> Attach</button>
          <div>
            <button className="button secondary" onClick={close} disabled={sending}>Discard</button>
            <button className="button primary" onClick={() => void send()} disabled={sending}>
              {sending ? <CircleNotch className="spin" size={15} /> : <PaperPlaneTilt size={15} weight="fill" />} {sending ? "Sending…" : "Send"}
            </button>
          </div>
        </footer>
      </div>
      {confirmDiscard && (
        <ConfirmDialog title="Discard this message?" confirmLabel="Discard" onConfirm={onClose} onCancel={() => setConfirmDiscard(false)}>
          Mail Sage does not keep drafts yet. The text that you wrote will be lost.
        </ConfirmDialog>
      )}
    </div>
  );
}
