import { useEffect, useRef, useState } from "react";
import { isTagChar, type TagCount } from "../api";

/** How many of the most-used tags the browser shows before "+N". */
const COLLAPSED_COUNT = 10;
const EXPANDED_KEY = "rnote.tagCloudExpanded";

function loadExpanded(): boolean {
  try {
    return localStorage.getItem(EXPANDED_KEY) === "1";
  } catch {
    return false;
  }
}

/** What the user typed as a new tag name, normalised the way the backend
 *  will: surrounding space and a leading `#` dropped. */
function normalise(name: string): string {
  return name.trim().replace(/^#+/, "");
}

function isValidTagName(name: string): boolean {
  return name.length > 0 && Array.from(name).every(isTagChar);
}

interface Props {
  /** Every tag across every note, most used first. */
  tags: TagCount[];
  onTagClick: (tag: string) => void;
  /** Rewrites `#from` to `#to` in every note; resolves to an error message,
   *  or null on success. */
  onRenameTag: (from: string, to: string) => Promise<string | null>;
}

/**
 * The search screen's tag browser — every tag across every note, so it works
 * as an entry point into a topic before anything is typed. Collapsed to the
 * most-used few by default so a long tail of one-off tags doesn't bury the
 * list (the open/closed choice is remembered). "編集" switches to rename
 * mode: click a tag, type the new name, Enter. Renaming onto a tag that
 * already exists merges the two.
 */
export default function TagCloud({ tags, onTagClick, onRenameTag }: Props) {
  const [expanded, setExpanded] = useState(loadExpanded);
  const [editMode, setEditMode] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Mirrors `busy` for the blur handler: React swaps in new event handlers
   *  only after it has updated the DOM, so a blur fired mid-update would
   *  still see the old `busy` and cancel the rename it just started. */
  const busyRef = useRef(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const composing = useRef(false);

  useEffect(() => {
    if (editing !== null) inputRef.current?.select();
  }, [editing]);

  const toggleExpanded = () => {
    setExpanded((open) => {
      try {
        localStorage.setItem(EXPANDED_KEY, open ? "0" : "1");
      } catch {
        // per-viewer convenience only
      }
      return !open;
    });
  };

  const startEdit = (tag: string) => {
    setEditing(tag);
    setDraft(tag);
    setError(null);
  };

  const cancelEdit = () => {
    setEditing(null);
    setDraft("");
    setError(null);
  };

  const next = normalise(draft);
  const valid = isValidTagName(next);
  const mergeTarget =
    editing !== null && next !== editing ? tags.find((t) => t.tag === next) : undefined;

  const commit = () => {
    if (editing === null || busyRef.current) return;
    if (next === editing) {
      cancelEdit();
      return;
    }
    if (!valid) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    onRenameTag(editing, next)
      .catch((e) => String(e))
      .then((err) => {
        busyRef.current = false;
        setBusy(false);
        if (err === null) {
          cancelEdit();
        } else {
          // Stay in the field with the reason shown, ready to fix and retry.
          setError(err);
          requestAnimationFrame(() => inputRef.current?.focus());
        }
      });
  };

  // Edit mode lists everything — a tag you want to fix is often a rare one.
  const showAll = expanded || editMode;
  const hiddenCount = Math.max(0, tags.length - COLLAPSED_COUNT);
  const visible = showAll ? tags : tags.slice(0, COLLAPSED_COUNT);

  return (
    <div className={editMode ? "tag-cloud tag-cloud-editing" : "tag-cloud"}>
      {visible.map(({ tag, count }) =>
        editing === tag ? (
          <span key={tag} className="tag-rename">
            <span className="tag-rename-hash">#</span>
            <input
              ref={inputRef}
              className="tag-rename-input"
              value={draft}
              // readOnly, not disabled: disabling drops focus (the caret
              // vanished mid-rename and Enter went nowhere afterwards).
              readOnly={busy}
              spellCheck={false}
              onChange={(e) => {
                setDraft(e.target.value);
                setError(null);
              }}
              onCompositionStart={() => {
                composing.current = true;
              }}
              onCompositionEnd={() => {
                composing.current = false;
              }}
              onBlur={() => {
                if (!busyRef.current) cancelEdit();
              }}
              onKeyDown={(e) => {
                // Keep Enter/Esc/arrows away from the search screen (which
                // would open a result or leave the screen).
                e.stopPropagation();
                // 229: WebView2 can deliver the IME's confirming Enter after
                // compositionend, flagged only by this keyCode.
                if (composing.current || e.nativeEvent.isComposing || e.keyCode === 229) return;
                if (e.key === "Enter") {
                  e.preventDefault();
                  commit();
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  cancelEdit();
                }
              }}
            />
            <span className={error ? "tag-rename-hint tag-rename-error" : "tag-rename-hint"}>
              {error ??
                (busy
                  ? "変更中…"
                  : !valid
                    ? "文字・数字・_・- のみ"
                    : mergeTarget
                      ? `既存の #${mergeTarget.tag} と統合（Enter で ${count}件を変更）`
                      : `Enter で ${count}件のメモを変更`)}
            </span>
          </span>
        ) : (
          <button
            key={tag}
            type="button"
            className="tag-pill tag-cloud-pill"
            title={editMode ? "クリックして名前を変更" : undefined}
            onClick={() => (editMode ? startEdit(tag) : onTagClick(tag))}
          >
            {editMode && <span className="tag-edit-mark">✎ </span>}#{tag}{" "}
            <span className="tag-count">{count}</span>
          </button>
        ),
      )}
      {!editMode && hiddenCount > 0 && (
        <button type="button" className="tag-cloud-more" onClick={toggleExpanded}>
          {expanded ? "たたむ" : `+${hiddenCount}`}
        </button>
      )}
      <button
        type="button"
        className="tag-cloud-more tag-cloud-edit"
        onClick={() => {
          cancelEdit();
          setEditMode((m) => !m);
        }}
      >
        {editMode ? "完了" : "編集"}
      </button>
    </div>
  );
}
