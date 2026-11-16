import { Briefcase, CircleNotch, EnvelopeSimple, GoogleLogo, MicrosoftOutlookLogo, WarningCircle, X } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { addGmailAccount, addMicrosoftAccount, cancelSignIn, googleSignInReady, microsoftSignInReady } from "../lib/mail";

interface Props {
  onClose: () => void;
  /** Called with the new account's id after the sign-in is complete. */
  onAdded: (accountId: number) => void;
}

/** The kind of account to add. */
type Kind = "gmail" | "microsoft" | "work";
/** The company whose sign-in page the browser shows. */
type Provider = "Google" | "Microsoft";

type State =
  | { kind: "choose" }
  | { kind: "waiting"; provider: Provider }
  | { kind: "error"; message: string };

/** The dialog that adds an account. Gmail, Outlook.com, Hotmail and Microsoft 365 work now; other mail servers come in a later step of the plan. */
export function AddAccountDialog({ onClose, onAdded }: Props) {
  const [state, setState] = useState<State>({ kind: "choose" });
  const [googleReady, setGoogleReady] = useState<boolean | null>(null);
  const [microsoftReady, setMicrosoftReady] = useState<boolean | null>(null);
  const [workReady, setWorkReady] = useState<boolean | null>(null);
  const waiting = state.kind === "waiting";

  useEffect(() => {
    void googleSignInReady().then(setGoogleReady).catch(() => setGoogleReady(false));
    void microsoftSignInReady(false).then(setMicrosoftReady).catch(() => setMicrosoftReady(false));
    void microsoftSignInReady(true).then(setWorkReady).catch(() => setWorkReady(false));
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !waiting) onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, waiting]);

  const add = async (kind: Kind) => {
    setState({ kind: "waiting", provider: kind === "gmail" ? "Google" : "Microsoft" });
    try {
      const account = await (kind === "gmail" ? addGmailAccount() : addMicrosoftAccount(kind === "work"));
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
          {state.kind === "waiting" ? (
            <>
              <p className="dialog-status"><CircleNotch className="spin" size={16} /> Your browser shows the {state.provider} sign-in page. Sign in there.</p>
              <p className="dialog-note">Mail Sage does not see your {state.provider} password.</p>
            </>
          ) : (
            <>
              {state.kind === "error" && (
                <p className="dialog-status error"><WarningCircle size={16} weight="fill" /> <span className="selectable">{state.message}</span></p>
              )}
              <div className="account-kinds">
                <button className="account-kind" onClick={() => void add("gmail")} disabled={!googleReady}>
                  <GoogleLogo className="account-kind-icon google" size={26} weight="bold" />
                  <strong>Gmail</strong>
                  <span>{googleReady === false ? "Not set up in this build. See docs/GOOGLE-SETUP.md." : "Sign in with Google in your browser."}</span>
                </button>
                <button className="account-kind" onClick={() => void add("microsoft")} disabled={!microsoftReady}>
                  <MicrosoftOutlookLogo className="account-kind-icon microsoft" size={26} weight="fill" />
                  <strong>Outlook.com or Hotmail</strong>
                  <span>{microsoftReady === false ? "Not set up in this build. See docs/MICROSOFT-SETUP.md." : "Sign in with Microsoft in your browser."}</span>
                </button>
                <button className="account-kind" onClick={() => void add("work")} disabled={!workReady}>
                  <Briefcase className="account-kind-icon microsoft" size={26} weight="fill" />
                  <strong>Work or school (Microsoft 365)</strong>
                  <span>{workReady === false ? "Not set up in this build. See docs/MICROSOFT-SETUP.md." : "Sign in with Microsoft in your browser."}</span>
                </button>
                <button className="account-kind" disabled>
                  <EnvelopeSimple className="account-kind-icon" size={26} />
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
