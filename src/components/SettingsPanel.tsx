import { useState } from "react";
import { openNotesFolder, setSettings, type Settings } from "../api";

interface Props {
  settings: Settings;
  onClose: () => void;
  onSaved: (next: Settings) => void;
}

export default function SettingsPanel({ settings, onClose, onSaved }: Props) {
  const [draft, setDraft] = useState<Settings>(settings);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const dirChanged = draft.notesDir !== settings.notesDir;

  const save = () => {
    setError(null);
    setSaving(true);
    setSettings(draft)
      .then((next) => {
        onSaved(next);
        onClose();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setSaving(false));
  };

  return (
    <>
      <div className="view-title" data-drag-region>
        設定
      </div>
      <div className="settings">
        <label>
          <span>ホットキー</span>
          <input
            type="text"
            value={draft.hotkey}
            onChange={(e) => setDraft({ ...draft, hotkey: e.target.value })}
          />
          <small>
            例: <code>Ctrl+Alt+N</code>
          </small>
        </label>

        <label>
          <span>メモの保存フォルダ</span>
          <input
            type="text"
            value={draft.notesDir}
            onChange={(e) => setDraft({ ...draft, notesDir: e.target.value })}
          />
          {dirChanged && (
            <p className="settings-note">
              既存のメモは自動移動されません。新しいフォルダに <code>keep</code>/<code>scratch</code>{" "}
              が作られます。
            </p>
          )}
        </label>

        <label className="settings-inline">
          <span>期限切れの目安（日数）</span>
          <input
            type="number"
            min={1}
            value={draft.expiryDays}
            onChange={(e) => setDraft({ ...draft, expiryDays: Number(e.target.value) || 1 })}
          />
        </label>

        <label className="settings-inline">
          <span>Windows の起動時に自動で起動する</span>
          <input
            type="checkbox"
            checked={draft.autostart}
            onChange={(e) => setDraft({ ...draft, autostart: e.target.checked })}
          />
        </label>

        <label className="settings-inline">
          <span>フォーカスを外したら自動で隠す</span>
          <input
            type="checkbox"
            checked={draft.hideOnBlur}
            onChange={(e) => setDraft({ ...draft, hideOnBlur: e.target.checked })}
          />
        </label>

        <label className="settings-inline">
          <span>インデント幅（Ctrl+[ / Ctrl+]）</span>
          <input
            type="number"
            min={1}
            max={8}
            value={draft.indentWidth}
            onChange={(e) => setDraft({ ...draft, indentWidth: Number(e.target.value) || 1 })}
          />
        </label>

        {error && <p className="settings-error">{error}</p>}

        <div className="settings-actions">
          <button type="button" onClick={save} disabled={saving}>
            保存
          </button>
          <button type="button" onClick={() => openNotesFolder().catch((e) => setError(String(e)))}>
            エクスプローラーで開く
          </button>
          <button type="button" onClick={onClose}>
            閉じる
          </button>
        </div>
      </div>
    </>
  );
}
