import { describe, expect, it } from "vitest";
import { fitPaneWidths, MIN_PREVIEW_WIDTH, PANE_LIMITS } from "./panes";

describe("fitPaneWidths", () => {
  it("keeps widths that fit", () => {
    expect(fitPaneWidths({ accounts: 300, list: 400 }, 1600)).toEqual({ accounts: 300, list: 400 });
  });

  it("holds each width inside its limits", () => {
    expect(fitPaneWidths({ accounts: 10, list: 5000 }, 0)).toEqual({ accounts: PANE_LIMITS.accounts.min, list: PANE_LIMITS.list.max });
  });

  it("makes the message list narrower first when the space is small", () => {
    expect(fitPaneWidths({ accounts: 300, list: 600 }, 1000)).toEqual({ accounts: 300, list: 1000 - MIN_PREVIEW_WIDTH - 300 });
  });

  it("makes the accounts column narrower when the message list is at its minimum", () => {
    const fitted = fitPaneWidths({ accounts: 400, list: 600 }, 820);
    expect(fitted).toEqual({ accounts: 820 - MIN_PREVIEW_WIDTH - PANE_LIMITS.list.min, list: PANE_LIMITS.list.min });
  });
});
