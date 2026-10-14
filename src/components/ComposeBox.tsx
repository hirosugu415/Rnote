import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { extractTags, isTagChar, type Status, type TagCount } from "../api";

interface Props {
  value: string;
  onChange: (value: string) => void;
  status: Status;
  /** Called for both the button click and Ctrl+K — App decides whether that
   *  means "change what a new note will be saved as" or "move an already
   *  saved note right now", depending on whether one is open. */
  onStatusChange: (status: Status) => void;
  onSaveDraft: () => void;
  onSaveAndClose: () => void;
  /** Title of the note currently open for editing, or null for a fresh one. */
  openTitle: string | null;
  /** Bumped by App every time it loads a (possibly different) note in —
   *  including the initial mount — so the caret always lands somewhere
   *  sensible without the effect depending on the value itself. */
  loadToken: number;
  pendingSelection: { start: number; end: number } | null;
  /** Jumps to the search screen pre-filtered to this tag — same topic-pivot
   *  a tag pill in the search results list offers. */
  onTagClick: (tag: string) => void;
  /** Spaces per indent level for Ctrl+]/Ctrl+[. */
  indentWidth: number;
  /** Existing tags offered as completions after typing `#`, most used first. */
  allTags: TagCount[];
}

/** Most completions shown at once. */
const SUGGEST_LIMIT = 8;

/** The `#tag` being typed at `caret`, if any: the caret sits at the end of a
 *  run of tag characters directly preceded by a `#` that starts a word —
 *  the same rule `extractTags` uses to recognise a tag. `partial` may be
 *  empty (just typed `#`). */
function tagTrigger(value: string, caret: number): { hash: number; partial: string } | null {
  let i = caret;
  while (i > 0 && isTagChar(value[i - 1])) i--;
  if (i === 0 || value[i - 1] !== "#") return null;
  const hash = i - 1;
  if (hash > 0 && isTagChar(value[hash - 1])) return null;
  if (caret < value.length && isTagChar(value[caret])) return null;
  return { hash, partial: value.slice(i, caret) };
}

/** Styles that decide where text wraps, copied onto the measuring mirror. */
const MIRROR_PROPS = [
  "boxSizing", "width", "borderTopWidth", "borderRightWidth", "borderBottomWidth",
  "borderLeftWidth", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft",
  "fontStyle", "fontVariant", "fontWeight", "fontStretch", "fontSize", "lineHeight",
  "fontFamily", "letterSpacing", "wordSpacing", "tabSize", "textIndent", "textTransform",
] as const;

/** Pixel position of character `pos` inside the textarea's content (before
 *  scrolling), found by laying the text out in a hidden copy of the box —
 *  textareas have no API for caret coordinates. */
function caretCoords(el: HTMLTextAreaElement, pos: number) {
  const style = getComputedStyle(el);
  const mirror = document.createElement("div");
  for (const prop of MIRROR_PROPS) mirror.style[prop] = style[prop];
  mirror.style.position = "absolute";
  mirror.style.visibility = "hidden";
  mirror.style.whiteSpace = "pre-wrap";
  mirror.style.overflowWrap = "break-word";
  mirror.style.overflow = "hidden";
  mirror.style.width = `${el.clientWidth + parseFloat(style.borderLeftWidth) + parseFloat(style.borderRightWidth)}px`;
  mirror.textContent = el.value.slice(0, pos);
  const marker = document.createElement("span");
  marker.textContent = "\u200b";
  mirror.appendChild(marker);
  document.body.appendChild(mirror);
  const coords = {
    top: marker.offsetTop,
    left: marker.offsetLeft,
    height: parseFloat(style.lineHeight) || parseFloat(style.fontSize) * 1.6,
  };
  document.body.removeChild(mirror);
  return coords;
}

/** Start/end of the line(s) fully spanning `[start, end]`, VSCode-style —
 *  used so indent/outdent/move-line act on whole lines even when the
 *  selection only partially covers the first/last one. */
function lineRange(value: string, start: number, end: number) {
  const lineStart = value.lastIndexOf("\n", Math.max(0, start - 1)) + 1;
  const idx = value.indexOf("\n", end);
  const lineEnd = idx === -1 ? value.length : idx;
  return { lineStart, lineEnd };
}

