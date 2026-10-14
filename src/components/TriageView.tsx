import { useCallback, useEffect, useRef, useState } from "react";
import { getNote, listNotes, type Note, type NoteSummary } from "../api";

type Verdict = "keep" | "trash" | "skip";
type Scope = "expiring" | "all";

interface Props {
  /** Moves a scratch note to Keep; resolves to the moved note, or null. */
  onKeep: (note: NoteSummary) => Promise<NoteSummary | null>;
  /** Moves a kept note back to Scratch (undo of a Keep). */
  onUnkeep: (note: NoteSummary) => Promise<NoteSummary | null>;
  /** Sends a note to the Recycle Bin; resolves true on success. */
  onTrash: (filename: string) => Promise<boolean>;
  /** Leaves triage and opens the note for editing. */
  onOpen: (note: NoteSummary) => void;
  onExit: () => void;
}

/** Expiring first, then oldest first — the notes most in need of a decision
 *  come up before the ones that can still wait. */
function triageOrder(a: NoteSummary, b: NoteSummary): number {
  if (a.isExpiring !== b.isExpiring) return a.isExpiring ? -1 : 1;
  return a.createdAt.localeCompare(b.createdAt);
}

function daysSince(iso: string): number {
  const t = new Date(iso).getTime();
  return Number.isNaN(t) ? 0 : Math.max(0, Math.floor((Date.now() - t) / 86_400_000));
}

function formatDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ""
    : d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

/**
 * Scratch triage: walks the scratch notes one at a time, full text on
 * screen, and files each with a single key — K keep, D recycle bin, S later,
 * E open to edit, ← back. The Keep/Scratch split only pays off if scratch
 * actually gets emptied; the "expiring" badge alone never prompted that.
 */
