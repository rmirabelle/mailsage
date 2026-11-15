import { CircleNotch, Paperclip, ShieldCheck, Warning, WarningCircle } from "@phosphor-icons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { checkLink, formatFileSize, openMessage, saveAttachment, setSenderTrusted, type AttachmentInfo, type LinkCheck, type MessageHeader, type MessageView } from "../lib/mail";
import { ConfirmDialog } from "./ConfirmDialog";
import { LinkDialog } from "./LinkDialog";

interface Props {
  folderId: number;
  message: MessageHeader;
}

type Notice = { kind: "info" | "error"; text: string };

/**
 * The right pane: the secure preview of one message.
 *
 * Rust cleans the message before it comes here (src-tauri/src/preview.rs).
 * The frame below cannot run scripts: its sandbox has no "allow-scripts".
 * It has "allow-same-origin" only so that this component can catch a click
 * on a link; a link never opens by the click alone.
 */
export function MessagePreview({ folderId, message }: Props) {
  const [view, setView] = useState<MessageView | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [link, setLink] = useState<LinkCheck | null>(null);
  const [riskyAttachment, setRiskyAttachment] = useState<AttachmentInfo | null>(null);
  /** The message that this pane shows. An answer for a different message is old, and is ignored. */
  const shown = useRef("");
  const key = `${folderId}:${message.uid}`;

  const load = useCallback(async (loadRemote: boolean) => {
    const current = () => shown.current === key;
    setBusy(true);
    setError(null);
    try {
      const result = await openMessage(folderId, message.uid, loadRemote);
      if (current()) setView(result);
    } catch (problem) {
      if (current()) setError(String(problem));
    } finally {
      if (current()) setBusy(false);
    }
  }, [folderId, message.uid, key]);

  useEffect(() => {
    shown.current = key;
    setView(null);
    setNotice(null);
    setLink(null);
    setRiskyAttachment(null);
    void load(false);
  }, [key, load]);

  /** Catches every way to use a link in the frame: click, middle click, the keyboard, and the context menu. */
  const guardFrame = (frame: HTMLIFrameElement) => {
    const page = frame.contentDocument;
    if (!page) return;
    const onLink = (event: Event) => {
      const anchor = (event.target as Element | null)?.closest?.("a[href]");
      if (!anchor) return;
      event.preventDefault();
      const href = anchor.getAttribute("href") ?? "";
      if (!/^https?:/i.test(href.trim())) return;
      void checkLink(href, anchor.textContent ?? "")
        .then(setLink)
        .catch((problem) => setNotice({ kind: "error", text: String(problem) }));
    };
    page.addEventListener("click", onLink);
    page.addEventListener("auxclick", onLink);
    page.addEventListener("contextmenu", (event) => event.preventDefault());
    page.addEventListener("dragstart", (event) => event.preventDefault());
  };

  const save = async (attachment: AttachmentInfo) => {
    setNotice(null);
    try {
      const path = await saveAttachment(folderId, message.uid, attachment.index);
      if (path) setNotice({ kind: "info", text: `Saved ${path}` });
    } catch (problem) {
      setNotice({ kind: "error", text: String(problem) });
    }
  };

  /** Adds the sender to the list of senders whose images always load, or removes it, then shows the message again. */
  const trustSender = async (trusted: boolean) => {
    if (!view) return;
    try {
      await setSenderTrusted(view.from_address, trusted);
      await load(false);
    } catch (problem) {
      setNotice({ kind: "error", text: String(problem) });
    }
  };

  const sender = view ? view.from_name || view.from_address : message.from_name || message.from_address;
  const senderAddress = view?.from_address ?? message.from_address;

  return (
    <section className="pane preview-pane" aria-label="Secure preview">
      <header className="preview-header">
        <h2 className="preview-subject selectable">{(view?.subject ?? message.subject) || "(no subject)"}</h2>
        <dl className="preview-fields selectable">
          <div className="preview-line">
            <dt>From</dt>
            <dd><strong>{sender || "(no sender)"}</strong>{senderAddress && sender !== senderAddress ? ` <${senderAddress}>` : ""}</dd>
          </div>
          {view?.to && <div className="preview-line"><dt>To</dt><dd>{view.to}</dd></div>}
          {view?.cc && <div className="preview-line"><dt>Cc</dt><dd>{view.cc}</dd></div>}
        </dl>
        {message.date > 0 && <div className="preview-date">{new Date(message.date * 1000).toLocaleString()}</div>}
      </header>

      {error && (
        <div className="preview-bar error"><WarningCircle size={15} weight="fill" /><span className="selectable">{error}</span></div>
      )}
      {view && view.blocked_trackers > 0 && (
        <div className="preview-bar">
          <ShieldCheck size={15} weight="fill" />
          <span>Mail Sage removed {view.blocked_trackers} tracking {view.blocked_trackers === 1 ? "image" : "images"}. These never load.</span>
        </div>
      )}
      {view && view.blocked_images > 0 && !view.remote_loaded && (
        <div className="preview-bar">
          <ShieldCheck size={15} weight="fill" />
          <span>Mail Sage blocked {view.blocked_images} remote {view.blocked_images === 1 ? "image" : "images"}. If you load them, the sender can see that you opened the message.</span>
          <button className="button secondary" disabled={busy} onClick={() => void load(true)}>Load one time</button>
          {view.from_address && <button className="button secondary" disabled={busy} onClick={() => void trustSender(true)}>Always for this sender</button>}
        </div>
      )}
      {view && view.remote_loaded && (
        <div className="preview-bar warn">
          <Warning size={15} weight="fill" />
          <span>
            {view.sender_trusted ? `Images from ${view.from_address} always load.` : "Remote images are loaded for this message."}
            {view.blocked_images > 0 ? ` ${view.blocked_images} did not load.` : ""}
          </span>
          {view.sender_trusted
            ? <button className="button secondary" disabled={busy} onClick={() => void trustSender(false)}>Stop</button>
            : view.from_address && <button className="button secondary" disabled={busy} onClick={() => void trustSender(true)}>Always for this sender</button>}
        </div>
      )}
      {notice && (
        <div className={`preview-bar ${notice.kind === "error" ? "error" : ""}`}>
          {notice.kind === "error" ? <WarningCircle size={15} weight="fill" /> : <ShieldCheck size={15} weight="fill" />}
          <span className="selectable">{notice.text}</span>
        </div>
      )}
      {view && view.attachments.length > 0 && (
        <div className="preview-attachments">
          {view.attachments.map((attachment) => (
            <button
              key={attachment.index}
              className={`attachment-chip ${attachment.risky ? "risky" : ""}`}
              title={attachment.risky ? `${attachment.name}: this file type can run code. Save it only if you trust the sender.` : `Save ${attachment.name}`}
              onClick={() => attachment.risky ? setRiskyAttachment(attachment) : void save(attachment)}
            >
              {attachment.risky ? <Warning size={14} weight="fill" /> : <Paperclip size={14} />}
              <span>{attachment.name}</span>
              <small>{formatFileSize(attachment.size)}</small>
            </button>
          ))}
        </div>
      )}

      {view ? (
        <iframe
          key={`${key}:${view.remote_loaded}`}
          className="preview-frame"
          title="Message"
          sandbox="allow-same-origin"
          referrerPolicy="no-referrer"
          srcDoc={view.html}
          onLoad={(event) => guardFrame(event.currentTarget)}
        />
      ) : (
        <div className="pane-empty">
          {busy ? <CircleNotch className="spin" size={26} /> : <ShieldCheck size={30} />}
          <p>{busy ? "Reading the message…" : "Mail Sage cannot show this message."}</p>
        </div>
      )}

      {link && <LinkDialog link={link} onClose={() => setLink(null)} onError={(text) => setNotice({ kind: "error", text })} />}
      {riskyAttachment && (
        <ConfirmDialog
          title={`Save ${riskyAttachment.name}?`}
          confirmLabel="Save file"
          onConfirm={() => { const attachment = riskyAttachment; setRiskyAttachment(null); void save(attachment); }}
          onCancel={() => setRiskyAttachment(null)}
        >
          This file type can run code or hold macros. Save it only if you know the sender and expected the file. Mail Sage does not open it.
        </ConfirmDialog>
      )}
    </section>
  );
}
