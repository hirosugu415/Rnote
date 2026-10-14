import { useEffect, useRef } from "react";
import type { NoteSummary } from "../api";
import NoteListItem from "./NoteListItem";

interface Props {
  notes: NoteSummary[];
  selectedIndex: number;
  onOpen: (index: number) => void;
  onToggleStatus: (note: NoteSummary) => void;
  onDelete: (filename: string) => void;
  onTagClick: (tag: string) => void;
  highlight: string[];
}

export default function NoteList({
  notes,
  selectedIndex,
  onOpen,
  onToggleStatus,
  onDelete,
  onTagClick,
  highlight,
}: Props) {
  const listRef = useRef<HTMLDivElement>(null);

  // Keep the keyboard cursor in view as ArrowUp/Down walks past the visible
  // edge: scroll just far enough that the selected row is fully shown.
  useEffect(() => {
    const list = listRef.current;
    const row = list?.children[selectedIndex] as HTMLElement | undefined;
    if (!list || !row) return;
    const listBox = list.getBoundingClientRect();
    const rowBox = row.getBoundingClientRect();
    if (rowBox.top < listBox.top) {
      list.scrollTop -= listBox.top - rowBox.top;
    } else if (rowBox.bottom > listBox.bottom) {
      list.scrollTop += rowBox.bottom - listBox.bottom;
    }
  }, [selectedIndex, notes]);

  if (notes.length === 0) {
    return <div className="empty">見つかりませんでした。</div>;
  }

  return (
    <div className="list" ref={listRef}>
      {notes.map((note, index) => (
        <NoteListItem
          key={note.filename}
          note={note}
          selected={index === selectedIndex}
          onOpen={() => onOpen(index)}
          onToggleStatus={() => onToggleStatus(note)}
          onDelete={() => onDelete(note.filename)}
          onTagClick={onTagClick}
          highlight={highlight}
        />
      ))}
    </div>
  );
}
