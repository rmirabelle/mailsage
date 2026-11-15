import { useEffect, type ReactNode } from "react";

interface Props {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/** A question with a confirm button and a Cancel button. Escape cancels. */
export function ConfirmDialog({ title, children, confirmLabel, onConfirm, onCancel }: Props) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="about-dialog-overlay" role="presentation" onPointerDown={onCancel}>
      <div className="about-dialog confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" onPointerDown={(event) => event.stopPropagation()}>
        <div className="dialog-body">
          <h2 id="confirm-title">{title}</h2>
          <div className="dialog-note">{children}</div>
        </div>
        <footer className="about-dialog-actions">
          <button className="button secondary" onClick={onCancel}>Cancel</button>
          <button className="button danger" onClick={onConfirm}>{confirmLabel}</button>
        </footer>
      </div>
    </div>
  );
}
