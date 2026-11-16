import { describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS, readSettings } from "./settings";

describe("readSettings", () => {
  it("gives the defaults when there is no saved data", () => {
    expect(readSettings(null)).toEqual(DEFAULT_SETTINGS);
    expect(readSettings("text")).toEqual(DEFAULT_SETTINGS);
  });

  it("keeps a saved value", () => {
    expect(readSettings({ collapsePreviousDays: false })).toEqual({ collapsePreviousDays: false });
  });

  it("gives the default for a value with a wrong type", () => {
    expect(readSettings({ collapsePreviousDays: "no" })).toEqual(DEFAULT_SETTINGS);
  });
});
