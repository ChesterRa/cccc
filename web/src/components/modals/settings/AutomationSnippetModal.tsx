import React from "react";
import { useTranslation } from "react-i18next";
import { BodyPortal } from "../../ui/BodyPortal";

import { getAutomationVarHelp } from "./automationUtils";
import {
  cardClass,
  dangerButtonClass,
  inputClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsDialogBodyClass,
  settingsDialogFooterClass,
  settingsDialogHeaderClass,
  settingsDialogPanelClass,
} from "./types";

interface AutomationSnippetModalProps {
  open: boolean;
  isDark: boolean;
  templateErr: string;
  saveErr: string;
  saveBusy: boolean;
  newSnippetId: string;
  supportedVars: string[];
  snippetIds: string[];
  snippets: Record<string, string>;
  builtinSnippetDefaults: Record<string, string>;
  builtinOverrideIds: string[];
  onClose: () => void;
  onNewSnippetIdChange: (next: string) => void;
  onAddSnippet: () => void;
  onDeleteSnippet: (snippetId: string) => void;
  onResetBuiltinSnippet: (snippetId: string) => void;
  onUpdateSnippet: (snippetId: string, content: string) => void;
  onSave: () => void | Promise<void>;
}

export function AutomationSnippetModal(props: AutomationSnippetModalProps) {
  const {
    open,
    isDark,
    templateErr,
    saveErr,
    saveBusy,
    newSnippetId,
    supportedVars,
    snippetIds,
    snippets,
    builtinSnippetDefaults,
    builtinOverrideIds,
    onClose,
    onNewSnippetIdChange,
    onAddSnippet,
    onDeleteSnippet,
    onResetBuiltinSnippet,
    onUpdateSnippet,
    onSave,
  } = props;

  const { t } = useTranslation("settings");
  const automationVarHelp = getAutomationVarHelp(t);
  const builtinOverrideSet = new Set(builtinOverrideIds);

  if (!open) return null;

  const content = (
    <div className="fixed inset-0 z-[1000]" role="dialog" aria-modal="true">
      <div className="absolute inset-0 glass-overlay" onPointerDown={onClose} />
      <div className={settingsDialogPanelClass("lg")}>
        <div className={settingsDialogHeaderClass}>
          <div className="min-w-0">
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t("snippetModal.title")}
            </div>
            <div className="mt-1 text-[11px] text-[var(--color-text-tertiary)]">
              {t("snippetModal.description")}
            </div>
          </div>
          <button
            type="button"
            className={`${secondaryButtonClass("sm")} ml-auto`}
            onClick={onClose}
          >
            {t("common:close")}
          </button>
        </div>

        <div className={settingsDialogBodyClass}>
          <div className="space-y-4">
            {templateErr ? (
              <div className="text-xs text-rose-600 dark:text-rose-300">{templateErr}</div>
            ) : null}
            {!templateErr && saveErr ? (
              <div className="text-xs text-rose-600 dark:text-rose-300">{saveErr}</div>
            ) : null}
            <div className="grid grid-cols-1 sm:grid-cols-[1fr_auto] gap-2">
              <input
                value={newSnippetId}
                onChange={(e) => onNewSnippetIdChange(e.target.value)}
                className={`${inputClass(isDark)} font-mono`}
                placeholder="snippet_name"
                spellCheck={false}
              />
              <button type="button" className={secondaryButtonClass()} onClick={onAddSnippet}>
                {t("snippetModal.addSnippet")}
              </button>
            </div>

            {supportedVars.length > 0 ? (
              <div className="rounded-lg border border-[var(--glass-border-subtle)] p-2.5 text-[11px] bg-[var(--glass-panel-bg)] text-[var(--color-text-tertiary)]">
                <div className="font-semibold mb-1 text-[var(--color-text-secondary)]">
                  {t("snippetModal.availablePlaceholders")}
                </div>
                <div className="space-y-1">
                  {supportedVars.map((v) => {
                    const help = automationVarHelp[v];
                    return (
                      <div key={v}>
                        <span className="font-mono">{`{{${v}}}`}</span>
                        <span>{` - ${help?.description || t("automation.placeholderBuiltIn")}`}</span>
                        <span className="text-[var(--color-text-muted)]">{` (${t("automation.exampleLabel")}: ${help?.example || "-"})`}</span>
                      </div>
                    );
                  })}
                </div>
              </div>
            ) : null}

            {snippetIds.length === 0 ? (
              <div className="text-sm text-[var(--color-text-tertiary)]">
                {t("snippetModal.noSnippets")}
              </div>
            ) : null}

            <div className="space-y-3">
              {snippetIds.map((snippetId) => {
                const isBuiltIn = builtinSnippetDefaults[snippetId] !== undefined;
                const isOverridden = builtinOverrideSet.has(snippetId) && isBuiltIn;
                const content = snippets[snippetId] ?? builtinSnippetDefaults[snippetId] ?? "";
                return (
                  <div key={snippetId} className={cardClass(isDark)}>
                    <div className="flex items-center justify-between gap-2 mb-2">
                      <div className="min-w-0">
                        <div className="text-xs font-semibold font-mono text-[var(--color-text-primary)]">
                          {snippetId}
                        </div>
                        <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--color-text-tertiary)]">
                          <span className="rounded-full border border-[var(--glass-border-subtle)] px-2 py-0.5">
                            {isBuiltIn
                              ? t("snippetModal.builtInBadge")
                              : t("snippetModal.customBadge")}
                          </span>
                          {isOverridden ? (
                            <span className="rounded-full border border-[var(--glass-border-subtle)] px-2 py-0.5">
                              {t("snippetModal.overrideBadge")}
                            </span>
                          ) : null}
                        </div>
                      </div>
                      {isBuiltIn ? (
                        <button
                          type="button"
                          className={secondaryButtonClass("sm")}
                          onClick={() => onResetBuiltinSnippet(snippetId)}
                          title={t("snippetModal.resetSnippet")}
                        >
                          {t("snippetModal.resetSnippet")}
                        </button>
                      ) : (
                        <button
                          type="button"
                          className={dangerButtonClass("sm")}
                          onClick={() => onDeleteSnippet(snippetId)}
                          title={t("snippetModal.deleteSnippet")}
                        >
                          {t("common:delete")}
                        </button>
                      )}
                    </div>
                    <textarea
                      value={content}
                      onChange={(e) => onUpdateSnippet(snippetId, e.target.value)}
                      className={`${inputClass(isDark)} font-mono text-[12px]`}
                      style={{ minHeight: 140 }}
                      spellCheck={false}
                    />
                  </div>
                );
              })}
            </div>
          </div>
        </div>
        <div className={settingsDialogFooterClass}>
          <button
            type="button"
            onClick={onClose}
            className={secondaryButtonClass()}
            disabled={saveBusy}
          >
            {t("common:cancel")}
          </button>
          <button
            type="button"
            onClick={() => void onSave()}
            className={primaryButtonClass(saveBusy)}
            disabled={saveBusy}
          >
            {saveBusy ? t("common:saving") : t("common:save")}
          </button>
        </div>
      </div>
    </div>
  );

  return <BodyPortal>{content}</BodyPortal>;
}