/** Shifts every line touched by the selection one indent level in `direction`
 *  (+1 = Ctrl+], -1 = Ctrl+[), remapping the selection to track the same
 *  characters afterward. Outdent strips a leading tab, else up to `width`
 *  leading spaces, per line — never past what's actually there. */
function shiftIndent(
  value: string,
  start: number,
  end: number,
  width: number,
  direction: 1 | -1,
) {
  const { lineStart, lineEnd } = lineRange(value, start, end);
  const before = value.slice(0, lineStart);
  const after = value.slice(lineEnd);
  const lines = value.slice(lineStart, lineEnd).split("\n");

  const deltas: number[] = [];
  const newLines = lines.map((line) => {
    if (direction === 1) {
      deltas.push(width);
      return " ".repeat(width) + line;
    }
    let remove = 0;
    if (line[0] === "\t") remove = 1;
    else while (remove < width && line[remove] === " ") remove++;
    deltas.push(-remove);
    return line.slice(remove);
  });

  const newValue = before + newLines.join("\n") + after;

  const mapPos = (pos: number) => {
    const local = pos - lineStart;
    let offset = 0;
    let newOffset = 0;
    for (let i = 0; i < lines.length; i++) {
      const lineLen = lines[i].length;
      if (i === lines.length - 1 || local <= offset + lineLen) {
        return lineStart + newOffset + Math.max(0, local - offset + deltas[i]);
      }
      offset += lineLen + 1;
      newOffset += lineLen + deltas[i] + 1;
    }
    return lineStart + newOffset;
  };

  return { newValue, newStart: mapPos(start), newEnd: mapPos(end) };
}

/** Swaps the line(s) touched by the selection with the adjacent line in
 *  `direction` (-1 = Alt+Up, +1 = Alt+Down). Returns null at a document
 *  edge, where there's nothing to swap with. */
function moveLines(value: string, start: number, end: number, direction: -1 | 1) {
  const { lineStart, lineEnd } = lineRange(value, start, end);
  const block = value.slice(lineStart, lineEnd);

  if (direction === -1) {
    if (lineStart === 0) return null;
    const prevStart = value.lastIndexOf("\n", lineStart - 2) + 1;
    const prevLine = value.slice(prevStart, lineStart - 1);
    const newValue = value.slice(0, prevStart) + block + "\n" + prevLine + value.slice(lineEnd);
    const delta = -(prevLine.length + 1);
    return { newValue, newStart: start + delta, newEnd: end + delta };
  }

  if (lineEnd === value.length) return null;
  const nextIdx = value.indexOf("\n", lineEnd + 1);
  const nextEnd = nextIdx === -1 ? value.length : nextIdx;
  const nextLine = value.slice(lineEnd + 1, nextEnd);
  const newValue = value.slice(0, lineStart) + nextLine + "\n" + block + value.slice(nextEnd);
  const delta = nextLine.length + 1;
  return { newValue, newStart: start + delta, newEnd: end + delta };
}

/** Duplicates the line(s) touched by the selection (Shift+Alt+Up/Down). The
 *  selection follows whichever copy is "new": unchanged for Up (the new copy
 *  lands exactly where the old one was, on top), shifted past the first copy
 *  for Down (the new copy is the one below). */
function duplicateLines(value: string, start: number, end: number, direction: -1 | 1) {
  const { lineStart, lineEnd } = lineRange(value, start, end);
  const block = value.slice(lineStart, lineEnd);
  const newValue = value.slice(0, lineStart) + block + "\n" + block + value.slice(lineEnd);
  if (direction === -1) return { newValue, newStart: start, newEnd: end };
  const delta = block.length + 1;
  return { newValue, newStart: start + delta, newEnd: end + delta };
}

/** Case-insensitive occurrences of `query` in `value`, for the in-note find
 *  bar's match count. */
function countMatches(value: string, query: string): number {
  if (!query) return 0;
  const hay = value.toLowerCase();
  const needle = query.toLowerCase();
  let count = 0;
  let idx = hay.indexOf(needle);
  while (idx !== -1) {
    count++;
    idx = hay.indexOf(needle, idx + needle.length);
  }
  return count;
}

/** First case-insensitive match of `query` in `hay` at or after `from`,
 *  wrapping to the start if none is found there. */
