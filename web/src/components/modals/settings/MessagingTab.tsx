import React from "react";
import { useTranslation, Trans } from "react-i18next";

import { cardClass, inputClass, labelClass, primaryButtonClass } from "./types";

interface MessagingTabProps {
  isDark: boolean;
  busy: boolean;
  defaultSendTo: "foreman" | "broadcast";
  setDefaultSendTo: (v: "foreman" | "broadcast") => void;
  onSave: () => void;
}

const MessageSquareIcon = ({ className }: { className?: string }) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
  >
    <path d="M21 15a4 4 0 0 1-4 4H7l-4 4V7a4 4 0 0 1 4-4h10a4 4 0 0 1 4 4z" />
  </svg>
);

export function MessagingTab(props: MessagingTabProps) {
  const { isDark, busy, defaultSendTo, setDefaultSendTo, onSave } = props;
  const { t } = useTranslation("settings");

  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("messaging.title")}</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("messaging.description")}
        </p>
      </div>

      <div className={cardClass(isDark)}>
        <div className="flex items-center gap-2 mb-1">
          <div className={`p-1.5 rounded-md ${isDark ? "bg-slate-800 text-emerald-400" : "bg-emerald-50 text-emerald-700"}`}>
            <MessageSquareIcon className="w-4 h-4" />
          </div>
          <h3 className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>{t("messaging.defaultRecipient")}</h3>
        </div>
        <p className={`text-xs ml-9 mb-4 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          <Trans i18nKey="messaging.defaultRecipientDescription" ns="settings" components={[<span className="font-mono" />]} />
        </p>

        <div className="ml-1">
          <label className={labelClass(isDark)}>{t("messaging.whenNoRecipients")}</label>
          <select
            value={defaultSendTo}
            onChange={(e) => setDefaultSendTo((e.target.value as "foreman" | "broadcast") || "foreman")}
            className={`${inputClass(isDark)} cursor-pointer`}
          >
            <option value="foreman">{t("messaging.foremanOnly")}</option>
            <option value="broadcast">{t("messaging.broadcastAll")}</option>
          </select>
          <div className={`mt-2 text-[11px] leading-snug ${isDark ? "text-slate-500" : "text-gray-500"}`}>
            {t("messaging.tip")}
          </div>
        </div>
      </div>

      <div className="pt-2">
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
  );
}

