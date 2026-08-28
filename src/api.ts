import { invoke } from "@tauri-apps/api/core";

export type Status = "keep" | "scratch";

export interface Note {
  filename: string;
  status: Status;
  title: string;
  body: string;
  tags: string[];
  createdAt: string;
  modifiedAt: string;
  isExpiring: boolean;
  charCount: number;
}

/** What the list gets — everything but the full body. */
export interface NoteSummary {
  filename: string;
  status: Status;
  title: string;
  snippet: string;
  tags: string[];
  createdAt: string;
  modifiedAt: string;
  isExpiring: boolean;
  charCount: number;
}

export interface NoteFilter {
  status?: Status;
  query?: string;
}

export interface Settings {
  notesDir: string;
  hotkey: string;
  hideOnBlur: boolean;
  expiryDays: number;
  lastStatus: Status;
  indentWidth: number;
}

export const listNotes = (filter: NoteFilter) =>
  invoke<NoteSummary[]>("list_notes", { filter });

export const getNote = (filename: string) => invoke<Note>("get_note", { filename });

export const createNote = (body: string, status: Status) =>
  invoke<Note>("create_note", { body, status });

export const updateNote = (filename: string, body: string) =>
  invoke<Note>("update_note", { filename, body });

export const setStatus = (filename: string, status: Status) =>
  invoke<Note>("set_status", { filename, status });

export const deleteNote = (filename: string) => invoke<void>("delete_note", { filename });

export const openNotesFolder = () => invoke<void>("open_notes_folder");

export const getSettings = () => invoke<Settings>("get_settings");

export const setSettings = (next: Settings) => invoke<Settings>("set_settings", { next });

export const hideWindow = () => invoke<void>("hide_window");

function isTagChar(ch: string): boolean {
  return /[\p{L}\p{N}_-]/u.test(ch);
}

/**
 * Client-side mirror of `notes::tags::extract` in the Rust backend — used
 * only for a live preview while composing (the saved file is always the
 * source of truth; the backend re-extracts from the actual written body).
 * Tags in first-appearance order, deduplicated, without the leading `#`.
 */
export function extractTags(body: string): string[] {
  const tags: string[] = [];
  const chars = Array.from(body);
  let i = 0;
  while (i < chars.length) {
    if (chars[i] === "#" && (i === 0 || !isTagChar(chars[i - 1]))) {
      let end = i + 1;
      while (end < chars.length && isTagChar(chars[end])) end++;
      if (end > i + 1) {
        const tag = chars.slice(i + 1, end).join("");
        if (!tags.includes(tag)) tags.push(tag);
        i = end;
        continue;
      }
    }
    i++;
  }
  return tags;
}

/**
 * How long ago, at the resolution that matters for a note list — recent
 * things read as "how fresh", older things read as an actual date.
 */
export function formatWhen(iso: string): string {
  if (!iso) return "";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "";
  const seconds = Math.max(0, (Date.now() - then) / 1000);
  if (seconds < 60) return "now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)}h`;
  if (seconds < 7 * 86_400) return `${Math.floor(seconds / 86_400)}d`;
  return new Date(then).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
