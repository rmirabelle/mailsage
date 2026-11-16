import { EnvelopeSimple } from "@phosphor-icons/react";
import { useEffect, useState, type ReactNode } from "react";
import type { Settings } from "../lib/settings";

interface Props {
  settings: Settings;
  /** A changed setting applies, and is saved, at once. */
  onChange: (settings: Settings) => void;
  onClose: () => void;
}

type CategoryId = "emails";

const CATEGORIES: { id: CategoryId; label: string; icon: ReactNode }[] = [
  { id: "emails", label: "Emails", icon: <EnvelopeSimple size={15} /> }
];

/** The settings: the categories on the left, the settings of the selected category on the right. Escape closes. */
export function SettingsDialog({ settings, onChange, onClose }: Props) {
  const [category, setCategory] = useState<CategoryId>("emails");

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
          </section>
        </div>
        <footer className="about-dialog-actions">
          <button className="button secondary" onClick={onClose}>Close</button>
        </footer>
      </div>
    </div>
  );
}
