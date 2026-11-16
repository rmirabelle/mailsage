import { describe, expect, it } from "vitest";
import { readAccountLabels, withAccountLabel } from "./accountLabels";

describe("account labels", () => {
  it("reads saved labels and drops damaged entries", () => {
    expect(readAccountLabels({ 1: " Work ", 2: "", x: "Bad", 3: 7 })).toEqual({ 1: "Work" });
    expect(readAccountLabels(null)).toEqual({});
    expect(readAccountLabels(["Work"])).toEqual({});
  });

  it("sets a label, and removes it with an empty text", () => {
    const labels = withAccountLabel({}, 4, "  Personal ");
    expect(labels).toEqual({ 4: "Personal" });
    expect(withAccountLabel(labels, 4, "   ")).toEqual({});
  });
});
