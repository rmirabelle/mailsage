/** The labels that the user gave to accounts, by account number. An account with no label shows its email address. */
export type AccountLabels = Readonly<Record<number, string>>;

export const MAX_LABEL_LENGTH = 60;
const STORAGE_KEY = "mailsage.accountLabels";

/** Reads saved labels. Entries that are not a text for a number are dropped. */
export function readAccountLabels(saved: unknown): AccountLabels {
  const labels: Record<number, string> = {};
  if (saved && typeof saved === "object" && !Array.isArray(saved)) {
    for (const [id, label] of Object.entries(saved as Record<string, unknown>)) {
      if (/^\d+$/.test(id) && typeof label === "string" && label.trim() !== "") labels[Number(id)] = label.trim().slice(0, MAX_LABEL_LENGTH);
    }
  }
  return labels;
}

export function loadAccountLabels(): AccountLabels {
  try {
    return readAccountLabels(JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null"));
  } catch {
    /* No storage, or damaged data: no labels. */
    return {};
  }
}

/** Gives the labels with the label of one account set. An empty label removes it, and the account shows its email address again. */
export function withAccountLabel(labels: AccountLabels, accountId: number, label: string): AccountLabels {
  const next: Record<number, string> = { ...labels };
  const clean = label.trim().slice(0, MAX_LABEL_LENGTH);
  if (clean === "") delete next[accountId]; else next[accountId] = clean;
  return next;
}

export function saveAccountLabels(labels: AccountLabels): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(labels));
  } catch {
    /* The labels stay for this session only. */
  }
}
