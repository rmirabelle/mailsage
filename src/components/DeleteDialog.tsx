import { useEffect, useState } from "react";

interface Props {
  subject: string;
  /** The number of older messages from the same sender in the folder. */
  olderCount: number;
  /** True in the trash folder: the delete is for good. */
  forGood: boolean;
  onConfirm: (deleteOlder: boolean) => void;
  onCancel: () => void;
}

/** Asks before a delete of a message whose sender has older messages. The user can delete those also. Escape cancels. */
export function DeleteDialog({ subject, olderCount, forGood, onConfirm, onCancel }: Props) {
  const [deleteOlder, setDeleteOlder] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="about-dialog-overlay" role="presentation" onPointerDown={onCancel}>
      <div className="about-dialog confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="delete-title" onPointerDown={(event) => event.stopPropagation()}>
        <div className="dialog-body">
          <h2 id="delete-title">{forGood ? "Delete email for good?" : "Delete email?"}</h2>
          <p className="dialog-note">"{subject || "(no subject)"}"</p>
          <label className="settings-check dialog-check">
            <input type="checkbox" checked={deleteOlder} onChange={(event) => setDeleteOlder(event.target.checked)} />
            <span>Delete {olderCount} previous {olderCount === 1 ? "email" : "emails"}</span>
          </label>
          {forGood && <p className="dialog-note">This deletes from the server, and the emails cannot come back.</p>}
        </div>
        <footer className="about-dialog-actions">
          <button className="button secondary" onClick={onCancel}>Cancel</button>
          <button className="button danger" autoFocus onClick={() => onConfirm(deleteOlder)}>Delete</button>
        </footer>
      </div>
    </div>
  );
}
