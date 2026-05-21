// DeliveryTab configures PTY delivery read-cursor behavior.
import React from "react";
import { useTranslation } from "react-i18next";

import { ClockIcon } from "../../Icons";
import {
  labelClass,
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
  autoMarkOnDelivery: boolean;
  setAutoMarkOnDelivery: (v: boolean) => void;
  onSave: () => void;
  onAutoSave?: (field: string, value: boolean) => void;
}

const ToggleRow = ({
  label,
  checked,
  onChange,
  isDark,
  helperText,
  onAutoSave,
}: {
  label: string;
  checked: boolean;
  onChange: (val: boolean) => void;
  isDark: boolean;
  helperText?: React.ReactNode;
  onAutoSave?: (newValue: boolean) => void;
}) => (
  <div className={settingsWorkspaceSoftPanelClass(isDark)}>
    <label className="flex items-center justify-between cursor-pointer">
      <span className={`pr-4 ${labelClass(isDark)}`}>{label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => {
          const newValue = !checked;
          onChange(newValue);
          onAutoSave?.(newValue);
        }}
        className={`
          relative inline-flex h-7 w-12 shrink-0 items-center rounded-full border transition-colors duration-300 ease-spring
          focus:outline-none focus:ring-2 focus:ring-offset-2
          ${checked
            ? "border-emerald-500 bg-emerald-500 focus:ring-emerald-500"
            : "border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] focus:ring-offset-white dark:focus:ring-offset-slate-900"
          }
        `}
      >
        <span
          className={`
            absolute left-0.5 inline-block h-6 w-6 rounded-full bg-white shadow-[0_2px_4px_rgba(0,0,0,0.08),0_1px_1px_rgba(0,0,0,0.04)] transform transition-transform duration-300 ease-spring
            ${checked ? "translate-x-5" : "translate-x-0"}
          `}
        />
      </button>
    </label>
    {helperText && (
      <div className="mt-1.5 text-[11px] leading-snug text-[var(--color-text-muted)]">
        {helperText}
      </div>
    )}
  </div>
);

export function DeliveryTab(props: DeliveryTabProps) {
  const { isDark, busy, onSave, onAutoSave } = props;
  const { t } = useTranslation("settings");

  const autoSave = (field: string, getValue: () => boolean) => {
    if (!onAutoSave) return;
    setTimeout(() => onAutoSave(field, getValue()), 0);
  };

  return (
    <div className="animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("delivery.title")}</h3>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("delivery.description")}
            </p>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <ToggleRow
            isDark={isDark}
            label={t("delivery.autoMarkRead")}
            checked={props.autoMarkOnDelivery}
            onChange={props.setAutoMarkOnDelivery}
            helperText={t("delivery.autoMarkReadHelp")}
            onAutoSave={(newValue) => autoSave("auto_mark_on_delivery", () => newValue)}
          />
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
