import { useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { SecretValueInput } from "./SecretValueInput";
import { DeepSeekPendingChanges } from "./DeepSeekPendingChanges";
import {
  type ActorSecretChanges,
  stageActorSecretSet,
  stageActorSecretUnset,
  setActorSecretClearAll,
  undoActorSecretSet,
  undoActorSecretUnset,
} from "./actorSecretManagerModel";
import {
  DEEPSEEK_BASE_URL,
  DEEPSEEK_MODEL,
  DEEPSEEK_SETTING_KEYS,
  deepSeekSettingsError,
} from "./deepseekSettingsModel";

interface DeepSeekConnectionFieldsProps {
  keys?: string[];
  changes: ActorSecretChanges;
  initialValues?: Record<string, string>;
  disabled?: boolean;
  loading?: boolean;
  loadFailed?: boolean;
  onChange: (changes: ActorSecretChanges) => void;
  onSave?: () => void;
  onSaveAndRestart?: () => void;
  saveDisabled?: boolean;
}

export function DeepSeekConnectionFields({
  keys = [],
  changes,
  initialValues = {},
  disabled = false,
  loading = false,
  loadFailed = false,
  onChange,
  onSave,
  onSaveAndRestart,
  saveDisabled = false,
}: DeepSeekConnectionFieldsProps) {
  const { t } = useTranslation("actors");
  const id = useId();
  const [showKey, setShowKey] = useState(false);
  const controlsDisabled = disabled || loading || loadFailed || changes.clearAll;
  const values = { ...initialValues, ...changes.setVars };
  for (const key of changes.unsetKeys) delete values[key];
  const error = deepSeekSettingsError(values);
  const hasPendingChanges =
    changes.clearAll ||
    DEEPSEEK_SETTING_KEYS.some(
      (key) =>
        Object.prototype.hasOwnProperty.call(changes.setVars, key) ||
        changes.unsetKeys.includes(key),
    );
  const update = (key: string, value: string) => {
    const next = value
      ? stageActorSecretSet(changes, key, value)
      : Object.prototype.hasOwnProperty.call(initialValues, key)
        ? stageActorSecretUnset(changes, key)
        : undoActorSecretSet(undoActorSecretUnset(changes, key), key);
    onChange(next);
  };

  return (
    <section
      className="space-y-4 border-t border-[var(--glass-border-subtle)] pt-4"
      aria-label={t("deepseek.title")}
    >
      <div>
        <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
          {t("deepseek.title")}
        </h3>
      </div>
      {changes.clearAll ? (
        <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-[var(--color-text-secondary)]">
          <span>{t("secretManager.clearAllPending")}</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={disabled || loading || loadFailed}
            onClick={() => onChange(setActorSecretClearAll(changes, false))}
          >
            {t("secretManager.undo")}
          </Button>
        </div>
      ) : null}
      <div className="grid min-w-0 grid-cols-1 gap-4 sm:grid-cols-2">
        {DEEPSEEK_SETTING_KEYS.map((key, index) => {
          const secret = key === "DEEPSEEK_API_KEY";
          const configured =
            !changes.clearAll && keys.includes(key) && !changes.unsetKeys.includes(key);
          const pending =
            Object.prototype.hasOwnProperty.call(changes.setVars, key) ||
            changes.unsetKeys.includes(key);
          const value = values[key] || "";
          const fallback = index === 0 ? DEEPSEEK_BASE_URL : index === 1 ? DEEPSEEK_MODEL : "sk-…";
          const label = t(`deepseek.${index === 0 ? "host" : index === 1 ? "model" : "apiKey"}`);
          const fieldId = `${id}-${index}`;
          return (
            <div
              key={key}
              className={index === 0 ? "min-w-0 space-y-1.5 sm:col-span-2" : "min-w-0 space-y-1.5"}
            >
              <div className="flex min-h-[44px] flex-wrap items-center justify-between gap-2 sm:min-h-[32px]">
                <label
                  htmlFor={fieldId}
                  className="text-xs font-medium text-[var(--color-text-secondary)]"
                >
                  {label}
                </label>
                <div className="flex flex-wrap items-center gap-2 text-xs text-[var(--color-text-muted)]">
                  {configured && !pending ? <span>{t("deepseek.configured")}</span> : null}
                  {pending ? (
                    <span>
                      {t(
                        changes.unsetKeys.includes(key)
                          ? "deepseek.pendingRemoval"
                          : "deepseek.pending",
                      )}
                    </span>
                  ) : null}
                  {pending ? (
                    <button
                      type="button"
                      disabled={controlsDisabled}
                      className="min-h-[44px] underline underline-offset-4 disabled:opacity-50 sm:min-h-[32px]"
                      onClick={() =>
                        onChange(undoActorSecretSet(undoActorSecretUnset(changes, key), key))
                      }
                    >
                      {t("secretManager.undo")}
                    </button>
                  ) : null}
                  {!secret ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="min-h-[44px] shrink-0 px-2 text-xs sm:min-h-[32px]"
                      disabled={controlsDisabled}
                      onClick={() => onChange(stageActorSecretSet(changes, key, fallback))}
                    >
                      {t("deepseek.useDefault")}
                    </Button>
                  ) : configured && !pending ? (
                    <button
                      type="button"
                      className="min-h-[44px] shrink-0 underline underline-offset-4 disabled:opacity-50 sm:min-h-[32px]"
                      disabled={controlsDisabled}
                      onClick={() => onChange(stageActorSecretUnset(changes, key))}
                    >
                      {t("deepseek.removeKey")}
                    </button>
                  ) : null}
                </div>
              </div>
              {secret ? (
                <SecretValueInput
                  id={fieldId}
                  ariaLabel={label}
                  value={value}
                  visible={showKey}
                  disabled={controlsDisabled}
                  placeholder={configured ? t("deepseek.keepSaved") : fallback}
                  showLabel={t("secretManager.showValue")}
                  hideLabel={t("secretManager.hideValue")}
                  onChange={(next) => update(key, next)}
                  onToggleVisibility={() => setShowKey((shown) => !shown)}
                />
              ) : (
                <Input
                  id={fieldId}
                  value={value}
                  type={index === 0 ? "url" : "text"}
                  list={index === 1 ? `${id}-models` : undefined}
                  autoComplete="off"
                  autoCapitalize="none"
                  spellCheck={false}
                  disabled={controlsDisabled}
                  placeholder={configured ? t("deepseek.keepSaved") : fallback}
                  className="font-mono"
                  onChange={(event) => update(key, event.target.value)}
                  aria-invalid={Boolean(
                    error && index < 2 && error === (index === 0 ? "hostInvalid" : "modelInvalid"),
                  )}
                />
              )}
            </div>
          );
        })}
      </div>
      <datalist id={`${id}-models`}>
        <option value={DEEPSEEK_MODEL} />
        <option value="deepseek-v4-pro" />
      </datalist>
      {hasPendingChanges ? (
        <DeepSeekPendingChanges
          disabled={disabled || loading || loadFailed || saveDisabled || Boolean(error)}
          onSave={onSave}
          onSaveAndRestart={onSaveAndRestart}
        />
      ) : null}
      {loading ? (
        <p role="status" className="text-xs text-[var(--color-text-muted)]">
          {t("secretManager.loading")}
        </p>
      ) : null}
      {error || loadFailed ? (
        <p role="alert" className="text-xs text-red-600 dark:text-red-400">
          {error ? t(`deepseek.${error}`) : t("failedToLoadSecrets")}
        </p>
      ) : null}
      <details className="text-xs text-[var(--color-text-muted)]">
        <summary className="cursor-pointer select-none">{t("deepseek.help")}</summary>
        <div className="mt-2 space-y-2 leading-5">
          <p>{t("deepseek.hostHint")}</p>
          <p>{t("deepseek.modelHint")}</p>
          <p>{t("deepseek.keyHint")}</p>
          <p>{t("deepseek.savedHint")}</p>
          <p>{t("deepseek.compatibilityHint")}</p>
        </div>
      </details>
    </section>
  );
}

export function DeepSeekCommandDetails({
  runtime,
  children,
}: {
  runtime: string;
  children: ReactNode;
}) {
  const { t } = useTranslation("actors");
  if (runtime !== "deepseek") return children;
  return (
    <details className="border-t border-[var(--glass-border-subtle)] pt-3">
      <summary className="cursor-pointer text-xs text-[var(--color-text-muted)]">
        {t("deepseek.launchDetails")}
      </summary>
      <div className="mt-3">{children}</div>
    </details>
  );
}
