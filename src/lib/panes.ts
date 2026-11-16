/** The widths, in pixels, of the two columns that the user can resize. The preview gets the remaining space. */
export type PaneWidths = { accounts: number; list: number };

export const DEFAULT_PANE_WIDTHS: PaneWidths = { accounts: 240, list: 360 };
export const PANE_LIMITS = { accounts: { min: 160, max: 520 }, list: { min: 240, max: 900 } };
/** The preview always keeps this much width. */
export const MIN_PREVIEW_WIDTH = 320;

const STORAGE_KEY = "mailsage.paneWidths";

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, Math.round(value)));

/**
 * Makes the widths fit in the space of `total` pixels. When the space is too
 * small, the message list gives up width first, then the accounts column.
 * A `total` of 0 means that the space is not known yet.
 */
export function fitPaneWidths(widths: PaneWidths, total: number): PaneWidths {
  let accounts = clamp(widths.accounts, PANE_LIMITS.accounts.min, PANE_LIMITS.accounts.max);
  let list = clamp(widths.list, PANE_LIMITS.list.min, PANE_LIMITS.list.max);
  if (total > 0) {
    const space = Math.max(0, Math.floor(total) - MIN_PREVIEW_WIDTH);
    list = Math.max(PANE_LIMITS.list.min, Math.min(list, space - accounts));
    accounts = Math.max(PANE_LIMITS.accounts.min, Math.min(accounts, space - list));
  }
  return { accounts, list };
}

/** Reads the saved widths. Gives the default widths when there are none, or when they are damaged. */
export function loadPaneWidths(): PaneWidths {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null");
    if (saved && typeof saved === "object") {
      const { accounts, list } = saved as Record<string, unknown>;
      if (typeof accounts === "number" && typeof list === "number" && Number.isFinite(accounts) && Number.isFinite(list)) {
        return fitPaneWidths({ accounts, list }, 0);
      }
    }
  } catch {
    /* No storage, or damaged data: use the default widths. */
  }
  return DEFAULT_PANE_WIDTHS;
}

export function savePaneWidths(widths: PaneWidths): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(widths));
  } catch {
    /* The widths stay for this session only. */
  }
}
