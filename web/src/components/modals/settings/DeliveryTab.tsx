// DeliveryTab configures PTY message delivery behavior (throttling + read-cursor policy).
import React from "react";
import { useTranslation } from "react-i18next";

import { cardClass, inputClass, labelClass, primaryButtonClass } from "./types";

interface DeliveryTabProps {
  isDark: boolean;
  busy: boolean;
  deliveryInterval: number;
  setDeliveryInterval: (v: number) => void;
  autoMarkOnDelivery: boolean;
  setAutoMarkOnDelivery: (v: boolean) => void;
  onSave: () => void;
  onAutoSave?: (field: string, value: number | boolean) => void;
}

const TruckIcon = ({ className }: { className?: string }) => (
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
    <path d="M5 18H3c-.6 0-1-.4-1-1V7c0-.6.4-1 1-1h10c.6 0 1 .4 1 1v11" />
    <path d="M14 9h4l4 4v4c0 .6-.4 1-1 1h-2" />
    <circle cx="7" cy="18" r="2" />
    <circle cx="17" cy="18" r="2" />
  </svg>
);

const ClockIcon = ({ className }: { className?: string }) => (
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
    <circle cx="12" cy="12" r="10" />
    <polyline points="12 6 12 12 16 14" />
  </svg>
);

const formatDuration = (secondsRaw: number): string => {
  const seconds = Number.isFinite(secondsRaw) ? Math.max(0, Math.trunc(secondsRaw)) : 0;
  if (seconds <= 0) return "Off";
  const parts: string[] = [];
  let rem = seconds;
  const units: Array<[number, string]> = [
    [86400, "d"],
    [3600, "h"],
    [60, "m"],
    [1, "s"],
  ];
  for (const [unit, label] of units) {
    if (rem < unit) continue;
    const v = Math.floor(rem / unit);
    rem -= v * unit;
    parts.push(`${v}${label}`);
    if (parts.length >= 2) break;
  }
  return parts.join(" ");
};

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
  <div className="w-full">
    <label className="flex items-center justify-between cursor-pointer">
      <span className={labelClass(isDark)}>{label}</span>
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
          relative inline-flex h-6 w-11 items-center rounded-full transition-colors
          focus:outline-none focus:ring-2 focus:ring-offset-2
          ${checked
            ? "bg-emerald-500 focus:ring-emerald-500"
            : isDark
              ? "bg-slate-600 focus:ring-slate-500"
              : "bg-gray-300 focus:ring-gray-400"
          }
          ${isDark ? "focus:ring-offset-slate-900" : "focus:ring-offset-white"}
        `}
      >
        <span
          className={`
            inline-block h-4 w-4 rounded-full bg-white shadow-sm transform transition-transform
            ${checked ? "translate-x-6" : "translate-x-1"}
          `}
        />
      </button>
    </label>
    {helperText && (
      <div className={`mt-1.5 text-[11px] leading-snug ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        {helperText}
      </div>
    )}
  </div>
);

const NumberInputRow = ({
  label,
  value,
  onChange,
  isDark,
  min = 0,
  helperText,
  onAutoSave,
}: {
  label: string;
  value: number;
  onChange: (val: number) => void;
  isDark: boolean;
  min?: number;
  helperText?: React.ReactNode;
  onAutoSave?: () => void;
}) => (
  <div className="w-full">
    <label className={labelClass(isDark)}>{label}</label>
    <div className="relative">
      <input
        type="number"
        min={min}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        onBlur={() => onAutoSave?.()}
        className={inputClass(isDark)}
      />
      <div
        className={`
          absolute right-3 top-1/2 -translate-y-1/2 text-xs font-mono
          pointer-events-none transition-opacity duration-200
          ${isDark ? "text-slate-600" : "text-gray-400"}
        `}
      >
        {formatDuration(value)}
      </div>
    </div>
    {helperText && (
      <div className={`mt-1.5 text-[11px] leading-snug ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        {helperText}
      </div>
    )}
  </div>
);

const DeliverySection = ({
  isDark,
  title,
  description,
  children,
}: {
  isDark: boolean;
  title: string;
  description: string;
  children: React.ReactNode;
}) => (
  <div className={cardClass(isDark)}>
    <div className="flex items-center gap-2 mb-1">
      <div className={`p-1.5 rounded-md ${isDark ? "bg-slate-800 text-indigo-400" : "bg-indigo-50 text-indigo-600"}`}>
        <TruckIcon className="w-4 h-4" />
      </div>
      <h3 className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>{title}</h3>
    </div>
    <p className={`text-xs ml-9 mb-4 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{description}</p>
    <div className="space-y-4 ml-1">{children}</div>
  </div>
);

export function DeliveryTab(props: DeliveryTabProps) {
  const { isDark, busy, onSave, onAutoSave } = props;
  const { t } = useTranslation("settings");

  const autoSave = (field: string, getValue: () => number | boolean) => {
    if (!onAutoSave) return;
    setTimeout(() => onAutoSave(field, getValue()), 0);
  };

  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("delivery.title")}</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("delivery.description")}
        </p>
      </div>

      <DeliverySection
        isDark={isDark}
        title={t("delivery.throttleTitle")}
        description={t("delivery.throttleDescription")}
      >
        <NumberInputRow
          isDark={isDark}
          label={t("delivery.deliveryInterval")}
          value={props.deliveryInterval}
          onChange={props.setDeliveryInterval}
          helperText={t("delivery.deliveryIntervalHelp")}
          onAutoSave={() => autoSave("min_interval_seconds", () => props.deliveryInterval)}
        />
        <ToggleRow
          isDark={isDark}
          label={t("delivery.autoMarkRead")}
          checked={props.autoMarkOnDelivery}
          onChange={props.setAutoMarkOnDelivery}
          helperText={t("delivery.autoMarkReadHelp")}
          onAutoSave={(newValue) => autoSave("auto_mark_on_delivery", () => newValue)}
        />
      </DeliverySection>

      <div className="pt-2">
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
  );
}

