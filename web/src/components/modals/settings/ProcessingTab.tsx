// ProcessingTab configures processing state tracking settings.
import React from "react";
import { cardClass, inputClass, labelClass, primaryButtonClass } from "./types";

interface ProcessingTabProps {
  isDark: boolean;
  busy: boolean;
  processingEnabled: boolean;
  setProcessingEnabled: (v: boolean) => void;
  processingTimeoutSec: number;
  setProcessingTimeoutSec: (v: number) => void;
  mcpActivityGraceSec: number;
  setMcpActivityGraceSec: (v: number) => void;
  nudgeMaxCount: number;
  setNudgeMaxCount: (v: number) => void;
  onSave: () => void;
}

// --- Icons ---

const CpuIcon = ({ className }: { className?: string }) => (
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
    <rect x="4" y="4" width="16" height="16" rx="2" />
    <rect x="9" y="9" width="6" height="6" />
    <path d="M9 1v3" />
    <path d="M15 1v3" />
    <path d="M9 20v3" />
    <path d="M15 20v3" />
    <path d="M20 9h3" />
    <path d="M20 14h3" />
    <path d="M1 9h3" />
    <path d="M1 14h3" />
  </svg>
);

// --- Utilities ---

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

// --- Components ---

const ToggleRow = ({
  label,
  checked,
  onChange,
  isDark,
  helperText,
}: {
  label: string;
  checked: boolean;
  onChange: (val: boolean) => void;
  isDark: boolean;
  helperText?: React.ReactNode;
}) => (
  <div className="w-full">
    <label className="flex items-center justify-between cursor-pointer">
      <span className={labelClass(isDark)}>{label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
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
  formatValue = true,
  disabled = false,
}: {
  label: string;
  value: number;
  onChange: (val: number) => void;
  isDark: boolean;
  min?: number;
  helperText?: React.ReactNode;
  formatValue?: boolean;
  disabled?: boolean;
}) => (
  <div className="w-full">
    <label className={labelClass(isDark)}>{label}</label>
    <div className="relative">
      <input
        type="number"
        min={min}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        disabled={disabled}
        className={`${inputClass(isDark)} ${disabled ? "opacity-50 cursor-not-allowed" : ""}`}
      />
      {formatValue && (
        <div
          className={`
          absolute right-3 top-1/2 -translate-y-1/2 text-xs font-mono
          pointer-events-none transition-opacity duration-200
          ${isDark ? "text-slate-600" : "text-gray-400"}
        `}
        >
          {formatDuration(value)}
        </div>
      )}
    </div>
    {helperText && (
      <div className={`mt-1.5 text-[11px] leading-snug ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        {helperText}
      </div>
    )}
  </div>
);

// --- Main Export ---

export function ProcessingTab(props: ProcessingTabProps) {
  const { isDark, busy, onSave } = props;

  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-300">
      {/* Header */}
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Processing State Tracking</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          Detect when agents respond in terminal output instead of using MCP tools, and automatically remind them.
        </p>
      </div>

      {/* Main Section */}
      <div className={cardClass(isDark)}>
        <div className="flex items-center gap-2 mb-1">
          <div className={`p-1.5 rounded-md ${isDark ? "bg-slate-800 text-emerald-400" : "bg-emerald-50 text-emerald-600"}`}>
            <CpuIcon className="w-4 h-4" />
          </div>
          <h3 className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>MCP Response Tracking</h3>
        </div>
        <p className={`text-xs ml-9 mb-4 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          Track MCP call activity to detect and remind agents who may have responded in terminal output.
        </p>

        <div className="space-y-4 ml-1">
          <ToggleRow
            isDark={isDark}
            label="Enable Processing Tracking"
            checked={props.processingEnabled}
            onChange={props.setProcessingEnabled}
            helperText="When enabled, the system tracks MCP activity and sends reminders to agents who may have forgotten to use MCP tools."
          />

          <NumberInputRow
            isDark={isDark}
            label="Processing Timeout (sec)"
            value={props.processingTimeoutSec}
            onChange={props.setProcessingTimeoutSec}
            disabled={!props.processingEnabled}
            helperText="Time after message delivery before considering the agent may be stuck (minimum threshold)."
          />

          <NumberInputRow
            isDark={isDark}
            label="MCP Activity Grace Period (sec)"
            value={props.mcpActivityGraceSec}
            onChange={props.setMcpActivityGraceSec}
            disabled={!props.processingEnabled}
            helperText="Don't send reminders if agent made an MCP call within this time window."
          />

          <NumberInputRow
            isDark={isDark}
            label="Max Reminder Count"
            value={props.nudgeMaxCount}
            onChange={props.setNudgeMaxCount}
            disabled={!props.processingEnabled}
            formatValue={false}
            helperText={`Send up to ${props.nudgeMaxCount} reminder(s) per message before giving up.`}
          />
        </div>
      </div>

      {/* Info Box */}
      <div className={`rounded-lg border p-3 ${isDark ? "border-slate-700 bg-slate-800/50" : "border-blue-200 bg-blue-50"}`}>
        <p className={`text-xs ${isDark ? "text-slate-400" : "text-blue-700"}`}>
          <strong>How it works:</strong> When a message is delivered to an agent, the system starts tracking.
          If the agent makes MCP tool calls (like reading files, running commands), the timer resets.
          If no MCP response is sent after the timeout + grace period, a reminder is sent via system notification.
        </p>
      </div>

      {/* Actions */}
      <div className="pt-2">
        <button onClick={onSave} disabled={busy} className={primaryButtonClass(busy)}>
          {busy ? (
            "Saving..."
          ) : (
            <span className="flex items-center gap-2">
              <CpuIcon className="w-4 h-4" /> Save Processing Settings
            </span>
          )}
        </button>
      </div>
    </div>
  );
}