export default function TriageView({ onKeep, onUnkeep, onTrash, onOpen, onExit }: Props) {
  const [scope, setScope] = useState<Scope>("expiring");
  const [queue, setQueue] = useState<NoteSummary[] | null>(null);
  const [index, setIndex] = useState(0);
  /** What happened to each queue slot, by position. `keep` slots also hold
   *  the moved note (its filename may change on the move) for undo. */
  const [verdicts, setVerdicts] = useState<{ verdict: Verdict; moved?: NoteSummary }[]>([]);
  const [current, setCurrent] = useState<Note | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  const start = useCallback((nextScope: Scope, only?: NoteSummary[]) => {
    const load = only ? Promise.resolve(only) : listNotes({ status: "scratch" });
    load
      .then((all) => {
        let items = [...all].sort(triageOrder);
        // "Expiring" with nothing expiring would be an empty screen; fall
        // back to every scratch note so there's still something to tidy.
        if (!only && nextScope === "expiring") {
          const expiring = items.filter((n) => n.isExpiring);
          if (expiring.length > 0) items = expiring;
          else nextScope = "all";
        }
        setScope(nextScope);
        setQueue(items);
        setIndex(0);
        setVerdicts([]);
        setMessage(null);
      })
      .catch((e) => setMessage(String(e)));
  }, []);

  useEffect(() => {
    start("expiring");
    rootRef.current?.focus();
  }, [start]);

  const item = queue && index < queue.length ? queue[index] : null;

  useEffect(() => {
    setCurrent(null);
    if (!item) return;
    let live = true;
    getNote(item.filename)
      .then((note) => live && setCurrent(note))
      .catch(() => live && setCurrent(null));
    return () => {
      live = false;
    };
  }, [item]);

  const record = (verdict: Verdict, moved?: NoteSummary) => {
    setVerdicts((v) => {
      const next = [...v];
      next[index] = { verdict, moved };
      return next;
    });
    setIndex((i) => i + 1);
  };

  const act = (verdict: Verdict) => {
    if (!item || busy) return;
    setMessage(null);
    if (verdict === "skip") {
      record("skip");
      return;
    }
    setBusy(true);
    const run =
      verdict === "keep"
        ? onKeep(item).then((moved) => moved && record("keep", moved))
        : onTrash(item.filename).then((ok) => ok && record("trash"));
    run.finally(() => setBusy(false));
  };

  const back = () => {
    if (busy || index === 0) return;
    const prev = verdicts[index - 1];
    if (prev?.verdict === "trash") {
      setMessage("ゴミ箱に送ったメモはここでは戻せません（Windows のごみ箱から復元できます）");
      return;
    }
    setMessage(null);
    if (prev?.verdict === "keep" && prev.moved) {
      // Stepping back over a Keep undoes it, so the note can be re-decided.
      setBusy(true);
      onUnkeep(prev.moved)
        .then((restored) => {
          if (!restored || !queue) return;
          setQueue(queue.map((n, i) => (i === index - 1 ? restored : n)));
          setIndex((i) => i - 1);
        })
        .finally(() => setBusy(false));
      return;
    }
    setIndex((i) => i - 1);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.ctrlKey || e.altKey || e.metaKey || e.nativeEvent.isComposing) return;
    // Enter/Space on a focused button is that button's click — don't also
    // treat it as "open".
    if ((e.target as HTMLElement).tagName === "BUTTON" && (e.key === "Enter" || e.key === " ")) {
      return;
    }
    const key = e.key.toLowerCase();
    let handled = true;
    if (key === "k") act("keep");
    else if (key === "d" || e.key === "Delete") act("trash");
    else if (key === "s" || e.key === "ArrowRight") act("skip");
    else if (e.key === "ArrowLeft") back();
    else if ((key === "e" || e.key === "Enter") && item) onOpen(item);
    else handled = false;
    if (handled) e.preventDefault();
  };

  const counts = { keep: 0, trash: 0, skip: 0 };
  for (const v of verdicts) if (v) counts[v.verdict]++;
  const skipped = (queue ?? []).filter((_, i) => verdicts[i]?.verdict === "skip");

  return (
    <div className="triage" ref={rootRef} tabIndex={-1} onKeyDown={onKeyDown}>
      <div className="view-title triage-title" data-drag-region>
        <span>Scratch 整理</span>
        {queue && queue.length > 0 && (
          <span className="triage-progress">
            {Math.min(index + 1, queue.length)} / {queue.length}
          </span>
        )}
      </div>
      <div className="triage-toolbar">
        <div className="chips">
          {(["expiring", "all"] as const).map((s) => (
            <button
              key={s}
              type="button"
              className={scope === s ? "chip chip-active" : "chip"}
              onClick={() => {
                start(s);
                rootRef.current?.focus();
              }}
            >
              {s === "expiring" ? "期限切れ" : "すべての Scratch"}
            </button>
          ))}
        </div>
        <button type="button" className="chip" onClick={onExit}>
          終了 (Esc)
        </button>
      </div>

      {queue === null ? (
        <div className="empty">読み込み中…</div>
      ) : queue.length === 0 ? (
        <div className="empty">Scratch は空です。整理するものはありません。</div>
      ) : item ? (
        <>
          <div className="triage-card">
            <div className="triage-card-header">
              <span className="triage-card-title">{item.title}</span>
              {item.isExpiring && <span className="badge badge-expiring">期限切れ</span>}
            </div>
            <div className="triage-meta">
              {formatDate(item.createdAt)} 作成 ・ {daysSince(item.createdAt)}日経過 ・{" "}
              {item.charCount.toLocaleString()}文字
              {item.tags.length > 0 && <> ・ {item.tags.map((t) => `#${t}`).join(" ")}</>}
            </div>
            <div className="triage-body">{current ? current.body : "…"}</div>
          </div>
          {message && <div className="triage-message">{message}</div>}
          <div className="triage-actions">
            <button type="button" className="triage-btn triage-keep" disabled={busy} onClick={() => act("keep")}>
              <kbd>K</kbd> Keep に残す
            </button>
            <button type="button" className="triage-btn triage-trash" disabled={busy} onClick={() => act("trash")}>
              <kbd>D</kbd> ゴミ箱へ
            </button>
            <button type="button" className="triage-btn" disabled={busy} onClick={() => act("skip")}>
              <kbd>S</kbd> あとで
            </button>
            <button type="button" className="triage-btn" disabled={busy} onClick={() => onOpen(item)}>
              <kbd>E</kbd> 開いて編集
            </button>
            <button type="button" className="triage-btn" disabled={busy || index === 0} onClick={back}>
              <kbd>←</kbd> 戻る
            </button>
          </div>
        </>
      ) : (
        <div className="triage-done">
          <div className="triage-done-title">整理完了</div>
          <div className="triage-done-counts">
            Keep {counts.keep}件 ・ ゴミ箱 {counts.trash}件 ・ あとで {counts.skip}件
          </div>
          {message && <div className="triage-message">{message}</div>}
          <div className="triage-actions">
            {skipped.length > 0 && (
              <button
                type="button"
                className="triage-btn"
                onClick={() => {
                  start(scope, skipped);
                  rootRef.current?.focus();
                }}
              >
                「あとで」の{skipped.length}件をもう一度
              </button>
            )}
            <button type="button" className="triage-btn" onClick={back} disabled={index === 0}>
              <kbd>←</kbd> 最後の1件に戻る
            </button>
            <button type="button" className="triage-btn" onClick={onExit}>
              終了
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
