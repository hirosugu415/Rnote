import { useEffect, useRef, useState } from "react";
import { formatWhen, type NoteSummary } from "../api";

const CONFIRM_MS = 3000;

interface Props {
  note: NoteSummary;
  /** Highlighted because the keyboard cursor (ArrowUp/Down) is on it. */
  selected: boolean;
  onOpen: () => void;
  onToggleStatus: () => void;
  onDelete: () => void;
  onTagClick: (tag: string) => void;
}

export default function NoteListItem({
  note,
  selected,
  onOpen,
  onToggleStatus,
  onDelete,
  onTagClick,
}: Props) {
  const [confirmDelete, setConfirmDelete] = useState(false);
  const confirmTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (confirmTimer.current) clearTimeout(confirmTimer.current);
    },
    [],
  );

  const handleDeleteClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (confirmDelete) {
      if (confirmTimer.current) clearTimeout(confirmTimer.current);
      setConfirmDelete(false);
      onDelete();
      return;
    }
    setConfirmDelete(true);
    confirmTimer.current = setTimeout(() => setConfirmDelete(false), CONFIRM_MS);
  };

  return (
    <div className={selected ? "note-row note-row-selected" : "note-row"} onClick={onOpen}>
      <div className="note-row-header">
        <span className="note-title">{note.title}</span>
        {note.isExpiring && <span className="badge badge-expiring">まもなく期限切れ</span>}
        <span className="note-when">{formatWhen(note.modifiedAt)}</span>
      </div>

      <div className="note-snippet">{note.snippet || "(空)"}</div>

      {note.tags.length > 0 && (
        <div className="note-tags">
          {note.tags.map((t) => (
            <button
              key={t}
              type="button"
              className="tag-pill"
              onClick={(e) => {
                e.stopPropagation();
                onTagClick(t);
              }}
            >
              #{t}
            </button>
          ))}
        </div>
      )}

      <div className="note-row-actions">
        <button
          type="button"
          className="row-action"
          onClick={(e) => {
            e.stopPropagation();
            onToggleStatus();
          }}
        >
          {note.status === "keep" ? "→ Scratch" : "→ Keep"}
        </button>
        <button
          type="button"
          className={confirmDelete ? "row-action row-action-danger" : "row-action"}
          onClick={handleDeleteClick}
        >
          {confirmDelete ? "もう一度クリックで削除" : "削除"}
        </button>
      </div>
    </div>
  );
}
