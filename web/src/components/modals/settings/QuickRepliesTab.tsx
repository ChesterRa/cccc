// Settings › Quick replies: one list in the order the menus show it. The four default replies
// are ordinary entries the person may reword, reorder or delete; a name or text left unchanged
// follows the interface language, and a deleted one can be brought back. The list is edited as
// one draft and saved over the revision the edit started from, so a save made in another window
// or device in between is reported instead of overwritten.
import { Undo2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon, LcdIcon, PlusIcon, TrashIcon } from "../../Icons";
import { IconButton } from "../../ui/icon-button";
import {
  builtinReplyDefault,
  QUICK_REPLY_KINDS,
  type QuickReplyEntry,
  type QuickReplyKind,
} from "../../../features/quickReplies/effective";
import {
  loadQuickReplyTemplates,
  saveQuickReplyTemplateList,
  useQuickReplyTemplateStore,
} from "../../../features/quickReplies/templateStore";
import type { QuickReplyDraftEntry } from "../../../services/api/quickReplyTemplates";
import { SettingsCard, SettingsNotice, SettingsPage, SettingsSaveBar } from "./SettingsKit";
import { inputClass, secondaryButtonClass } from "./types";

// The daemon's limits, counted as it counts: by Unicode code point.
const MAX_NAME_CHARS = 40;
const MAX_TEXT_CHARS = 10_000;
const length = (value: string) => Array.from(value).length;

// A default reply keeps the person's changes field by field; a field left out shows and keeps
// the default. A stored change is kept until that field is edited, even when it reads like the
// default of the language shown now.
type Row =
  | { key: string; builtin: QuickReplyKind; name?: string; text?: string }
  | { key: string; id?: string; name: string; text: string };
type Defaults = Record<QuickReplyKind, { name: string; text: string }>;
type Problem = "name" | "nameTooLong" | "text" | "textTooLong";
type Base = { revision: number; replies: QuickReplyEntry[] };

let rowCounter = 0;
const newKey = () => `quick-reply-row-${++rowCounter}`;
const rowsOf = (replies: QuickReplyEntry[]): Row[] =>
  replies.map((reply) => ({ key: newKey(), ...reply }));

function problemOf(row: { name: string; text: string }): Problem | null {
  const name = row.name.trim();
  if (!name) return "name";
  if (length(name) > MAX_NAME_CHARS || /[\r\n]/.test(name)) return "nameTooLong";
  // The text is kept exactly as written; only a text of nothing but white space is refused.
  if (!row.text.trim()) return "text";
  if (length(row.text) > MAX_TEXT_CHARS) return "textTooLong";
  return null;
}

const sameEntry = (row: Row, reply: QuickReplyEntry) =>
  "builtin" in row
    ? "builtin" in reply &&
      row.builtin === reply.builtin &&
      row.name === reply.name &&
      row.text === reply.text
    : !("builtin" in reply) &&
      row.id === reply.id &&
      row.name === reply.name &&
      row.text === reply.text;

const differs = (rows: Row[], replies: QuickReplyEntry[]) =>
  rows.length !== replies.length || rows.some((row, index) => !sameEntry(row, replies[index]));

function draftOf(row: Row): QuickReplyDraftEntry {
  if ("builtin" in row) {
    return {
      builtin: row.builtin,
      ...(row.name === undefined ? {} : { name: row.name }),
      ...(row.text === undefined ? {} : { text: row.text }),
    };
  }
  return row.id
    ? { id: row.id, name: row.name, text: row.text }
    : { name: row.name, text: row.text };
}