function findNext(hay: string, from: number, query: string): number | null {
  if (!query) return null;
  const h = hay.toLowerCase();
  const needle = query.toLowerCase();
  let idx = h.indexOf(needle, from);
  if (idx === -1) idx = h.indexOf(needle, 0);
  return idx === -1 ? null : idx;
}

/** Last case-insensitive match of `query` in `hay` at or before `before`,
 *  wrapping to the end if none is found there. */
function findPrev(hay: string, before: number, query: string): number | null {
  if (!query) return null;
  const h = hay.toLowerCase();
  const needle = query.toLowerCase();
  let idx = h.lastIndexOf(needle, before);
  if (idx === -1) idx = h.lastIndexOf(needle);
  return idx === -1 ? null : idx;
}

/** Replaces every case-insensitive occurrence of `query` with `replacement`. */
function replaceAllOccurrences(
  value: string,
  query: string,
  replacement: string,
): { newValue: string; count: number } {
  if (!query) return { newValue: value, count: 0 };
  const hay = value.toLowerCase();
  const needle = query.toLowerCase();
  let result = "";
  let cursor = 0;
  let count = 0;
  let idx = hay.indexOf(needle, cursor);
  while (idx !== -1) {
    result += value.slice(cursor, idx) + replacement;
    cursor = idx + query.length;
    count++;
    idx = hay.indexOf(needle, cursor);
  }
  result += value.slice(cursor);
  return { newValue: result, count };
}

/**
 * The whole point of this screen: writing. Enter is a plain newline — saving
 * is Ctrl+S (draft) or Ctrl+Enter (save and close) only. The draft is never
 * cleared by anything other than App explicitly deciding to reset it; hiding
 * the window does not touch it (Tauri's `hide()` preserves DOM state).
 */
