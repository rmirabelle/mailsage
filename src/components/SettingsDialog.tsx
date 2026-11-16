import { EnvelopeSimple, Prohibit, X } from "@phosphor-icons/react";
import { useEffect, useState, type ReactNode } from "react";
import { addBlocked, listBlocked, unblockSender } from "../lib/mail";
import type { Settings } from "../lib/settings";

interface Props {
  /** The category that shows first. */
  initialCategory?: SettingsCategory;
  settings: Settings;
  /** A changed setting applies, and is saved, at once. */
  onChange: (settings: Settings) => void;
  onClose: () => void;
}

export type SettingsCategory = "emails" | "blocked";
type CategoryId = SettingsCategory;

const CATEGORIES: { id: CategoryId; label: string; icon: ReactNode }[] = [
  { id: "emails", label: "Emails", icon: <EnvelopeSimple size={15} /> },
  { id: "blocked", label: "Blocked senders", icon: <Prohibit size={15} /> }
];

/** The settings: the categories on the left, the settings of the selected category on the right. Escape closes. */
export function SettingsDialog({ initialCategory = "emails", settings, onChange, onClose }: Props) {
  const [category, setCategory] = useState<CategoryId>(initialCategory);
  /** The block list, or null until it is read. */
  const [blocked, setBlocked] = useState<string[] | null>(null);
  const [blockedError, setBlockedError] = useState<string | null>(null);

  useEffect(() => {
    void listBlocked().then(setBlocked).catch((error) => setBlockedError(String(error)));
  }, []);

  /** The text in the box that adds an entry by hand. */
  const [newEntry, setNewEntry] = useState("");

  /** Adds the typed entry. A text with no at sign is a domain, so it gets the at sign in front. */
  const addEntry = async () => {
    const typed = newEntry.trim();
    if (typed === "") return;
    setBlockedError(null);
    try {
      const added = await addBlocked(typed.includes("@") ? typed : `@${typed}`);
      setBlocked((current) => [...(current ?? []).filter((other) => other !== added), added].sort((first, second) => first.localeCompare(second)));
      setNewEntry("");
    } catch (error) {
      setBlockedError(String(error));
    }
  };

  const unblock = async (entry: string) => {
    setBlockedError(null);
    try {
      await unblockSender(entry);
      setBlocked((current) => current?.filter((other) => other !== entry) ?? null);
    } catch (error) {
      setBlockedError(String(error));
    }
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div className="about-dialog-overlay" role="presentation" onPointerDown={onClose}>
      <div className="about-dialog settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" onPointerDown={(event) => event.stopPropagation()}>
        <div className="settings-main">
          <nav className="settings-categories" aria-label="Settings categories">
            <h2 id="settings-title">Settings</h2>
            {CATEGORIES.map((entry) => (
              <button
                key={entry.id}
                className={`settings-category ${entry.id === category ? "selected" : ""}`}
                aria-current={entry.id === category}
                onClick={() => setCategory(entry.id)}
              >
                {entry.icon} {entry.label}
              </button>
            ))}
          </nav>
          <section className="settings-page" aria-label={CATEGORIES.find((entry) => entry.id === category)?.label}>
            {category === "emails" && (
              <>
                <h3>Emails</h3>
                <label className="settings-check">
                  <input
                    type="checkbox"
                    checked={settings.collapsePreviousDays}
                    onChange={(event) => onChange({ ...settings, collapsePreviousDays: event.target.checked })}
                  />
                  <span>
                    Collapse previous days by default
                    <small>The message list opens with only the most recent day open.</small>
                  </span>
                </label>
              </>
            )}
            {category === "blocked" && (
              <>
                <h3>Blocked senders</h3>
                <p className="settings-note">
                  Emails from these addresses and domains go to the Junk folder, and do not show in the Inbox.
                  To add one, right-click an email and select "Block sender" or "Block domain", or type it here.
                </p>
                <form className="blocked-add" onSubmit={(event) => { event.preventDefault(); void addEntry(); }}>
                  <input
                    type="text"
                    value={newEntry}
                    placeholder="name@example.com or example.com"
                    aria-label="A sender address or a domain to block"
                    spellCheck={false}
                    onChange={(event) => setNewEntry(event.target.value)}
                    onKeyDown={(event) => { if (event.key === "Escape" && newEntry !== "") { event.stopPropagation(); setNewEntry(""); } }}
                  />
                  <button className="button secondary" type="submit" disabled={newEntry.trim() === ""}>Block</button>
                </form>
                {blockedError && <p className="settings-note error selectable">{blockedError}</p>}
                {blocked !== null && blocked.length === 0 && <p className="settings-note">The list is empty.</p>}
                {blocked !== null && blocked.length > 0 && (
                  <ul className="blocked-list">
                    {blocked.map((entry) => (
                      <li key={entry}>
                        <span className="selectable">{entry}</span>
                        <small>{entry.startsWith("@") ? "Domain" : "Sender"}</small>
                        <button className="icon-button" onClick={() => void unblock(entry)} aria-label={`Remove the block of ${entry}`} title="Remove the block">
                          <X size={13} weight="bold" />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
          </section>
        </div>
        <footer className="about-dialog-actions">
          <button className="button secondary" onClick={onClose}>Close</button>
        </footer>
      </div>
    </div>
  );
}
