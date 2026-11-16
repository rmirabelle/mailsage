import { useEffect, useState } from "react";
import { MAX_LABEL_LENGTH } from "../lib/accountLabels";

interface Props {
  /** The email address of the account. */
  email: string;
  /** The label that the account has now, or an empty text. */
  label: string;
  /** Called with the new label. An empty text removes the label. */
  onSave: (label: string) => void;
  onCancel: () => void;
}

/** Asks for the label of an account in the accounts list. Enter saves. Escape cancels. */
export function LabelDialog({ email, label, onSave, onCancel }: Props) {
  const [text, setText] = useState(label);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="about-dialog-overlay" role="presentation" onPointerDown={onCancel}>
      <form
        className="about-dialog confirm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="label-title"
        onPointerDown={(event) => event.stopPropagation()}
        onSubmit={(event) => { event.preventDefault(); onSave(text); }}
      >
        <div className="dialog-body">
          <h2 id="label-title">Account label</h2>
          <p className="dialog-note selectable">{email}</p>
          <input
            className="dialog-input"
            type="text"
            value={text}
            maxLength={MAX_LABEL_LENGTH}
            placeholder={email}
            aria-label="The label of the account"
            spellCheck={false}
            autoFocus
            onFocus={(event) => event.target.select()}
            onChange={(event) => setText(event.target.value)}
          />
          <p className="dialog-note">The accounts list shows the label. With no label, it shows the email address.</p>
        </div>
        <footer className="about-dialog-actions">
          <button className="button secondary" type="button" onClick={onCancel}>Cancel</button>
          <button className="button primary" type="submit">Save</button>
        </footer>
      </form>
    </div>
  );
}
