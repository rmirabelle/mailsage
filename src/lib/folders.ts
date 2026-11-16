import type { Folder } from "./mail";

/** One line of the folder list. */
export type FolderRow = {
  folder: Folder;
  /** How far the line is indented: 0 for the top level. */
  depth: number;
  /** True when the folder has folders under it that can show. */
  hasChildren: boolean;
  /** True when the user closed the folder: the folders under it do not show. */
  closed: boolean;
  /** True when the user hid the folder. It shows only while hidden folders show. */
  hidden: boolean;
};

/** The name of a folder in the saved sets: the account number and the server's folder path. */
export const folderKey = (folder: Folder) => `${folder.account_id}:${folder.path}`;

type Node = { folder: Folder; children: Node[] };

/** True when `child` is directly under `parent` on the server: one level deeper, and its path starts with the parent's path and a divider. */
function isDirectChild(parent: Folder, child: Folder): boolean {
  if (child.depth !== parent.depth + 1 || !child.path.startsWith(parent.path)) return false;
  const next = child.path.charAt(parent.path.length);
  return next !== "" && !/[A-Za-z0-9]/.test(next);
}

/**
 * Makes the tree of an account's folders. The folders are in the order of
 * the list (the well-known ones first), and each level keeps that order.
 * A well-known folder (Inbox, Sent, Trash, and so on) goes to the top level
 * also when the server has it under a different folder, as Gmail does with
 * "[Gmail]".
 */
function buildTree(folders: Folder[]): Node[] {
  const nodes = folders.map((folder): Node => ({ folder, children: [] }));
  const top: Node[] = [];
  for (const node of nodes) {
    const parent = node.folder.role === "other" ? nodes.find((other) => isDirectChild(other.folder, node.folder)) : undefined;
    if (parent) parent.children.push(node); else top.push(node);
  }
  return top;
}

/**
 * The lines of an account's folder list, in order.
 *
 * - A folder that the user closed does not show the folders under it.
 * - A folder that the user hid, and all folders under it, show only when
 *   `showHidden` is true.
 * - A folder that cannot hold messages and has no folder to show under it
 *   (for example "[Gmail]" when all its folders are at the top level) is
 *   left out.
 */
export function folderRows(folders: Folder[], closed: ReadonlySet<string>, hidden: ReadonlySet<string>, showHidden: boolean): FolderRow[] {
  const rows: FolderRow[] = [];
  const add = (node: Node, depth: number, parentHidden: boolean): boolean => {
    const key = folderKey(node.folder);
    const isHidden = parentHidden || hidden.has(key);
    if (isHidden && !showHidden) return false;
    const row: FolderRow = { folder: node.folder, depth, hasChildren: false, closed: closed.has(key), hidden: isHidden };
    const place = rows.length;
    rows.push(row);
    let shown = 0;
    for (const child of node.children) {
      if (add(child, depth + 1, isHidden)) shown += 1;
    }
    row.hasChildren = shown > 0;
    if (!node.folder.selectable && shown === 0) {
      rows.length = place;
      return false;
    }
    if (row.closed) rows.length = place + 1;
    return true;
  };
  for (const node of buildTree(folders)) add(node, 0, false);
  return rows;
}

function loadSet(storageKey: string): Set<string> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "null");
    if (Array.isArray(saved)) return new Set(saved.filter((entry): entry is string => typeof entry === "string"));
  } catch {
    /* No storage, or damaged data: start with an empty set. */
  }
  return new Set();
}

function saveSet(storageKey: string, entries: ReadonlySet<string>): void {
  try {
    localStorage.setItem(storageKey, JSON.stringify([...entries]));
  } catch {
    /* The set stays for this session only. */
  }
}

const CLOSED_KEY = "mailsage.closedFolders";
const HIDDEN_KEY = "mailsage.hiddenFolders";

export const loadClosedFolders = () => loadSet(CLOSED_KEY);
export const saveClosedFolders = (entries: ReadonlySet<string>) => saveSet(CLOSED_KEY, entries);
export const loadHiddenFolders = () => loadSet(HIDDEN_KEY);
export const saveHiddenFolders = (entries: ReadonlySet<string>) => saveSet(HIDDEN_KEY, entries);
