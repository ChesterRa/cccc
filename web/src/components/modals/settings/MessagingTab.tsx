import React from "react";
import { useTranslation, Trans } from "react-i18next";

import { MessageSquareIcon } from "../../Icons";
import { SelectCombobox } from "../../SelectCombobox";
import {
  inputClass,
  labelClass,
  primaryButtonClass,
  settingsWorkspaceActionBarClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";

interface MessagingTabProps {
  isDark: boolean;
  busy: boolean;
  defaultSendTo: "foreman" | "broadcast";
  setDefaultSendTo: (v: "foreman" | "broadcast") => void;
  onSave: () => void;
}

export function MessagingTab(props: MessagingTabProps) {
  const { isDark, busy, defaultSendTo, setDefaultSendTo, onSave } = props;
  const { t } = useTranslation("settings");

  return (
    <div className="animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("messaging.title")}</h3>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("messaging.description")}
            </p>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className={settingsWorkspaceSoftPanelClass(isDark)}>
            <div className="mb-1 flex items-center gap-2">
              <div className="rounded-xl bg-emerald-500/15 p-1.5 text-emerald-700 dark:text-emerald-400">
                <MessageSquareIcon className="h-4 w-4" />
              </div>
              <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("messaging.defaultRecipient")}</h3>
            </div>
            <p className="mb-5 ml-9 text-xs text-[var(--color-text-muted)]">
              <Trans i18nKey="messaging.defaultRecipientDescription" ns="settings" components={[<span className="font-mono" />]} />
            </p>

            <div className="ml-1 space-y-2.5">
              <label className={labelClass(isDark)}>{t("messaging.whenNoRecipients")}</label>
              <SelectCombobox
                items={[
                  { value: "foreman", label: t("messaging.foremanOnly") },
                  { value: "broadcast", label: t("messaging.broadcastAll") },
                ]}
                value={defaultSendTo}
                onChange={(value) => setDefaultSendTo((value as "foreman" | "broadcast") || "foreman")}
                ariaLabel={t("messaging.whenNoRecipients")}
                className={inputClass(isDark)}
              />
              <div className="text-[11px] leading-snug text-[var(--color-text-muted)]">
                {t("messaging.tip")}
              </div>
            </div>
          </div>
        </div>

        <div className={settingsWorkspaceActionBarClass(isDark)}>
          <button onClick={onSave} disabled={busy} className={primaryButtonClass(busy)}>
            {busy ? (
              t("common:saving")
            ) : (
              <span className="flex items-center gap-2">
                <MessageSquareIcon className="w-4 h-4" /> {t("messaging.saveMessaging")}
              </span>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
