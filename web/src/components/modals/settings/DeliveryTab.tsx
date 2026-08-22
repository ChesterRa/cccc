// DeliveryTab configures runtime delivery pacing and one-shot reminder windows.
import { useTranslation } from "react-i18next";

import { ClockIcon } from "../../Icons";
import { NumberInputRow } from "./automationUtils";
import {
  primaryButtonClass,
  settingsWorkspaceActionBarClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";

interface DeliveryTabProps {
  isDark: boolean;
  busy: boolean;
  minIntervalSeconds: number;
  setMinIntervalSeconds: (v: number) => void;
  mailNoticeAfterSeconds: number;
  setMailNoticeAfterSeconds: (v: number) => void;
  replyNoticeAfterSeconds: number;
  setReplyNoticeAfterSeconds: (v: number) => void;
  onSave: () => void;
}

export function DeliveryTab(props: DeliveryTabProps) {
  const { isDark, busy, onSave } = props;
  const { t } = useTranslation("settings");

  return (
    <div className="animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t("delivery.title")}
            </h3>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("delivery.description")}
            </p>
          </div>
        </div>

        <div className={`${settingsWorkspaceBodyClass} grid grid-cols-1 gap-3 lg:grid-cols-3`}>
          <div className={settingsWorkspaceSoftPanelClass(isDark)}>
            <NumberInputRow
              isDark={isDark}
              label={t("delivery.minInterval")}
              value={props.minIntervalSeconds}
              onChange={props.setMinIntervalSeconds}
              helperText={t("delivery.minIntervalHelp")}
            />
          </div>
          <div className={settingsWorkspaceSoftPanelClass(isDark)}>
            <NumberInputRow
              isDark={isDark}
              label={t("delivery.mailNotice")}
              value={props.mailNoticeAfterSeconds}
              onChange={props.setMailNoticeAfterSeconds}
              helperText={t("delivery.mailNoticeHelp")}
            />
          </div>
          <div className={settingsWorkspaceSoftPanelClass(isDark)}>
            <NumberInputRow
              isDark={isDark}
              label={t("delivery.replyNotice")}
              value={props.replyNoticeAfterSeconds}
              onChange={props.setReplyNoticeAfterSeconds}
              helperText={t("delivery.replyNoticeHelp")}
            />
          </div>
        </div>

        <div className={settingsWorkspaceActionBarClass(isDark)}>
          <button onClick={onSave} disabled={busy} className={primaryButtonClass(busy)}>
            {busy ? (
              t("common:saving")
            ) : (
              <span className="flex items-center gap-2">
                <ClockIcon className="w-4 h-4" /> {t("delivery.saveDelivery")}
              </span>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
