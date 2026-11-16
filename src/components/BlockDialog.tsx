import { useEffect, useState } from "react";

interface Props {
  /** The sender address, or the domain with the at sign in front. */
  entry: string;
  /** The number of emails in the folder's list that are from this sender or domain. */
  emailCount: number;
  /** True in the trash folder: there, the delete option does not show. */
  inTrash: boolean;
  onConfirm: (deleteEmails: boolean) => void;
  onCancel: () => void;
}

/** Asks before a block of a sender or a domain. The user can delete the emails from that sender also. Escape cancels. */
export function BlockDialog({ entry, emailCount, inTrash, onConfirm, onCancel }: Props) {
  const [deleteEmails, setDeleteEmails] = useState(false);
  const isDomain = entry.startsWith("@");

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="about-dialog-overlay" role="presentation" onPointerDown={onCancel}>
      <div className="about-dialog confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="block-title" onPointerDown={(event) => event.stopPropagation()}>
        <div className="dialog-body">
          <h2 id="block-title">{isDomain ? "Block domain?" : "Block sender?"}</h2>
          <p className="dialog-note">
            {isDomain ? `New emails from each address at ${entry.slice(1)}` : `New emails from ${entry}`} go to the Junk folder, and do not show in the Inbox.
            You can remove the block in File &gt; Settings.
          </p>
          {emailCount > 0 && !inTrash && (
            <label className="settings-check dialog-check">
              <input type="checkbox" checked={deleteEmails} onChange={(event) => setDeleteEmails(event.target.checked)} />
              <span>
                Delete {emailCount} {emailCount === 1 ? "email" : "emails"} from this {isDomain ? "domain" : "sender"}
                <small>They move to the trash. Without this, the emails that are in the Inbox move to the Junk folder.</small>
              </span>
            </label>
          )}
        </div>
        <footer className="about-dialog-actions">
          <button className="button secondary" onClick={onCancel}>Cancel</button>
          <button className="button danger" autoFocus onClick={() => onConfirm(deleteEmails)}>Block</button>
        </footer>
      </div>
    </div>
  );
}
