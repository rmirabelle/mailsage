import { ShieldCheck, Warning } from "@phosphor-icons/react";
import { useEffect } from "react";
import { openLink, type LinkCheck } from "../lib/mail";

interface Props {
  link: LinkCheck;
  onClose: () => void;
  onError: (text: string) => void;
}

/** Shows where a link really goes, with warnings, before the browser opens it. */
export function LinkDialog({ link, onClose, onError }: Props) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const open = () => {
    onClose();
    void openLink(link.url).catch((problem) => onError(String(problem)));
  };

  /* The site name stands out in the address, because the rest of an address can be made to look like a different site. */
  const hostStart = link.url.toLowerCase().indexOf(link.host);
  const warnings = [
    link.text_mismatch && "The message shows a different site for this link. This is a common trick in false messages.",
    link.had_user_part && "The address tried to hide the real site behind a different name. Mail Sage removed that part.",
    link.lookalike_risk && "The site name uses special characters that can look like the letters of a different site.",
    link.not_encrypted && "This link is not encrypted (http). Other people on the network can see what you send to this site."
  ].filter((warning): warning is string => Boolean(warning));

  return (
    <div className="about-dialog-overlay" role="presentation" onPointerDown={onClose}>
      <div className="about-dialog link-dialog" role="alertdialog" aria-modal="true" aria-labelledby="link-title" onPointerDown={(event) => event.stopPropagation()}>
        <div className="dialog-body">
          <h2 id="link-title">Open this link in your browser?</h2>
          <p className="link-address selectable">
            {hostStart >= 0
              ? <>{link.url.slice(0, hostStart)}<strong>{link.url.slice(hostStart, hostStart + link.host.length)}</strong>{link.url.slice(hostStart + link.host.length)}</>
              : link.url}
          </p>
          <ul className="link-warnings">
            {warnings.map((warning) => <li key={warning}><Warning size={15} weight="fill" /> {warning}</li>)}
            {link.tracking_removed && <li className="info"><ShieldCheck size={15} weight="fill" /> Mail Sage removed tracking data from this address.</li>}
          </ul>
          <p className="dialog-note">The link goes to <strong>{link.host}</strong>.</p>
        </div>
        <footer className="about-dialog-actions">
          <button className="button secondary" onClick={onClose}>Cancel</button>
          <button className={`button ${warnings.length > 0 ? "danger" : "primary"}`} onClick={open}>Open link</button>
        </footer>
      </div>
    </div>
  );
}
