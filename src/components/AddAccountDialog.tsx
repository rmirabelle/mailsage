import { CircleNotch, WarningCircle, X } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { addGmailAccount, cancelSignIn, googleSignInReady } from "../lib/mail";

interface Props {
  onClose: () => void;
  /** Called with the new account's id after the sign-in is complete. */
  onAdded: (accountId: number) => void;
}

type State =
  | { kind: "choose" }
  | { kind: "waiting" }
  | { kind: "error"; message: string };

/** The dialog that adds an account. Gmail works now; the other kinds come in a later step of the plan. */
export function AddAccountDialog({ onClose, onAdded }: Props) {
  const [state, setState] = useState<State>({ kind: "choose" });
  const [googleReady, setGoogleReady] = useState<boolean | null>(null);
  const waiting = state.kind === "waiting";

  useEffect(() => {
    void googleSignInReady().then(setGoogleReady).catch(() => setGoogleReady(false));
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !waiting) onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, waiting]);

  const addGmail = async () => {
    setState({ kind: "waiting" });
    try {
      const account = await addGmailAccount();
      onAdded(account.id);
    } catch (error) {
      setState({ kind: "error", message: String(error) });
    }
  };

  return (
    <div className="about-dialog-overlay" role="presentation" onPointerDown={() => !waiting && onClose()}>
      <div className="about-dialog add-account-dialog" role="dialog" aria-modal="true" aria-labelledby="add-account-title" onPointerDown={(event) => event.stopPropagation()}>
        {!waiting && (
          <button className="about-dialog-close" onClick={onClose} aria-label="Close" title="Close">
            <X size={18} />
          </button>
        )}
        <div className="dialog-body">
          <h2 id="add-account-title">Add account</h2>
          {waiting ? (
            <>
              <p className="dialog-status"><CircleNotch className="spin" size={16} /> Your browser shows the Google sign-in page. Sign in there.</p>
              <p className="dialog-note">Mail Sage does not see your Google password.</p>
            </>
          ) : (
            <>
              {state.kind === "error" && (
                <p className="dialog-status error"><WarningCircle size={16} weight="fill" /> <span className="selectable">{state.message}</span></p>
              )}
              <div className="account-kinds">
                <button className="account-kind" onClick={() => void addGmail()} disabled={!googleReady}>
                  <strong>Gmail</strong>
                  <span>{googleReady === false ? "Not set up in this build. See docs/GOOGLE-SETUP.md." : "Sign in with Google in your browser."}</span>
                </button>
                <button className="account-kind" disabled>
                  <strong>Outlook.com or Hotmail</strong>
                  <span>Comes in a later version.</span>
                </button>
                <button className="account-kind" disabled>
                  <strong>Other mail server</strong>
                  <span>Comes in a later version.</span>
                </button>
              </div>
            </>
          )}
        </div>
        <footer className="about-dialog-actions">
          {waiting
            ? <button className="button secondary" onClick={() => void cancelSignIn()}>Cancel sign-in</button>
            : <button className="button secondary" onClick={onClose}>Close</button>}
        </footer>
      </div>
    </div>
  );
}
