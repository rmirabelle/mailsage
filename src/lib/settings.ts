/** The user's settings. Each one has a default, so a new setting needs no change to the saved data. */
export type Settings = {
  /** The message list opens with only the most recent day group open. */
  collapsePreviousDays: boolean;
};

export const DEFAULT_SETTINGS: Settings = { collapsePreviousDays: true };

const STORAGE_KEY = "mailsage.settings";

/** Reads the saved settings. A setting that is absent or has a wrong type gets its default. */
export function loadSettings(): Settings {
  try {
    return readSettings(JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null"));
  } catch {
    /* No storage, or damaged data: use the defaults. */
    return DEFAULT_SETTINGS;
  }
}

/** Makes settings from saved data that can be damaged or from an older version. */
export function readSettings(saved: unknown): Settings {
  if (!saved || typeof saved !== "object") return DEFAULT_SETTINGS;
  const values = saved as Record<string, unknown>;
  return {
    collapsePreviousDays: typeof values.collapsePreviousDays === "boolean" ? values.collapsePreviousDays : DEFAULT_SETTINGS.collapsePreviousDays
  };
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    /* The settings stay for this session only. */
  }
}
