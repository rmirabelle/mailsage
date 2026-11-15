import { describe, expect, it } from "vitest";
import { formatMessageDate } from "./mail";

const seconds = (date: Date) => date.getTime() / 1000;

describe("formatMessageDate", () => {
  const now = new Date(2026, 10, 14, 15, 0, 0);

  it("shows the time for a message of today", () => {
    const text = formatMessageDate(seconds(new Date(2026, 10, 14, 9, 5)), now);
    expect(text).toMatch(/9.05/);
  });

  it("shows the weekday for a message of the last week", () => {
    const date = new Date(2026, 10, 11, 9, 5);
    expect(formatMessageDate(seconds(date), now)).toBe(date.toLocaleDateString(undefined, { weekday: "short" }));
  });

  it("shows the year only for a message of a different year", () => {
    expect(formatMessageDate(seconds(new Date(2026, 2, 3)), now)).not.toContain("2026");
    expect(formatMessageDate(seconds(new Date(2025, 2, 3)), now)).toContain("2025");
  });

  it("shows nothing for a message with no date", () => {
    expect(formatMessageDate(0, now)).toBe("");
  });
});
