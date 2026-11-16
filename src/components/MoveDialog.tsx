import { Folder as FolderIcon } from "@phosphor-icons/react";
import { useEffect } from "react";
import type { Folder } from "../lib/mail";

interface Props {
  /** How many emails move. */
  count: number;
  /** The folders of the account that can get the emails, in the order of the folder list. */
  folders: Folder[];
  onMove: (target: Folder) => void;
  onCancel: () => void;
}

/** Asks for the folder that emails move to. A click on a folder moves them. Escape cancels. */
export function MoveDialog({ count, folders, onMove, onCancel }: Props) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="about-dialog-overlay" role="presentation" onPointerDown={onCancel}>
      <div className="about-dialog confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="move-title" onPointerDown={(event) => event.stopPropagation()}>
        <div className="dialog-body">
          <h2 id="move-title">Move {count === 1 ? "1 email" : `${count} emails`} to</h2>
          <div className="move-folders">
            {folders.map((folder) => (
              <button key={folder.id} className="move-folder" onClick={() => onMove(folder)}>
                <FolderIcon size={15} />
                <span>{folder.name}</span>
              </button>
            ))}
            {folders.length === 0 && <p className="dialog-note">This account has no other folder.</p>}
          </div>
        </div>
        <footer className="about-dialog-actions">
          <button className="button secondary" onClick={onCancel}>Cancel</button>
        </footer>
      </div>
    </div>
  );
}