export default function ComposeBox({
  value,
  onChange,
  status,
  onStatusChange,
  onSaveDraft,
  onSaveAndClose,
  openTitle,
  loadToken,
  pendingSelection,
  onTagClick,
  indentWidth,
  allTags,
}: Props) {
  const textRef = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false);
  // A live preview only — the file on disk is always re-tagged from its
  // actual saved body by the backend, this never has to stay in sync with it.
  const liveTags = useMemo(() => extractTags(value), [value]);

  // ---------- in-note find & replace (Ctrl+F / Ctrl+H) ----------
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [replaceOpen, setReplaceOpen] = useState(false);
  const [replaceText, setReplaceText] = useState("");
  const findAnchor = useRef(0);
  const findInputRef = useRef<HTMLInputElement>(null);
  const replaceInputRef = useRef<HTMLInputElement>(null);
  // Shared by both find and replace inputs — only one can be composing at a
  // time, so one ref is enough to guard both against IME's confirming Enter
  // being misread as "jump to match" (same problem the main textarea guards
  // against with `composing`, just on a different field).
  const findComposing = useRef(false);
  const matchCount = useMemo(() => countMatches(value, findQuery), [value, findQuery]);

  // setSelectionRange works on an unfocused textarea (Chromium still shows a
  // grey highlight), so the match can be revealed without moving focus off
  // the find/replace input. An earlier version briefly focused the textarea
  // and focused back — that's a same-tick round trip in a plain browser, but
  // in the real Tauri window (WebView2) the two focus() calls cross a native
  // OS message-pump boundary and aren't guaranteed to land in order; focus
  // could get stuck on the textarea, silently swallowing every keystroke
  // after the first live-search hit — including Enter. Scrolling by hand
  // avoids needing focus at all.
  const scrollMatchIntoView = (idx: number) => {
    const el = textRef.current;
    if (!el) return;
    const lineHeight = parseFloat(getComputedStyle(el).lineHeight) || 20;
    const line = value.slice(0, idx).split("\n").length - 1;
    const target = line * lineHeight - el.clientHeight / 2 + lineHeight;
    el.scrollTop = Math.max(0, target);
  };

  const jumpTo = (idx: number, length: number) => {
    const el = textRef.current;
    if (!el) return;
    el.setSelectionRange(idx, idx + length);
    scrollMatchIntoView(idx);
  };

  const jumpForward = (from: number, query: string, hay: string = value) => {
    const idx = findNext(hay, from, query);
    if (idx !== null) jumpTo(idx, query.length);
  };

  const jumpBackward = (before: number, query: string, hay: string = value) => {
    const idx = findPrev(hay, before, query);
    if (idx !== null) jumpTo(idx, query.length);
  };

  // Live search: re-run from the caret position captured when the bar was
  // opened, not from the last match — otherwise every keystroke would chase
  // whatever the previous, shorter query happened to land on. Skipped
  // mid-IME composition so it doesn't act on a not-yet-committed query.
  useEffect(() => {
    if (!findOpen || !findQuery || findComposing.current) return;
    jumpForward(findAnchor.current, findQuery);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [findQuery, findOpen]);

  const openFind = (withReplace: boolean) => {
    if (!findOpen) {
      findAnchor.current = textRef.current?.selectionStart ?? 0;
      setFindQuery("");
      setFindOpen(true);
    }
    if (withReplace) setReplaceOpen(true);
    requestAnimationFrame(() => (withReplace ? replaceInputRef : findInputRef).current?.select());
  };

  const closeFind = () => {
    setFindOpen(false);
    textRef.current?.focus();
  };

  // Ctrl+F/Ctrl+H are handled identically from the textarea and from inside
  // the find/replace inputs themselves — without this, a second Ctrl+F press
  // while focus was already in the find box fell through to the browser's
  // own find-in-page instead of being caught here.
  const handleFindHotkeys = (e: React.KeyboardEvent): boolean => {
    if (e.ctrlKey && !e.shiftKey && (e.key === "f" || e.key === "F")) {
      e.preventDefault();
      if (findOpen) closeFind();
      else openFind(false);
      return true;
    }
    if (e.ctrlKey && (e.key === "h" || e.key === "H")) {
      e.preventDefault();
      openFind(true);
      return true;
    }
    return false;
  };

  // Replaces the current match and advances to the next one. If the caret
  // isn't already sitting on a match (nothing jumped to yet), the first
  // press just jumps there instead of blindly overwriting the selection —
  // same "reveal, then act" two-step VSCode's Replace button uses.
  const replaceCurrent = () => {
    const el = textRef.current;
    if (!el || !findQuery) return;
    const selStart = el.selectionStart;
    const selEnd = el.selectionEnd;
    const selected = value.slice(selStart, selEnd);
    if (selected.toLowerCase() !== findQuery.toLowerCase()) {
      jumpForward(findAnchor.current, findQuery);
      return;
    }
    const newValue = value.slice(0, selStart) + replaceText + value.slice(selEnd);
    const newPos = selStart + replaceText.length;
    applyEdit({ newValue, newStart: newPos, newEnd: newPos });
    jumpForward(newPos, findQuery, newValue);
  };

  const replaceAll = () => {
    if (!findQuery) return;
    const { newValue, count } = replaceAllOccurrences(value, findQuery, replaceText);
    if (count === 0) return;
    applyEdit({ newValue, newStart: 0, newEnd: 0 });
  };

  useEffect(() => {
    const el = textRef.current;
    if (!el) return;
    el.focus();
    if (pendingSelection) {
      el.setSelectionRange(pendingSelection.start, pendingSelection.end);
    } else {
      el.setSelectionRange(0, 0);
    }
    // Only `loadToken` should re-trigger this — it changes exactly when App
    // wants the caret repositioned, unlike `value` which changes every
    // keystroke.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadToken]);

  // Programmatic edits (indent/outdent/move-line/replace) go through
  // `execCommand("insertText")` when the textarea has focus: unlike assigning
  // `el.value`, that lands on the browser's native undo stack, so Ctrl+Z
  // reverts the edit in one step instead of the history being wiped. Only
  // the span that actually differs is replaced, keeping the undo step tight.
  // Without focus (replace clicked from the find bar — see the note on
  // `scrollMatchIntoView` for why we don't steal it) or if the command is
  // unavailable, falls back to writing the DOM node directly before
  // onChange: React skips resetting an input's selection when the incoming
  // value already matches the node's, which keeps the caret in place.
  const applyEdit = (edit: { newValue: string; newStart: number; newEnd: number } | null) => {
    if (!edit) return;
    const el = textRef.current;
    if (!el) {
      onChange(edit.newValue);
      return;
    }
    const old = el.value;
    const next = edit.newValue;
    if (document.activeElement === el && old !== next) {
      let prefix = 0;
      const maxPrefix = Math.min(old.length, next.length);
      while (prefix < maxPrefix && old[prefix] === next[prefix]) prefix++;
      let suffix = 0;
      const maxSuffix = maxPrefix - prefix;
      while (
        suffix < maxSuffix &&
        old[old.length - 1 - suffix] === next[next.length - 1 - suffix]
      ) {
        suffix++;
      }
      el.setSelectionRange(prefix, old.length - suffix);
      const inserted = next.slice(prefix, next.length - suffix);
      const ok = inserted
        ? document.execCommand("insertText", false, inserted)
        : document.execCommand("delete", false);
      if (ok && el.value === next) {
        el.setSelectionRange(edit.newStart, edit.newEnd);
        onChange(next);
        return;
      }
    }
    el.value = next;
    el.setSelectionRange(edit.newStart, edit.newEnd);
    onChange(next);
  };

  // ---------- `#` tag completion ----------
  // Typing `#` (optionally followed by the start of a name) offers the tags
  // already in use, so the same topic doesn't splinter into near-duplicate
  // tags you forgot you had. Prefix matches first, then substring matches,
  // each group most-used first (the order `allTags` arrives in).
  const containerRef = useRef<HTMLDivElement>(null);
  const [sel, setSel] = useState({ start: 0, end: 0 });
  const [focused, setFocused] = useState(false);
  const [suggestIndex, setSuggestIndex] = useState(0);
  /** `#` position whose popup was closed (Esc / accepted) — stays closed
   *  until the caret leaves that tag. */
  const [dismissedHash, setDismissedHash] = useState<number | null>(null);
  const [popupPos, setPopupPos] = useState<React.CSSProperties | null>(null);
  /** Bumped on textarea scroll, so the popup re-anchors to the moved caret. */
  const [scrollTick, setScrollTick] = useState(0);

  const syncSel = () => {
    const el = textRef.current;
    if (el) setSel({ start: el.selectionStart, end: el.selectionEnd });
  };

  const trigger = sel.start === sel.end ? tagTrigger(value, sel.start) : null;

  const suggestions = useMemo(() => {
    if (!trigger || !focused || trigger.hash === dismissedHash) return [];
    const p = trigger.partial.toLowerCase();
    const prefix: TagCount[] = [];
    const contains: TagCount[] = [];
    for (const t of allTags) {
      const lower = t.tag.toLowerCase();
      if (lower.startsWith(p)) prefix.push(t);
      else if (lower.includes(p)) contains.push(t);
    }
    const list = [...prefix, ...contains].slice(0, SUGGEST_LIMIT);
    // Already typed in full and nothing longer to offer: nothing to say.
    if (list.length === 1 && list[0].tag === trigger.partial) return [];
    return list;
  }, [trigger?.hash, trigger?.partial, focused, dismissedHash, allTags]);

  const open = suggestions.length > 0;

  useEffect(() => {
    setSuggestIndex(0);
    if (dismissedHash !== null && trigger?.hash !== dismissedHash) setDismissedHash(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [trigger?.hash, trigger?.partial]);

  // Anchor the popup just under the caret, flipping above it when there's
  // no room below, and kept inside the compose area horizontally.
  useLayoutEffect(() => {
    const el = textRef.current;
    const box = containerRef.current;
    if (!open || !el || !box || !trigger) {
      setPopupPos(null);
      return;
    }
    const c = caretCoords(el, trigger.hash);
    const lineTop = el.offsetTop + c.top - el.scrollTop;
    const left = Math.max(8, Math.min(el.offsetLeft + c.left, box.clientWidth - 200));
    const below = lineTop + c.height + 2;
    setPopupPos(
      below + 240 <= box.clientHeight
        ? { left, top: below }
        : { left, bottom: box.clientHeight - lineTop + 2 },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, trigger?.hash, value, scrollTick]);

  const acceptSuggestion = (tag: string) => {
    const el = textRef.current;
    if (!el || !trigger) return;
    const start = trigger.hash + 1;
    const end = sel.start;
    const next = value[end];
    // A trailing space ends the tag, so the next word isn't glued onto it.
    const insert = next === undefined || !/\s/.test(next) ? `${tag} ` : tag;
    const caret = start + insert.length;
    applyEdit({
      newValue: value.slice(0, start) + insert + value.slice(end),
      newStart: caret,
      newEnd: caret,
    });
    setDismissedHash(trigger.hash);
    syncSel();
  };

  /** Keys the open popup claims; returns true when it handled the event. */
  const handleSuggestKeys = (e: React.KeyboardEvent): boolean => {
    if (!open || !trigger || e.altKey || e.ctrlKey || e.metaKey) return false;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setSuggestIndex((i) => (i + step + suggestions.length) % suggestions.length);
      return true;
    }
    // While the popup is showing, Enter always picks the highlighted tag —
    // a newline there would read as the popup ignoring you. (Esc first if a
    // newline is really what you want.)
    if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
      e.preventDefault();
      acceptSuggestion(suggestions[suggestIndex].tag);
      return true;
    }
    if (e.key === "Escape") {
      // Closing the popup is this Escape's whole job — don't also let App
      // treat it as "leave this note".
      e.preventDefault();
      e.stopPropagation();
      setDismissedHash(trigger.hash);
      return true;
    }
    return false;
  };

  return (
    <div className="compose" ref={containerRef}>
      <div className="compose-open-title" data-drag-region>
        {openTitle ? `編集中: ${openTitle}` : "新規メモ"}
      </div>
      <textarea
        ref={textRef}
        id="compose-input"
        className="compose-input"
        placeholder="なにか書く…"
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          setSel({ start: e.target.selectionStart, end: e.target.selectionEnd });
        }}
        onSelect={syncSel}
        onFocus={() => {
          setFocused(true);
          syncSel();
        }}
        onBlur={() => setFocused(false)}
        onScroll={() => {
          if (open) setScrollTick((t) => t + 1);
        }}
        onCompositionStart={() => {
          composing.current = true;
        }}
        onCompositionEnd={() => {
          composing.current = false;
        }}
        onKeyDown={(e) => {
          if (composing.current || e.nativeEvent.isComposing) return;
          if (handleSuggestKeys(e)) return;
          if (e.ctrlKey && (e.key === "s" || e.key === "S")) {
            e.preventDefault();
            onSaveDraft();
            return;
          }
          if (e.ctrlKey && e.key === "Enter") {
            e.preventDefault();
            onSaveAndClose();
            return;
          }
          if (e.ctrlKey && (e.key === "k" || e.key === "K")) {
            e.preventDefault();
            onStatusChange(status === "keep" ? "scratch" : "keep");
            return;
          }
          if (handleFindHotkeys(e)) return;
          const el = textRef.current;
          if (e.ctrlKey && (e.key === "]" || e.key === "[") && el) {
            e.preventDefault();
            applyEdit(
              shiftIndent(
                value,
                el.selectionStart,
                el.selectionEnd,
                indentWidth,
                e.key === "]" ? 1 : -1,
              ),
            );
            return;
          }
          if (e.shiftKey && e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown") && el) {
            e.preventDefault();
            applyEdit(
              duplicateLines(value, el.selectionStart, el.selectionEnd, e.key === "ArrowUp" ? -1 : 1),
            );
            return;
          }
          if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown") && el) {
            e.preventDefault();
            applyEdit(
              moveLines(
                value,
                el.selectionStart,
                el.selectionEnd,
                e.key === "ArrowUp" ? -1 : 1,
              ),
            );
          }
        }}
      />
      {findOpen && (
        <div className="compose-find">
          <div className="compose-find-row">
            <input
              ref={findInputRef}
              type="text"
              className="compose-find-input"
              placeholder="ノート内を検索…"
              value={findQuery}
              onChange={(e) => setFindQuery(e.target.value)}
              onCompositionStart={() => {
                findComposing.current = true;
              }}
              onCompositionEnd={() => {
                findComposing.current = false;
              }}
              onKeyDown={(e) => {
                if (findComposing.current || e.nativeEvent.isComposing) return;
                if (handleFindHotkeys(e)) return;
                if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  closeFind();
                  return;
                }
                if (e.key === "Enter") {
                  e.preventDefault();
                  const el = textRef.current;
                  const pos = el ? el.selectionStart : 0;
                  if (e.shiftKey) jumpBackward(pos - 1, findQuery);
                  else jumpForward(pos + 1, findQuery);
                }
              }}
            />
            <span className="compose-find-count">{findQuery ? `${matchCount}件` : ""}</span>
            <button
              type="button"
              className="compose-find-toggle"
              title="置換"
              aria-label="置換"
              onClick={() => setReplaceOpen((o) => !o)}
            >
              {replaceOpen ? "▾" : "▸"}
            </button>
            <button type="button" className="compose-find-close" onClick={closeFind}>
              ×
            </button>
          </div>
          {replaceOpen && (
            <div className="compose-find-row">
              <input
                ref={replaceInputRef}
                type="text"
                className="compose-find-input"
                placeholder="置換後の文字列…"
                value={replaceText}
                onChange={(e) => setReplaceText(e.target.value)}
                onCompositionStart={() => {
                  findComposing.current = true;
                }}
                onCompositionEnd={() => {
                  findComposing.current = false;
                }}
                onKeyDown={(e) => {
                  if (findComposing.current || e.nativeEvent.isComposing) return;
                  if (handleFindHotkeys(e)) return;
                  if (e.key === "Escape") {
                    e.preventDefault();
                    e.stopPropagation();
                    closeFind();
                    return;
                  }
                  if (e.key === "Enter") {
                    e.preventDefault();
                    if (e.ctrlKey) replaceAll();
                    else replaceCurrent();
                  }
                }}
              />
              <button
                type="button"
                className="compose-find-btn"
                disabled={!findQuery}
                onClick={replaceCurrent}
              >
                置換
              </button>
              <button
                type="button"
                className="compose-find-btn"
                disabled={!findQuery}
                onClick={replaceAll}
              >
                すべて置換
              </button>
            </div>
          )}
        </div>
      )}
      {open && popupPos && (
        <ul className="tag-suggest" style={popupPos} role="listbox">
          {suggestions.map((t, i) => (
            <li
              key={t.tag}
              role="option"
              aria-selected={i === suggestIndex}
              className={i === suggestIndex ? "tag-suggest-item tag-suggest-item-active" : "tag-suggest-item"}
              // mousedown, not click, and preventDefault: the textarea must
              // keep focus or the popup (and the insert target) would vanish.
              onMouseDown={(e) => {
                e.preventDefault();
                acceptSuggestion(t.tag);
              }}
              onMouseEnter={() => setSuggestIndex(i)}
            >
              <span className="tag-suggest-name">#{t.tag}</span>
              <span className="tag-count">{t.count}</span>
            </li>
          ))}
        </ul>
      )}
      {liveTags.length > 0 && (
        <div className="note-tags">
          {liveTags.map((t) => (
            <button key={t} type="button" className="tag-pill" onClick={() => onTagClick(t)}>
              #{t}
            </button>
          ))}
        </div>
      )}
      <div className="compose-toolbar">
        <div className="switch" role="group" aria-label="Keep or scratch">
          <button
            type="button"
            className={status === "keep" ? "switch-btn switch-btn-active" : "switch-btn"}
            onClick={() => onStatusChange("keep")}
          >
            Keep
          </button>
          <button
            type="button"
            className={status === "scratch" ? "switch-btn switch-btn-active" : "switch-btn"}
            onClick={() => onStatusChange("scratch")}
          >
            Scratch
          </button>
        </div>
        <span className="compose-hint">
          Ctrl+F ノート内検索 ・ Ctrl+H 置換 ・ Ctrl+Shift+F 検索 ・ Ctrl+K 切替 ・ Ctrl+, 設定 ・ Ctrl+]/[
          インデント ・ Alt+↑/↓ 行移動 ・ Shift+Alt+↑/↓ 行複製
        </span>
        <div className="compose-actions">
          <button type="button" className="compose-save" onClick={onSaveDraft}>
            一時保存 (Ctrl+S)
          </button>
          <button type="button" className="compose-save compose-save-primary" onClick={onSaveAndClose}>
            保存して閉じる (Ctrl+Enter)
          </button>
        </div>
      </div>
    </div>
  );
}
