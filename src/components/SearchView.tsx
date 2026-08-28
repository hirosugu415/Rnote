import { useEffect, useRef, useState } from "react";
import { listNotes, type NoteSummary, type Status } from "../api";
import NoteList from "./NoteList";

interface Props {
  notes: NoteSummary[];
  query: string;
  onQueryChange: (query: string) => void;
  statusFilter: Status | "all";
  onStatusFilterChange: (status: Status | "all") => void;
  onOpenSettings: () => void;
  onOpenNote: (note: NoteSummary) => void;
  onToggleStatus: (note: NoteSummary) => void;
  onDelete: (filename: string) => void;
}

interface TagCount {
  tag: string;
  count: number;
}

export default function SearchView({
  notes,
  query,
  onQueryChange,
  statusFilter,
  onStatusFilterChange,
  onOpenSettings,
  onOpenNote,
  onToggleStatus,
  onDelete,
}: Props) {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [tagCounts, setTagCounts] = useState<TagCount[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  // While an IME is composing, the input's own value must not be clobbered
  // by a re-render — only the committed text (on compositionend) should
  // trigger a search.
  const composing = useRef(false);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query, statusFilter]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // A lightweight tag browser — every tag across every note, not just the
  // current (possibly filtered) result set, so it works as an entry point
  // into a topic even before typing anything. Fetched once per visit to
  // this screen; re-entering (Ctrl+F) refreshes it for free.
  useEffect(() => {
    listNotes({})
      .then((all) => {
        const counts = new Map<string, number>();
        for (const note of all) {
          for (const tag of note.tags) {
            counts.set(tag, (counts.get(tag) ?? 0) + 1);
          }
        }
        const sorted = Array.from(counts.entries())
          .map(([tag, count]) => ({ tag, count }))
          .sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag));
        setTagCounts(sorted);
      })
      .catch(() => {});
  }, []);

  const handleTagClick = (tag: string) => {
    onStatusFilterChange("all");
    onQueryChange(tag);
    inputRef.current?.focus();
  };

  const last = notes.length - 1;

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;

    if (e.ctrlKey && (e.key === "k" || e.key === "K")) {
      e.preventDefault();
      const note = notes[selectedIndex];
      if (note) onToggleStatus(note);
      return;
    }

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setSelectedIndex((i) => Math.min(i + 1, last));
        break;
      case "ArrowUp":
        e.preventDefault();
        setSelectedIndex((i) => Math.max(i - 1, 0));
        break;
      case "Enter": {
        const note = notes[selectedIndex];
        if (note) {
          e.preventDefault();
          onOpenNote(note);
        }
        break;
      }
    }
  };

  return (
    <div className="search-view" onKeyDown={onKeyDown}>
      <div className="searchbar">
        <input
          ref={inputRef}
          className="search"
          value={query}
          spellCheck={false}
          placeholder="検索…"
          onChange={(e) => onQueryChange(e.target.value)}
          onCompositionStart={() => {
            composing.current = true;
          }}
          onCompositionEnd={(e) => {
            composing.current = false;
            onQueryChange(e.currentTarget.value);
          }}
        />
        <div className="chips">
          {(["all", "keep", "scratch"] as const).map((s) => (
            <button
              key={s}
              type="button"
              className={statusFilter === s ? "chip chip-active" : "chip"}
              onClick={() => onStatusFilterChange(s)}
            >
              {s === "all" ? "All" : s === "keep" ? "Keep" : "Scratch"}
            </button>
          ))}
        </div>
        <button className="gear" title="Settings (Ctrl+,)" onClick={onOpenSettings}>
          ⚙
        </button>
      </div>

      {query.trim() === "" && tagCounts.length > 0 && (
        <div className="tag-cloud">
          {tagCounts.map(({ tag, count }) => (
            <button
              key={tag}
              type="button"
              className="tag-pill tag-cloud-pill"
              onClick={() => handleTagClick(tag)}
            >
              #{tag} <span className="tag-count">{count}</span>
            </button>
          ))}
        </div>
      )}

      <NoteList
        notes={notes}
        selectedIndex={selectedIndex}
        onOpen={(index) => {
          const note = notes[index];
          if (note) onOpenNote(note);
        }}
        onToggleStatus={onToggleStatus}
        onDelete={onDelete}
        onTagClick={handleTagClick}
      />
    </div>
  );
}
