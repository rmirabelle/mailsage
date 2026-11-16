import { describe, expect, it } from "vitest";
import { folderKey, folderRows } from "./folders";
import type { Folder, FolderRole } from "./mail";

let nextId = 1;
const folder = (path: string, role: FolderRole = "other", selectable = true): Folder => {
  const parts = path.split("/");
  return { id: nextId++, account_id: 1, path, name: parts[parts.length - 1], depth: parts.length - 1, role, selectable, unread: 0, total: 0 };
};

/** The folders of a Gmail account, in the order of the list: the well-known ones first. */
const gmail = () => [
  folder("INBOX", "inbox"),
  folder("[Gmail]/Sent Mail", "sent"),
  folder("[Gmail]/Trash", "trash"),
  folder("[Gmail]", "other", false),
  folder("[Gmail]/Important"),
  folder("Work"),
  folder("Work/Clients"),
  folder("Work/Clients/Old"),
  folder("Workshop")
];

const lines = (rows: ReturnType<typeof folderRows>) => rows.map((row) => `${"  ".repeat(row.depth)}${row.folder.path}`);
const none: ReadonlySet<string> = new Set();

describe("folderRows", () => {
  it("puts well-known folders at the top level, and the others under their parent", () => {
    expect(lines(folderRows(gmail(), none, none, false))).toEqual([
      "INBOX",
      "[Gmail]/Sent Mail",
      "[Gmail]/Trash",
      "[Gmail]",
      "  [Gmail]/Important",
      "Work",
      "  Work/Clients",
      "    Work/Clients/Old",
      "Workshop"
    ]);
  });

  it("marks the folders that have folders under them", () => {
    const rows = folderRows(gmail(), none, none, false);
    expect(rows.filter((row) => row.hasChildren).map((row) => row.folder.path)).toEqual(["[Gmail]", "Work", "Work/Clients"]);
  });

  it("does not show the folders under a closed folder", () => {
    const folders = gmail();
    const work = folders.find((entry) => entry.path === "Work")!;
    const rows = folderRows(folders, new Set([folderKey(work)]), none, false);
    expect(lines(rows)).not.toContain("  Work/Clients");
    expect(rows.find((row) => row.folder.path === "Work")).toMatchObject({ closed: true, hasChildren: true });
  });

  it("leaves out a hidden folder and the folders under it", () => {
    const folders = gmail();
    const work = folders.find((entry) => entry.path === "Work")!;
    const hidden = new Set([folderKey(work)]);
    expect(lines(folderRows(folders, none, hidden, false))).toEqual(["INBOX", "[Gmail]/Sent Mail", "[Gmail]/Trash", "[Gmail]", "  [Gmail]/Important", "Workshop"]);
    const shown = folderRows(folders, none, hidden, true);
    expect(shown.filter((row) => row.hidden).map((row) => row.folder.path)).toEqual(["Work", "Work/Clients", "Work/Clients/Old"]);
  });

  it("leaves out a folder that cannot hold messages when no folder shows under it", () => {
    const folders = gmail();
    const important = folders.find((entry) => entry.path === "[Gmail]/Important")!;
    expect(lines(folderRows(folders, none, new Set([folderKey(important)]), false))).not.toContain("[Gmail]");
  });
});
