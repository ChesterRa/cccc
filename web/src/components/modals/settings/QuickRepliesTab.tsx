// Settings › Quick replies: the person's own reply templates. The list is edited here as a
// whole and saved together over the revision the edit started from, so a save made in another
// window or device in between is reported instead of overwritten.
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon, PlusIcon, TrashIcon } from "../../Icons";
import { IconButton } from "../../ui/icon-button";
import {
  loadQuickReplyTemplates,
  saveQuickReplyTemplateList,
  useQuickReplyTemplateStore,
} from "../../../features/quickReplies/templateStore";
import type { QuickReplyTemplate } from "../../../services/api/quickReplyTemplates";
import { SettingsCard, SettingsNotice, SettingsPage, SettingsSaveBar } from "./SettingsKit";
import { inputClass, secondaryButtonClass } from "./types";

// The daemon's limits, counted as it counts: by Unicode code point.
const MAX_NAME_CHARS = 40;
const MAX_TEXT_CHARS = 10_000;
const length = (value: string) => Array.from(value).length;

type Row = { key: string; id?: string; name: string; text: string };
type Problem = "name" | "nameTooLong" | "text" | "textTooLong";
type Base = { revision: number; templates: QuickReplyTemplate[] };

let rowCounter = 0;
const newKey = () => `quick-reply-row-${++rowCounter}`;
const rowsOf = (templates: QuickReplyTemplate[]): Row[] =>
  templates.map((template) => ({ key: newKey(), ...template }));

function problemOf(row: Row): Problem | null {
  const name = row.name.trim();
  if (!name) return "name";
  if (length(name) > MAX_NAME_CHARS || /[\r\n]/.test(name)) return "nameTooLong";
  // The text is kept exactly as written; only a text of nothing but white space is refused.
  if (!row.text.trim()) return "text";
  if (length(row.text) > MAX_TEXT_CHARS) return "textTooLong";
  return null;
}

function differs(rows: Row[], templates: QuickReplyTemplate[]) {
  return (
    rows.length !== templates.length ||
    rows.some(
      (row, index) =>
        row.id !== templates[index].id ||
        row.name !== templates[index].name ||
        row.text !== templates[index].text,
    )
  );
}

export function QuickRepliesTab() {
  const { t } = useTranslation(["settings", "common"]);
  const templates = useQuickReplyTemplateStore((state) => state.templates);
  const revision = useQuickReplyTemplateStore((state) => state.revision);
  const limit = useQuickReplyTemplateStore((state) => state.limit);
  const loaded = useQuickReplyTemplateStore((state) => state.loaded);
  const [base, setBase] = useState<Base>({ revision, templates });
  const [rows, setRows] = useState<Row[]>(() => rowsOf(templates));
  const [loadError, setLoadError] = useState("");
  const [saveError, setSaveError] = useState("");
  const [conflict, setConflict] = useState(false);
  const [showProblems, setShowProblems] = useState(false);
  const [busy, setBusy] = useState(false);
  const [hint, setHint] = useState("");
  const mounted = useRef(true);
  const focusRow = useRef<string | null>(null);
  const dirty = differs(rows, base.templates);
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  const startFrom = (next: Base) => {
    setBase(next);
    setRows(rowsOf(next.templates));
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
    if (revision === base.revision && templates === base.templates) return;
    startFrom({ revision, templates });
  }, [base.revision, base.templates, revision, templates]);

  useEffect(() => {
    const key = focusRow.current;
    if (!key) return;
    focusRow.current = null;
    document.querySelector<HTMLInputElement>(`[data-quick-reply-name="${key}"]`)?.focus();
  }, [rows]);

  const edit = (change: (current: Row[]) => Row[]) => {
    setHint("");
    setSaveError("");
    setRows(change);
  };
  const update = (key: string, field: "name" | "text", value: string) =>
    edit((current) => current.map((row) => (row.key === key ? { ...row, [field]: value } : row)));
  const move = (index: number, offset: -1 | 1) =>
    edit((current) => {
      const next = [...current];
      const [row] = next.splice(index, 1);
      next.splice(index + offset, 0, row);
      return next;
    });
  const remove = (key: string) => edit((current) => current.filter((row) => row.key !== key));
  const add = () => {
    const key = newKey();
    focusRow.current = key;
    edit((current) => [...current, { key, name: "", text: "" }]);
  };

  const save = async () => {
    if (rows.some((row) => problemOf(row))) {
      setShowProblems(true);
      return;
    }
    setBusy(true);
    setHint("");
    setSaveError("");
    const result = await saveQuickReplyTemplateList(
      base.revision,
      rows.map(({ id, name, text }) => (id ? { id, name, text } : { name, text })),
    );
    if (!mounted.current) return;
    setBusy(false);
    if (result.ok) {
      const saved = useQuickReplyTemplateStore.getState();
      startFrom({ revision: saved.revision, templates: saved.templates });
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
    startFrom({ revision: latest.revision, templates: latest.templates });
  };

  const full = rows.length >= limit;
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
        title={t("quickReplies.listTitle")}
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
        {rows.length === 0 ? (
          <p className="px-5 py-4 text-[13px] text-[var(--color-text-muted)]">
            {loaded ? t("quickReplies.empty") : t("common:loading")}
          </p>
        ) : (
          // display: contents keeps the rows in the card's layout; the dividers are matched in
          // the DOM, so the fieldset carries them for its own rows.
          <fieldset
            disabled={busy}
            className="contents divide-y divide-[var(--glass-border-subtle)]"
          >
            {rows.map((row, index) => {
              const problem = showProblems ? problemOf(row) : null;
              const problemId = `${row.key}-problem`;
              return (
                <div key={row.key} className="grid min-w-0 gap-2 px-5 py-3.5" data-quick-reply-row>
                  <div className="flex min-w-0 items-center gap-1.5">
                    <input
                      data-quick-reply-name={row.key}
                      value={row.name}
                      onChange={(event) => update(row.key, "name", event.target.value)}
                      placeholder={t("quickReplies.name")}
                      aria-label={t("quickReplies.name")}
                      aria-invalid={problem === "name" || problem === "nameTooLong" || undefined}
                      aria-describedby={problem ? problemId : undefined}
                      className={`${inputClass()} min-w-0 flex-1`}
                    />
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
                    value={row.text}
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
      </SettingsCard>
    </SettingsPage>
  );
}
