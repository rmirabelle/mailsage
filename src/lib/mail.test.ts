import { describe, expect, it } from "vitest";
import { countAddresses, messagesFromBlocked, senderDomain, formatMessageDate, messageDayGroup, olderFromSender, searchMessages, type MessageHeader } from "./mail";

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

describe("messageDayGroup", () => {
  const now = new Date(2026, 10, 14, 15, 0, 0);
  const group = (date: Date) => messageDayGroup(seconds(date), now);

  it("names today and yesterday", () => {
    expect(group(new Date(2026, 10, 14, 0, 1))).toBe("Today");
    expect(group(new Date(2026, 10, 13, 23, 59))).toBe("Yesterday");
  });

  it("names the weekday for the days before yesterday", () => {
    expect(group(new Date(2026, 10, 12, 9, 0))).toBe(new Date(2026, 10, 12).toLocaleDateString(undefined, { weekday: "long" }));
    expect(group(new Date(2026, 10, 8, 9, 0))).toBe(new Date(2026, 10, 8).toLocaleDateString(undefined, { weekday: "long" }));
  });

  it("counts weeks, then says older", () => {
    expect(group(new Date(2026, 10, 7, 9, 0))).toBe("Last week");
    expect(group(new Date(2026, 9, 31, 9, 0))).toBe("Two weeks ago");
    expect(group(new Date(2026, 9, 24, 9, 0))).toBe("Three weeks ago");
    expect(group(new Date(2026, 9, 1, 9, 0))).toBe("Older");
    expect(messageDayGroup(0, now)).toBe("Older");
  });
});

describe("olderFromSender", () => {
  const header = (uid: number, from_address: string, date: number): MessageHeader =>
    ({ uid, subject: "", from_name: "", from_address, date, seen: true, flagged: false, size: 0 });
  const list = [header(5, "A@x.test", 500), header(4, "b@x.test", 400), header(3, "a@x.test", 300), header(2, "a@x.test", 200)];

  it("gives only the older messages of the same sender", () => {
    expect(olderFromSender(list, list[2]).map((message) => message.uid)).toEqual([2]);
    expect(olderFromSender(list, list[0]).map((message) => message.uid)).toEqual([3, 2]);
  });

  it("gives nothing for a sender with one message, or with no name and no address", () => {
    expect(olderFromSender(list, list[1])).toEqual([]);
    expect(olderFromSender([header(1, "", 100), header(2, "", 200)], header(2, "", 200))).toEqual([]);
  });
});

describe("searchMessages", () => {
  const header = (uid: number, from_name: string, from_address: string, subject: string): MessageHeader =>
    ({ uid, subject, from_name, from_address, date: uid, seen: true, flagged: false, size: 0 });
  const list = [
    header(3, "Ana Lima", "ana@shop.test", "Your order is ready"),
    header(2, "Bank", "news@bank.test", "Monthly statement"),
    header(1, "", "ana@shop.test", "Order received")
  ];
  const uids = (text: string) => searchMessages(list, text).map((message) => message.uid);

  it("gives all messages for an empty text", () => {
    expect(uids("  ")).toEqual([3, 2, 1]);
  });

  it("matches the sender name, the sender address, and the subject, with no difference for capital letters", () => {
    expect(uids("LIMA")).toEqual([3]);
    expect(uids("shop.test")).toEqual([3, 1]);
    expect(uids("statement")).toEqual([2]);
  });

  it("needs each word to match", () => {
    expect(uids("ana order")).toEqual([3, 1]);
    expect(uids("ana statement")).toEqual([]);
  });
});

describe("countAddresses", () => {
  it("counts the addresses of a list", () => {
    expect(countAddresses("")).toBe(0);
    expect(countAddresses("me@example.com")).toBe(1);
    expect(countAddresses("Ann <ann@example.com>, bob@example.com; carol@example.com")).toBe(3);
  });
  it("does not divide the list at a comma in quotes", () => {
    expect(countAddresses('"Lee, Carol" <carol@example.com>')).toBe(1);
    expect(countAddresses('"Lee, Carol" <carol@example.com>, , ')).toBe(1);
  });
});

describe("senderDomain", () => {
  it("gives the domain with the at sign in front", () => {
    expect(senderDomain("Ann@Example.com")).toBe("@example.com");
    expect(senderDomain("no-domain")).toBe("");
    expect(senderDomain("ann@")).toBe("");
  });
});

describe("messagesFromBlocked", () => {
  const from = (uid: number, from_address: string): MessageHeader => ({ uid, subject: "", from_name: "", from_address, date: 0, seen: true, flagged: false, size: 0 });
  const list = [from(1, "News@Shop.example"), from(2, "sales@shop.example"), from(3, "news@mail.shop.example"), from(4, "")];
  it("finds the messages of a sender address", () => {
    expect(messagesFromBlocked(list, "news@shop.example").map((message) => message.uid)).toEqual([1]);
  });
  it("finds the messages of a domain, but not of a domain under it", () => {
    expect(messagesFromBlocked(list, "@shop.example").map((message) => message.uid)).toEqual([1, 2]);
  });
});
