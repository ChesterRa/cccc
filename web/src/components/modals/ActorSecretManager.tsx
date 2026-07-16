import { useId, useMemo, useReducer, useState } from "react";
import {
  AlertTriangle,
  ClipboardPaste,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  Undo2,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { SecretValueInput } from "./SecretValueInput";
import { ActorSecretBulkInput } from "./ActorSecretBulkInput";
import { actorSecretDraftReducer, emptyActorSecretDraftState } from "./actorSecretDraftModel";
import {
  type ActorSecretChanges,
  isValidActorSecretKey,
  setActorSecretClearAll,
  stageActorSecretSet,
  stageActorSecretSetMany,
  stageActorSecretUnset,
  undoActorSecretSet,
  undoActorSecretUnset,
} from "./actorSecretManagerModel";
interface ActorSecretManagerProps {
  keys: string[];
  masks: Record<string, string>;
  changes: ActorSecretChanges;
  loading?: boolean;
  keysLoadFailed?: boolean;
  disabled?: boolean;
  onRefresh: () => void;
  onChangesChange: (changes: ActorSecretChanges) => void;
}
export function ActorSecretManager({
  keys,
  masks,
  changes,
  loading = false,
  keysLoadFailed = false,
  disabled = false,
  onRefresh,
  onChangesChange,
}: ActorSecretManagerProps) {
  const { t } = useTranslation("actors");
  const inputId = useId();
  const [drafts, dispatchDraft] = useReducer(
    actorSecretDraftReducer,
    undefined,
    emptyActorSecretDraftState,
  );
  const [bulkOpen, setBulkOpen] = useState(false);
  const {
    addOpen,
    addKey,
    addValue,
    addValueTouched,
    showAddValue,
    editingKey,
    editValue,
    editValueTouched,
    showEditValue,
  } = drafts;
  const configuredKeys = useMemo(() => new Set(keys), [keys]);
  const stagedSetKeys = Object.keys(changes.setVars);
  const hasPendingChanges = stagedSetKeys.length > 0 || changes.unsetKeys.length > 0;
  const normalizedAddKey = addKey.trim();
  const addKeyValid = isValidActorSecretKey(normalizedAddKey);
  const controlsDisabled = disabled || loading;
  const refreshButton = (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className="w-9 shrink-0 px-0"
      disabled={controlsDisabled}
      aria-label={t("refreshConfiguredKeys")}
      title={t("refreshConfiguredKeys")}
      onClick={onRefresh}
    >
      <RefreshCw size={14} className={loading ? "animate-spin" : ""} aria-hidden="true" />
    </Button>
  );
  const submitAdd = () => {
    if (!addKeyValid || !addValueTouched || controlsDisabled) return;
    onChangesChange(stageActorSecretSet(changes, normalizedAddKey, addValue));
    dispatchDraft({ type: "closeAdd" });
  };

  const submitUpdate = (key: string) => {
    if (!editValueTouched || controlsDisabled) return;
    onChangesChange(stageActorSecretSet(changes, key, editValue));
    dispatchDraft({ type: "closeEdit" });
  };

  if (changes.clearAll) {
    return (
      <section aria-label={t("secretManager.title")} className="mt-4">
        <div className="flex items-start gap-3 rounded-xl border border-rose-500/25 bg-rose-500/10 p-3 text-rose-700 dark:text-rose-300">
          <AlertTriangle size={18} className="mt-0.5 shrink-0" aria-hidden="true" />
          <div className="min-w-0 flex-1">
            <div className="text-xs font-semibold">{t("secretManager.clearAllTitle")}</div>
            <div className="mt-1 text-[11px] leading-relaxed">
              {t("secretManager.clearAllPending")}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            {refreshButton}
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={controlsDisabled}
              onClick={() => onChangesChange(setActorSecretClearAll(changes, false))}
            >
              <Undo2 size={14} aria-hidden="true" />
              {t("secretManager.undo")}
            </Button>
          </div>
        </div>
      </section>
    );
  }

  return (
    <section aria-label={t("secretManager.title")} className="mt-4">
      <div className="flex items-center justify-between gap-3">
        <div className="text-[11px] font-medium text-[var(--color-text-secondary)]">
          {t("secretManager.configured")}
        </div>
        <div className="flex flex-wrap items-center justify-end gap-1">
          {refreshButton}
          {!bulkOpen ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={controlsDisabled}
              onClick={() => setBulkOpen(true)}
            >
              <ClipboardPaste size={14} aria-hidden="true" />
              {t("secretManager.batchPaste")}
            </Button>
          ) : null}
          {!addOpen ? (
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={controlsDisabled}
              onClick={() => dispatchDraft({ type: "openAdd" })}
            >
              <Plus size={14} aria-hidden="true" />
              {t("secretManager.addVariable")}
            </Button>
          ) : null}
        </div>
      </div>

      {addOpen ? (
        <div className="mt-2 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3">
          <div className="grid gap-2 sm:grid-cols-[minmax(0,0.9fr)_minmax(0,1.35fr)_auto] sm:items-end">
            <label className="min-w-0 text-[11px] font-medium text-[var(--color-text-secondary)]">
              {t("secretManager.variableName")}
              <Input
                value={addKey}
                disabled={controlsDisabled}
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                className="mt-1 font-mono"
                onChange={(event) =>
                  dispatchDraft({ type: "setAddKey", value: event.target.value })
                }
              />
            </label>
            <div className="min-w-0 text-[11px] font-medium text-[var(--color-text-secondary)]">
              <label htmlFor={`${inputId}-add-value`}>{t("secretManager.value")}</label>
              <div className="mt-1">
                <SecretValueInput
                  id={`${inputId}-add-value`}
                  ariaLabel={t("secretManager.value")}
                  value={addValue}
                  visible={showAddValue}
                  disabled={controlsDisabled}
                  placeholder={t("secretManager.newValue")}
                  showLabel={t("secretManager.showValue")}
                  hideLabel={t("secretManager.hideValue")}
                  onChange={(value) => dispatchDraft({ type: "setAddValue", value })}
                  onToggleVisibility={() => dispatchDraft({ type: "toggleAddVisibility" })}
                />
              </div>
            </div>
            <div className="flex gap-2 sm:pb-0">
              <Button
                type="button"
                size="sm"
                disabled={controlsDisabled || !addKeyValid || !addValueTouched}
                onClick={submitAdd}
              >
                {t("secretManager.addVariable")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                disabled={controlsDisabled}
                aria-label={t("common:cancel")}
                title={t("common:cancel")}
                onClick={() => dispatchDraft({ type: "closeAdd" })}
              >
                <X size={16} aria-hidden="true" />
              </Button>
            </div>
          </div>
          {normalizedAddKey && !addKeyValid ? (
            <div className="mt-2 text-[10px] text-rose-600 dark:text-rose-400">
              {t("secretManager.invalidKey")}
            </div>
          ) : null}
        </div>
      ) : null}

      {bulkOpen ? (
        <div className="mt-2">
          <ActorSecretBulkInput
            disabled={controlsDisabled}
            onCancel={() => setBulkOpen(false)}
            onApply={(setVars) => {
              onChangesChange(stageActorSecretSetMany(changes, setVars));
              setBulkOpen(false);
            }}
          />
        </div>
      ) : null}

      <div className="mt-2 space-y-2">
        {loading ? (
          <div className="rounded-xl border border-dashed border-[var(--glass-border-subtle)] px-3 py-5 text-center text-[11px] text-[var(--color-text-muted)]">
            {t("secretManager.loading")}
          </div>
        ) : keys.length === 0 ? (
          <div className="rounded-xl border border-dashed border-[var(--glass-border-subtle)] px-3 py-5 text-center text-[11px] text-[var(--color-text-muted)]">
            {t("secretManager.noVariables")}
          </div>
        ) : null}
        {keys.map((key) => {
          const pendingRemove = changes.unsetKeys.includes(key);
          const pendingUpdate = Object.prototype.hasOwnProperty.call(changes.setVars, key);
          const isEditing = editingKey === key;
          return (
            <div
              key={key}
              className={`rounded-xl border px-3 py-2.5 ${pendingRemove ? "border-rose-500/25 bg-rose-500/5" : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]"}`}
            >
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <span
                  className={`min-w-0 basis-[min(12rem,100%)] flex-1 truncate font-mono text-[11px] ${pendingRemove ? "text-[var(--color-text-muted)] line-through" : "text-[var(--color-text-primary)]"}`}
                  title={key}
                >
                  {key}
                </span>
                <span
                  className="shrink-0 font-mono text-[10px] text-[var(--color-text-muted)]"
                  title={`${t("secretManager.maskedValue")}: ${masks[key] || "******"}`}
                >
                  ••••••
                </span>
                {pendingUpdate ? (
                  <span className="max-w-full rounded-md bg-emerald-500/10 px-1.5 py-0.5 text-[10px] text-emerald-700 dark:text-emerald-300">
                    {t("secretManager.pendingUpdate")}
                  </span>
                ) : null}
                {pendingRemove ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={controlsDisabled}
                    onClick={() => onChangesChange(undoActorSecretUnset(changes, key))}
                  >
                    <Undo2 size={14} aria-hidden="true" />
                    {t("secretManager.undo")}
                  </Button>
                ) : (
                  <>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="w-9 px-0"
                      disabled={controlsDisabled}
                      aria-label={`${t("secretManager.update")} ${key}`}
                      title={`${t("secretManager.update")} ${key}`}
                      onClick={() => dispatchDraft({ type: "startEdit", key })}
                    >
                      <Pencil size={14} aria-hidden="true" />
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="w-9 px-0 text-rose-600 hover:text-rose-700 dark:text-rose-400"
                      disabled={controlsDisabled}
                      aria-label={`${t("secretManager.remove")} ${key}`}
                      title={`${t("secretManager.remove")} ${key}`}
                      onClick={() => {
                        dispatchDraft({ type: "discardKey", key });
                        onChangesChange(stageActorSecretUnset(changes, key));
                      }}
                    >
                      <Trash2 size={14} aria-hidden="true" />
                    </Button>
                  </>
                )}
              </div>
              {isEditing && !pendingRemove ? (
                <div className="mt-2 flex flex-col gap-2 border-t border-[var(--glass-border-subtle)] pt-2 sm:flex-row">
                  <SecretValueInput
                    id={`${inputId}-${key}-value`}
                    ariaLabel={`${t("secretManager.newValue")} ${key}`}
                    value={editValue}
                    visible={showEditValue}
                    disabled={controlsDisabled}
                    placeholder={t("secretManager.newValue")}
                    showLabel={t("secretManager.showValue")}
                    hideLabel={t("secretManager.hideValue")}
                    onChange={(value) => dispatchDraft({ type: "setEditValue", value })}
                    onToggleVisibility={() => dispatchDraft({ type: "toggleEditVisibility" })}
                  />
                  <div className="flex gap-2">
                    <Button
                      type="button"
                      size="sm"
                      disabled={controlsDisabled || !editValueTouched}
                      onClick={() => submitUpdate(key)}
                    >
                      {t("secretManager.update")}
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      disabled={controlsDisabled}
                      aria-label={t("common:cancel")}
                      title={t("common:cancel")}
                      onClick={() => dispatchDraft({ type: "closeEdit" })}
                    >
                      <X size={16} aria-hidden="true" />
                    </Button>
                  </div>
                </div>
              ) : null}
            </div>
          );
        })}
      </div>

      {hasPendingChanges ? (
        <div
          aria-live="polite"
          className="mt-3 rounded-xl border border-emerald-500/20 bg-emerald-500/5 p-3"
        >
          <div className="text-[11px] font-semibold text-emerald-800 dark:text-emerald-200">
            {t("secretManager.pendingChanges")}
          </div>
          <div className="mt-2 space-y-1.5">
            {stagedSetKeys.map((key) => (
              <div
                key={`set-${key}`}
                className="flex flex-wrap items-center gap-2 text-[10px] text-[var(--color-text-secondary)]"
              >
                <span
                  className="min-w-0 basis-[min(12rem,100%)] flex-1 truncate font-mono"
                  title={key}
                >
                  {key}
                </span>
                <span className="max-w-full">
                  {t(
                    configuredKeys.has(key)
                      ? "secretManager.pendingUpdate"
                      : "secretManager.pendingAdd",
                  )}
                </span>
                <button
                  type="button"
                  disabled={controlsDisabled}
                  className="inline-flex items-center gap-1 rounded-md px-1.5 py-1 text-emerald-700 hover:bg-emerald-500/10 disabled:opacity-50 dark:text-emerald-300"
                  onClick={() => onChangesChange(undoActorSecretSet(changes, key))}
                >
                  <Undo2 size={12} aria-hidden="true" />
                  {t("secretManager.undo")}
                </button>
              </div>
            ))}
            {changes.unsetKeys.map((key) => (
              <div
                key={`unset-${key}`}
                className="flex flex-wrap items-center gap-2 text-[10px] text-[var(--color-text-secondary)]"
              >
                <span
                  className="min-w-0 basis-[min(12rem,100%)] flex-1 truncate font-mono"
                  title={key}
                >
                  {key}
                </span>
                <span className="max-w-full">{t("secretManager.pendingRemove")}</span>
                <button
                  type="button"
                  disabled={controlsDisabled}
                  className="inline-flex items-center gap-1 rounded-md px-1.5 py-1 text-emerald-700 hover:bg-emerald-500/10 disabled:opacity-50 dark:text-emerald-300"
                  onClick={() => onChangesChange(undoActorSecretUnset(changes, key))}
                >
                  <Undo2 size={12} aria-hidden="true" />
                  {t("secretManager.undo")}
                </button>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      <div className="mt-4 flex flex-col gap-3 border-t border-[var(--glass-border-subtle)] pt-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="text-[11px] font-semibold text-[var(--color-text-primary)]">
            {t("secretManager.clearAllTitle")}
          </div>
          <div className="mt-0.5 text-[10px] text-[var(--color-text-muted)]">
            {t("secretManager.clearAllHint")}
          </div>
        </div>
        <Button
          type="button"
          variant="destructive"
          size="sm"
          disabled={controlsDisabled || keysLoadFailed || keys.length === 0}
          onClick={() => {
            dispatchDraft({ type: "discardAll" });
            onChangesChange(setActorSecretClearAll(changes, true));
          }}
        >
          <Trash2 size={14} aria-hidden="true" />
          {t("secretManager.clearAllAction")}
        </Button>
      </div>
    </section>
  );
}