export function QuickRepliesTab() {
  const { t } = useTranslation(["settings", "common"]);
  const { t: tChat } = useTranslation("chat");
  const defaults = Object.fromEntries(
    QUICK_REPLY_KINDS.map((kind) => [kind, builtinReplyDefault(tChat, kind)]),
  ) as Defaults;
  const replies = useQuickReplyTemplateStore((state) => state.replies);
  const revision = useQuickReplyTemplateStore((state) => state.revision);
  const limit = useQuickReplyTemplateStore((state) => state.limit);
  const loaded = useQuickReplyTemplateStore((state) => state.loaded);
  const [base, setBase] = useState<Base>({ revision, replies });
  const [rows, setRows] = useState<Row[]>(() => rowsOf(replies));
  const [loadError, setLoadError] = useState("");
  const [saveError, setSaveError] = useState("");
  const [conflict, setConflict] = useState(false);
  const [showProblems, setShowProblems] = useState(false);
  const [busy, setBusy] = useState(false);
  const [hint, setHint] = useState("");
  const mounted = useRef(true);
  const focusRow = useRef<string | null>(null);
  const dirty = differs(rows, base.replies);
  const shown = (row: Row) =>
    "builtin" in row
      ? {
          name: row.name ?? defaults[row.builtin].name,
          text: row.text ?? defaults[row.builtin].text,
        }
      : row;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  const startFrom = (next: Base) => {
    setBase(next);
    setRows(rowsOf(next.replies));
    setShowProblems(false);
  };

  const read = async () => {
    const result = await loadQuickReplyTemplates();
    if (!mounted.current) return false;
    setLoadError(result.ok ? "" : result.message);
    return result.ok;
  };

  useEffect(() => {
    mounted.current = true;
    void read();
    return () => {
      mounted.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- read once when the page opens.
  }, []);

  // A newer list, read here or saved from a message menu's page, replaces the draft while
  // nothing has been edited; edits stay, and saving them then reports the conflict.
  useEffect(() => {
    if (dirtyRef.current) return;
    if (revision === base.revision && replies === base.replies) return;
    startFrom({ revision, replies });
  }, [base.revision, base.replies, revision, replies]);

  useEffect(() => {
    const key = focusRow.current;
    if (!key) return;
    focusRow.current = null;
    document.querySelector<HTMLInputElement>(`[data-quick-reply-name="${key}"]`)?.focus();
  }, [rows]);

  const touched = () => {
    setHint("");
    setSaveError("");
  };
  const edit = (change: (current: Row[]) => Row[]) => {
    touched();
    setRows(change);
  };
  const update = (key: string, field: "name" | "text", value: string) =>
    edit((current) =>
      current.map((row) => {
        if (row.key !== key) return row;
        if (!("builtin" in row)) return { ...row, [field]: value };
        // A default reply's field typed back to its default follows the interface language again.
        const next = { ...row };
        if (value === defaults[row.builtin][field]) delete next[field];
        else next[field] = value;
        return next;
      }),
    );
  const move = (index: number, offset: -1 | 1) =>
    edit((current) => {
      const next = [...current];
      const [row] = next.splice(index, 1);
      next.splice(index + offset, 0, row);
      return next;
    });
  const remove = (key: string) => edit((current) => current.filter((row) => row.key !== key));
  // Restoring one default reply leaves the rest of the list as drafted.
  const restore = (key: string) =>
    edit((current) =>
      current.map((row) =>
        row.key === key && "builtin" in row ? { key, builtin: row.builtin } : row,
      ),
    );
  const add = () => {
    const key = newKey();
    focusRow.current = key;
    edit((current) => [...current, { key, name: "", text: "" }]);
  };
  // A deleted default reply comes back at the end, as its default, without touching the others.
  const bringBack = (kind: QuickReplyKind) =>
    edit((current) => [...current, { key: newKey(), builtin: kind }]);

  const save = async () => {
    if (rows.some((row) => problemOf(shown(row)))) {
      setShowProblems(true);
      return;
    }
    setBusy(true);
    setHint("");
    setSaveError("");
    const result = await saveQuickReplyTemplateList(base.revision, rows.map(draftOf));
    if (!mounted.current) return;
    setBusy(false);
    if (result.ok) {
      const saved = useQuickReplyTemplateStore.getState();
      startFrom({ revision: saved.revision, replies: saved.replies });
      setHint(t("saveFeedback.saved"));
    } else if (result.code === "quick_reply_templates_conflict") {
      setConflict(true);
    } else {
      setSaveError(result.message);
    }
  };

  const reload = async () => {
    if (!(await read())) return;
    const latest = useQuickReplyTemplateStore.getState();
    setConflict(false);
    setSaveError("");
    setHint("");
    startFrom({ revision: latest.revision, replies: latest.replies });
  };

  const full = rows.length >= limit;
  const missing = QUICK_REPLY_KINDS.filter(
    (kind) => !rows.some((row) => "builtin" in row && row.builtin === kind),
  );
  return (
    <SettingsPage
      title={t("quickReplies.title")}
      description={t("quickReplies.description")}
      footer={
        <SettingsSaveBar
          label={t("common:save")}
          busy={busy}
          onSave={() => void save()}
          disabled={!dirty || conflict}
          hint={hint || (dirty ? t("quickReplies.unsaved") : undefined)}
        />
      }
    >
      {loadError ? (
        <SettingsNotice
          tone="error"
          action={
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              onClick={() => void reload()}
            >
              {t("common:retry")}
            </button>
          }
        >
          {t("quickReplies.loadFailed", { error: loadError })}
        </SettingsNotice>
      ) : null}
      {conflict ? (
        <SettingsNotice
          tone="warning"
          action={
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              onClick={() => void reload()}
            >
              {t("quickReplies.reload")}
            </button>
          }
        >
          {t("quickReplies.conflict")}
        </SettingsNotice>
      ) : null}
      {saveError ? (
        <SettingsNotice tone="error">
          {t("quickReplies.saveFailed", { error: saveError })}
        </SettingsNotice>
      ) : null}
      <SettingsCard
        description={t("quickReplies.count", { used: rows.length, limit })}
        actions={
          <button
            type="button"
            className={secondaryButtonClass("sm")}
            onClick={add}
            // Until the list has been read, a new row would be saved over an unknown revision.
            disabled={!loaded || full || busy}
            title={full ? t("quickReplies.full", { limit }) : undefined}
          >
            <PlusIcon size={14} aria-hidden="true" />
            {t("quickReplies.add")}
          </button>
        }
      >
        {!loaded ? (
          // Until the person's list is read, which default replies it holds is unknown.
          <p className="px-5 py-4 text-[13px] text-[var(--color-text-muted)]">
            {t("common:loading")}
          </p>
        ) : rows.length === 0 ? (
          <p className="px-5 py-4 text-[13px] text-[var(--color-text-muted)]">
            {t("quickReplies.empty")}
          </p>
        ) : (
          // display: contents keeps the rows in the card's layout; the dividers are matched in
          // the DOM, so the fieldset carries them for its own rows.
          <fieldset
            disabled={busy}
            className="contents divide-y divide-[var(--glass-border-subtle)]"
          >
            {rows.map((row, index) => {
              const value = shown(row);
              const problem = showProblems ? problemOf(value) : null;
              const problemId = `${row.key}-problem`;
              const isDefault = "builtin" in row;
              return (
                <div
                  key={row.key}
                  className="grid min-w-0 gap-2 px-5 py-3.5"
                  data-quick-reply-row
                  data-quick-reply-builtin={isDefault ? row.builtin : undefined}
                >
                  <div className="flex min-w-0 items-center gap-1.5">
                    <input
                      data-quick-reply-name={row.key}
                      value={value.name}
                      onChange={(event) => update(row.key, "name", event.target.value)}
                      placeholder={t("quickReplies.name")}
                      aria-label={t("quickReplies.name")}
                      aria-invalid={problem === "name" || problem === "nameTooLong" || undefined}
                      aria-describedby={problem ? problemId : undefined}
                      className={`${inputClass()} min-w-0 flex-1`}
                    />
                    {/* Restore comes first, so the move and delete buttons keep one column
                        whether or not a row is a default reply. */}
                    {isDefault ? (
                      <IconButton
                        size="sm"
                        variant="ghost"
                        label={t("quickReplies.restore")}
                        disabled={row.name === undefined && row.text === undefined}
                        onClick={() => restore(row.key)}
                      >
                        <LcdIcon icon={Undo2} glyph="undo" size={16} aria-hidden="true" />
                      </IconButton>
                    ) : null}
                    <IconButton
                      size="sm"
                      variant="ghost"
                      label={t("quickReplies.moveUp")}
                      disabled={index === 0}
                      onClick={() => move(index, -1)}
                    >
                      <ChevronDownIcon size={16} className="rotate-180" aria-hidden="true" />
                    </IconButton>
                    <IconButton
                      size="sm"
                      variant="ghost"
                      label={t("quickReplies.moveDown")}
                      disabled={index === rows.length - 1}
                      onClick={() => move(index, 1)}
                    >
                      <ChevronDownIcon size={16} aria-hidden="true" />
                    </IconButton>
                    <IconButton
                      size="sm"
                      variant="ghost"
                      label={t("common:delete")}
                      onClick={() => remove(row.key)}
                    >
                      <TrashIcon size={16} aria-hidden="true" />
                    </IconButton>
                  </div>
                  <textarea
                    value={value.text}
                    onChange={(event) => update(row.key, "text", event.target.value)}
                    placeholder={t("quickReplies.text")}
                    aria-label={t("quickReplies.text")}
                    aria-invalid={problem === "text" || problem === "textTooLong" || undefined}
                    aria-describedby={problem ? problemId : undefined}
                    rows={3}
                    className={`${inputClass()} max-h-72 min-h-[5.5rem] resize-y`}
                  />
                  {problem ? (
                    <p id={problemId} className="text-xs text-[var(--color-accent-danger)]">
                      {t(`quickReplies.problem.${problem}`)}
                    </p>
                  ) : null}
                </div>
              );
            })}
          </fieldset>
        )}
        {loaded && missing.length > 0 ? (
          <div
            className="flex min-w-0 flex-wrap items-center gap-1.5 border-t border-[var(--glass-border-subtle)] px-5 py-3"
            data-quick-reply-bring-back
          >
            <span className="text-[13px] text-[var(--color-text-muted)]">
              {t("quickReplies.bringBack")}
            </span>
            {missing.map((kind) => (
              <button
                key={kind}
                type="button"
                className={secondaryButtonClass("sm")}
                onClick={() => bringBack(kind)}
                disabled={full || busy}
                title={full ? t("quickReplies.full", { limit }) : undefined}
              >
                <PlusIcon size={14} aria-hidden="true" />
                {defaults[kind].name}
              </button>
            ))}
          </div>
        ) : null}
      </SettingsCard>
    </SettingsPage>
  );
}
