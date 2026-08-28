import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import ComposeBox from "./components/ComposeBox";
import SearchView from "./components/SearchView";
import SettingsPanel from "./components/SettingsPanel";
import {
  createNote,
  deleteNote,
  getNote,
  getSettings,
  hideWindow,
  listNotes,
  setStatus,
  updateNote,
  type NoteSummary,
  type Settings,
  type Status,
} from "./api";

type View = "compose" | "search" | "settings";

/** A courtesy debounce on top of an already-cheap in-memory backend search. */
const DEBOUNCE_MS = 120;

/** How long compose typing must pause before it's autosaved to disk — short
 *  enough to feel immediate, long enough to not write on every keystroke.
 *  Applies equally to a brand-new note: the file is created the first time
 *  this fires with non-empty content, same as an existing one being updated. */
const AUTOSAVE_DEBOUNCE_MS = 500;

export default function App() {
  const [view, setView] = useState<View>("compose");

  // Compose state — the single source of truth for what's being written,
  // independent of whether ComposeBox itself is currently mounted.
  const [composeBody, setComposeBody] = useState("");
  const [composeStatus, setComposeStatus] = useState<Status>("scratch");
  const [openFilename, setOpenFilename] = useState<string | null>(null);
  const [openTitle, setOpenTitle] = useState<string | null>(null);
  const [loadToken, setLoadToken] = useState(0);
  const [pendingSelection, setPendingSelection] = useState<{ start: number; end: number } | null>(
    null,
  );
  const lastSavedBodyRef = useRef("");

  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [notes, setNotes] = useState<NoteSummary[]>([]);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<Status | "all">("all");
  const [toast, setToast] = useState<string | null>(null);

  const nextReqId = useRef(0);
  const newestSeen = useRef(0);
  const debounceTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const queryRef = useRef(query);
  const filterRef = useRef(statusFilter);

  const flash = useCallback((message: string) => {
    setToast(message);
    setTimeout(() => setToast(null), 2500);
  }, []);

  // Replies are reordered by request id so a slow one cannot clobber a newer one.
  const refresh = useCallback(
    (q: string, filter: Status | "all") => {
      const id = ++nextReqId.current;
      listNotes({ status: filter === "all" ? undefined : filter, query: q || undefined })
        .then((items) => {
          if (id < newestSeen.current) return;
          newestSeen.current = id;
          setNotes(items);
        })
        .catch((e) => flash(String(e)));
    },
    [flash],
  );

  useEffect(() => {
    queryRef.current = query;
  }, [query]);
  useEffect(() => {
    filterRef.current = statusFilter;
  }, [statusFilter]);

  // ---------- compose: save / reset ----------

  const resetToBlank = useCallback(() => {
    setComposeBody("");
    setOpenFilename(null);
    setOpenTitle(null);
    setPendingSelection(null);
    lastSavedBodyRef.current = "";
    setLoadToken((t) => t + 1);
  }, []);

  // Shared by Ctrl+S, Ctrl+Enter, and the hide-triggered auto-flush. Never
  // called with an empty body — callers check that first, since an empty
  // save is always "abandon", never "overwrite with nothing".
  const commitSave = useCallback(
    (body: string): Promise<boolean> => {
      const write = openFilename ? updateNote(openFilename, body) : createNote(body, composeStatus);
      return write
        .then((note) => {
          lastSavedBodyRef.current = body;
          setOpenFilename(note.filename);
          setOpenTitle(note.title);
          return true;
        })
        .catch((e) => {
          flash(String(e));
          return false;
        });
    },
    [openFilename, composeStatus, flash],
  );

  const saveDraft = useCallback(() => {
    if (composeBody.trim() === "") {
      resetToBlank();
      return;
    }
    commitSave(composeBody).then((ok) => {
      if (ok) flash("一時保存しました");
    });
  }, [composeBody, commitSave, resetToBlank, flash]);

  const saveAndClose = useCallback(() => {
    if (composeBody.trim() === "") {
      resetToBlank();
      return;
    }
    commitSave(composeBody).then((ok) => {
      if (ok) {
        flash("保存しました");
        resetToBlank();
      }
    });
  }, [composeBody, commitSave, resetToBlank, flash]);

  // Only writes when there's something worth persisting and it differs from
  // what's already on disk — a hide that fires with nothing typed, or right
  // after an explicit save, should not touch the file again.
  const flushDraft = useCallback(() => {
    if (composeBody.trim() === "") return;
    if (composeBody === lastSavedBodyRef.current) return;
    commitSave(composeBody);
  }, [composeBody, commitSave]);

  const flushDraftRef = useRef(flushDraft);
  useEffect(() => {
    flushDraftRef.current = flushDraft;
  }, [flushDraft]);

  // Autosave: any pause in typing long enough to be "done for now" flushes
  // to disk, so forgetting to hit Ctrl+S can't lose anything.
  useEffect(() => {
    const timer = setTimeout(() => flushDraftRef.current(), AUTOSAVE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [composeBody]);

  const handleComposeStatusChange = useCallback(
    (next: Status) => {
      if (openFilename) {
        setStatus(openFilename, next)
          .then((note) => {
            setComposeStatus(note.status);
            refresh(queryRef.current.trim(), filterRef.current);
          })
          .catch((e) => flash(String(e)));
      } else {
        setComposeStatus(next);
      }
    },
    [openFilename, flash, refresh],
  );

  // A tag clicked while composing pivots straight to a topic-filtered search
  // — the same jump a tag pill in the results list offers.
  const searchForTag = useCallback((tag: string) => {
    setStatusFilter("all");
    setQuery(tag);
    setView("search");
  }, []);

  const openNoteInCompose = useCallback(
    (summary: NoteSummary) => {
      getNote(summary.filename)
        .then((note) => {
          const q = queryRef.current.trim();
          let selection: { start: number; end: number } | null = null;
          if (q) {
            const idx = note.body.toLowerCase().indexOf(q.toLowerCase());
            if (idx >= 0) selection = { start: idx, end: idx + q.length };
          }
          setComposeBody(note.body);
          setComposeStatus(note.status);
          setOpenFilename(note.filename);
          setOpenTitle(note.title);
          lastSavedBodyRef.current = note.body;
          setPendingSelection(selection);
          setLoadToken((t) => t + 1);
          setView("compose");
        })
        .catch((e) => flash(String(e)));
    },
    [flash],
  );

  // ---------- lifecycle ----------

  useEffect(() => {
    getSettings()
      .then((s) => {
        setSettingsState(s);
        setComposeStatus(s.lastStatus);
      })
      .catch((e) => flash(String(e)));
    refresh("", "all");
  }, [refresh, flash]);

  useEffect(() => {
    if (view !== "search") return;
    if (debounceTimer.current) clearTimeout(debounceTimer.current);
    debounceTimer.current = setTimeout(() => refresh(query.trim(), statusFilter), DEBOUNCE_MS);
    return () => {
      if (debounceTimer.current) clearTimeout(debounceTimer.current);
    };
  }, [query, statusFilter, view, refresh]);

  useEffect(() => {
    const unlisten = [
      listen("notes-changed", () => refresh(queryRef.current.trim(), filterRef.current)),
      // A hotkey/tray summon always lands back on the primary writing screen.
      listen("focus-compose", () => setView("compose")),
      listen("window-hiding", () => flushDraftRef.current()),
    ];
    return () => {
      unlisten.forEach((p) => p.then((off) => off()));
    };
  }, [refresh]);

  // ---------- search-screen actions ----------

  const handleToggleStatus = useCallback(
    (note: NoteSummary) => {
      const next: Status = note.status === "keep" ? "scratch" : "keep";
      setStatus(note.filename, next)
        .then((updated) => {
          if (openFilename === updated.filename) setComposeStatus(updated.status);
          refresh(queryRef.current.trim(), filterRef.current);
        })
        .catch((e) => flash(String(e)));
    },
    [openFilename, flash, refresh],
  );

  const handleDelete = useCallback(
    (filename: string) => {
      deleteNote(filename)
        .then(() => {
          if (openFilename === filename) resetToBlank();
          refresh(queryRef.current.trim(), filterRef.current);
        })
        .catch((e) => flash(String(e)));
    },
    [openFilename, resetToBlank, refresh, flash],
  );

  // ---------- keyboard ----------

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;

    if (e.ctrlKey && e.shiftKey && (e.key === "f" || e.key === "F")) {
      e.preventDefault();
      setView((v) => (v === "search" ? "compose" : "search"));
      return;
    }
    if (e.ctrlKey && e.key === ",") {
      e.preventDefault();
      setView((v) => (v === "settings" ? "compose" : "settings"));
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (view === "search" || view === "settings") {
        setView("compose");
      } else if (openFilename !== null) {
        resetToBlank();
      } else {
        hideWindow();
      }
    }
  };

  return (
    // The compose screen's chrome is tinted by which status is live, so
    // editing the wrong one is obvious at a glance rather than only
    // discoverable by reading the small switch. Search/settings stay neutral
    // — there's no single "current status" while browsing.
    <div className="app" data-status={view === "compose" ? composeStatus : undefined} onKeyDown={onKeyDown}>
      {view === "compose" && (
        <ComposeBox
          value={composeBody}
          onChange={setComposeBody}
          status={composeStatus}
          onStatusChange={handleComposeStatusChange}
          onSaveDraft={saveDraft}
          onSaveAndClose={saveAndClose}
          openTitle={openTitle}
          loadToken={loadToken}
          pendingSelection={pendingSelection}
          onTagClick={searchForTag}
          indentWidth={settings?.indentWidth ?? 4}
        />
      )}

      {view === "search" && (
        <SearchView
          notes={notes}
          query={query}
          onQueryChange={setQuery}
          statusFilter={statusFilter}
          onStatusFilterChange={setStatusFilter}
          onOpenSettings={() => setView("settings")}
          onOpenNote={openNoteInCompose}
          onToggleStatus={handleToggleStatus}
          onDelete={handleDelete}
        />
      )}

      {view === "settings" && settings && (
        <SettingsPanel
          settings={settings}
          onClose={() => setView("compose")}
          onSaved={(next) => {
            setSettingsState(next);
            setView("compose");
          }}
        />
      )}

      <div className="status">
        <span>{view === "search" ? `${notes.length.toLocaleString()} 件` : ""}</span>
        <span className="status-right">{toast ?? ""}</span>
      </div>
    </div>
  );
}
