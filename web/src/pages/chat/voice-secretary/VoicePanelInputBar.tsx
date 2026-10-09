import type { KeyboardEvent } from "react";
import { useId } from "react";
import { StopIcon } from "../../../components/Icons";
import { classNames } from "../../../utils/classNames";

export type VoicePanelInputAction = {
  key: string;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
};

/** Bottom bar of a workspace view: one microphone for this view plus a typed request. */
export function VoicePanelInputBar({
  isDark,
  recordLabel,
  recordTitle,
  recordDisabled,
  recording,
  recordingStarting,
  recordingTargetLabel,
  stopLabel,
  liveSnippet,
  onToggleRecord,
  value,
  onChange,
  placeholder,
  inputLabel,
  notice,
  actions,
}: {
  isDark: boolean;
  recordLabel: string;
  recordTitle?: string;
  recordDisabled: boolean;
  recording: boolean;
  recordingStarting: boolean;
  recordingTargetLabel: string;
  stopLabel: string;
  liveSnippet: string;
  onToggleRecord: () => void;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  inputLabel: string;
  notice?: string;
  actions: VoicePanelInputAction[];
}) {
  const noticeId = useId();
  const active = recording || recordingStarting;
  const primary = actions.find((action) => action.primary && !action.disabled);
  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey) || !primary) return;
    event.preventDefault();
    primary.onClick();
  };
  return (
    <div data-voice-input-bar className="mt-3 shrink-0">
      {active ? (
        <div
          data-voice-input-recording
          className={classNames(
            "mb-2 flex min-w-0 items-center gap-2 rounded-lg px-2.5 py-1.5 text-xs",
            isDark ? "bg-rose-500/12 text-rose-100" : "bg-rose-50 text-rose-800",
          )}
        >
          <span
            aria-hidden="true"
            className="h-2 w-2 shrink-0 animate-pulse rounded-full bg-rose-500"
          />
          <span className="shrink-0 font-semibold">{recordingTargetLabel}</span>
          {liveSnippet ? (
            <span className="min-w-0 flex-1 truncate opacity-80" title={liveSnippet}>
              {liveSnippet}
            </span>
          ) : (
            <span className="flex-1" />
          )}
        </div>
      ) : null}
      {notice ? (
        <p id={noticeId} className="mb-2 text-xs text-amber-700 dark:text-amber-300">
          {notice}
        </p>
      ) : null}
      <div className="rounded-xl border border-[var(--glass-panel-border)] bg-[var(--color-bg-primary)] shadow-sm transition-colors focus-within:border-[var(--color-border-focus)]">
        <textarea
          data-voice-instruction-input
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          aria-label={inputLabel}
          aria-describedby={notice ? noticeId : undefined}
          rows={2}
          className="block max-h-40 min-h-[3.25rem] w-full resize-none rounded-t-xl bg-transparent px-3.5 pt-3 text-sm leading-5 text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)]"
        />
        <div className="flex min-w-0 flex-wrap items-center gap-2 px-2 pb-2 pt-1">
          <button
            type="button"
            data-voice-record
            aria-pressed={active}
            disabled={recordingStarting || (!active && recordDisabled)}
            title={recordTitle}
            onClick={onToggleRecord}
            className={classNames(
              "inline-flex min-h-9 shrink-0 items-center justify-center gap-2 rounded-lg px-3 text-sm font-semibold transition-colors disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]",
              active
                ? isDark
                  ? "bg-rose-500/20 text-rose-100 hover:bg-rose-500/28"
                  : "bg-rose-50 text-rose-800 hover:bg-rose-100"
                : "text-[var(--color-text-primary)] hover:bg-[var(--color-bg-tertiary)]",
            )}
          >
            {active ? (
              <StopIcon size={13} aria-hidden="true" />
            ) : (
              <span aria-hidden="true" className="h-2.5 w-2.5 rounded-full bg-rose-600" />
            )}
            {active ? stopLabel : recordLabel}
          </button>
          <div className="ml-auto flex shrink-0 items-center gap-1.5">
            {actions.map((action) => (
              <button
                key={action.key}
                type="button"
                data-voice-input-action={action.key}
                disabled={action.disabled}
                onClick={action.onClick}
                className={classNames(
                  "min-h-9 rounded-lg px-3 text-sm font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]",
                  action.primary
                    ? "bg-[var(--primary)] text-[var(--primary-foreground)] hover:opacity-90 disabled:bg-[var(--color-bg-tertiary)] disabled:text-[var(--color-text-tertiary)]"
                    : "text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] hover:text-[var(--color-text-primary)] disabled:text-[var(--color-text-tertiary)] disabled:hover:bg-transparent",
                )}
              >
                {action.label}
              </button>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
