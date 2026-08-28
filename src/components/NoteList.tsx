import type { NoteSummary } from "../api";
import NoteListItem from "./NoteListItem";

interface Props {
  notes: NoteSummary[];
  selectedIndex: number;
  onOpen: (index: number) => void;
  onToggleStatus: (note: NoteSummary) => void;
  onDelete: (filename: string) => void;
  onTagClick: (tag: string) => void;
}

export default function NoteList({
  notes,
  selectedIndex,
  onOpen,
  onToggleStatus,
  onDelete,
  onTagClick,
}: Props) {
  if (notes.length === 0) {
    return <div className="empty">見つかりませんでした。</div>;
  }

  return (
    <div className="list">
      {notes.map((note, index) => (
        <NoteListItem
          key={note.filename}
          note={note}
          selected={index === selectedIndex}
          onOpen={() => onOpen(index)}
          onToggleStatus={() => onToggleStatus(note)}
          onDelete={() => onDelete(note.filename)}
          onTagClick={onTagClick}
        />
      ))}
    </div>
  );
}
