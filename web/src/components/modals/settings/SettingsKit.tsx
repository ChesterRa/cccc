// Shared building blocks for every settings page: a titled page, grouped cards,
// "label left, control right" rows, a switch, a compact number field and a
// sticky save bar. Pages compose these instead of styling their own layout.
import { useId, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { classNames } from "../../../utils/classNames";
import { formatDuration } from "./automationUtils";
import { primaryButtonClass } from "./types";

export function SettingsPage({
  title,
  description,
  children,
  footer,
}: {
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  /** Usually a SettingsSaveBar; it sticks to the bottom of the scrolling page. */
  footer?: ReactNode;
}) {
  return (
    <div
      className="@container/settings flex min-w-0 flex-col gap-5 [overflow-wrap:anywhere]"
      data-settings-page
    >
      <header>
        <h2 className="text-xl font-semibold text-[var(--color-text-primary)]">{title}</h2>
        {description ? (
          <p className="mt-1.5 text-[13px] leading-6 text-[var(--color-text-secondary)]">
            {description}
          </p>
        ) : null}
      </header>
      {children}
      {footer}
    </div>
  );
}

export function SettingsCard({
  title,
  description,
  actions,
  tone = "default",
  children,
}: {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  tone?: "default" | "danger";
  children?: ReactNode;
}) {
  const titleId = useId();
  return (
    <section
      aria-labelledby={title ? titleId : undefined}
      data-settings-card={tone}
      className={classNames(
        "min-w-0 overflow-hidden rounded-[14px] border bg-[var(--color-bg-primary)]",
        tone === "danger"
          ? "border-rose-200 dark:border-rose-500/30"
          : "border-[var(--glass-border-subtle)]",
      )}
    >
      {title || actions ? (
        <div
          className={classNames(
            "flex flex-wrap items-center justify-between gap-3 px-5 py-3.5",
            children ? "border-b border-[var(--glass-border-subtle)]" : "",
          )}
        >
          <div className="min-w-0 flex-1 basis-64">
            {title ? (
              <h3
                id={titleId}
                className={classNames(
                  "text-sm font-semibold",
                  tone === "danger"
                    ? "text-rose-700 dark:text-rose-300"
                    : "text-[var(--color-text-primary)]",
                )}
              >
                {title}
              </h3>
            ) : null}
            {description ? (
              <p className="mt-0.5 text-xs leading-5 text-[var(--color-text-muted)]">
                {description}
              </p>
            ) : null}
          </div>
          {actions ? (
            // Card actions are compact (32px) whatever button size the page passes in. They
            // never shrink beside the title, but max-w-full caps the group at the header's
            // width so a group that is wider wraps its buttons instead of being clipped.
            <div className="flex max-w-full shrink-0 flex-wrap gap-2 [&_button]:h-8 [&_button]:min-h-8 [&_button]:px-3 [&_button]:py-0 [&_button]:text-[13px]">
              {actions}
            </div>
          ) : null}
        </div>
      ) : null}
      {children ? (
        <div className="divide-y divide-[var(--glass-border-subtle)]">{children}</div>
      ) : null}
    </section>
  );
}

/** One setting: label and help on the left, its control on the right (stacked on narrow screens). */
export function SettingsRow({
  label,
  help,
  htmlFor,
  labelId,
  stacked = false,
  children,
}: {
  label: ReactNode;
  help?: ReactNode;
  /** The control's id, so clicking the label focuses it. */
  htmlFor?: string;
  /** Id for the label text, for controls that use aria-labelledby. */
  labelId?: string;
  /** Put the control under the label (wide editors and previews). */
  stacked?: boolean;
  children: ReactNode;
}) {
  return (
    <div
      className={classNames(
        "grid min-w-0 gap-2 px-5 py-3.5",
        // Two columns only when the page itself is wide enough (sidebar, padding and
        // text scaling all narrow it), never by window width alone.
        stacked
          ? ""
          : "@min-[600px]/settings:grid-cols-[minmax(0,220px)_minmax(0,1fr)] @min-[600px]/settings:items-start @min-[600px]/settings:gap-6 @min-[900px]/settings:grid-cols-[280px_minmax(0,1fr)]",
      )}
      data-settings-row
    >
      <div className="min-w-0 @min-[600px]/settings:pt-1.5">
        {htmlFor ? (
          <label
            id={labelId}
            htmlFor={htmlFor}
            className="text-[13px] font-medium text-[var(--color-text-primary)]"
          >
            {label}
          </label>
        ) : (
          <span id={labelId} className="text-[13px] font-medium text-[var(--color-text-primary)]">
            {label}
          </span>
        )}
        {help ? (
          <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">{help}</p>
        ) : null}
      </div>
      {/* At least the 36px the label offset assumes, so short controls (switch, text) centre on it. */}
      <div className="flex min-w-0 flex-wrap items-center gap-2 @min-[600px]/settings:min-h-9">
        {children}
      </div>
    </div>
  );
}

export function SettingsSwitch({
  checked,
  onChange,
  labelledBy,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  labelledBy: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-labelledby={labelledBy}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={classNames(
        "inline-flex h-6 w-10 shrink-0 items-center rounded-full p-0.5 transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-text-secondary)] disabled:opacity-50",
        checked
          ? "justify-end bg-[var(--color-text-primary)]"
          : "justify-start bg-[var(--glass-border-subtle)]",
      )}
    >
      <span className="size-5 rounded-full bg-white shadow-sm" />
    </button>
  );
}

/** A number field sized to its content, with the duration it means beside it. */
export function SettingsNumberInput({
  id,
  value,
  onChange,
  min = 0,
  max,
  disabled = false,
  showDuration = false,
}: {
  id: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  disabled?: boolean;
  /** Seconds: show the value as a readable duration next to the field. */
  showDuration?: boolean;
}) {
  const { t } = useTranslation("settings");
  return (
    <>
      <input
        id={id}
        type="number"
        min={min}
        max={max}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
        className="glass-input h-9 w-28 rounded-[9px] px-2.5 text-[13px] text-[var(--color-text-primary)] disabled:cursor-not-allowed disabled:opacity-50"
      />
      {showDuration ? (
        <span className="font-mono text-xs text-[var(--color-text-muted)]">
          {formatDuration(value, t)}
        </span>
      ) : null}
    </>
  );
}

/** The page's save action, kept in view at the bottom of the scrolling page. */
export function SettingsSaveBar({
  label,
  busy,
  onSave,
  hint,
  actions,
  disabled = false,
}: {
  label: ReactNode;
  busy: boolean;
  onSave: () => void;
  hint?: ReactNode;
  /** Related actions such as discarding a draft or saving a Runtime Profile. */
  actions?: ReactNode;
  /** Nothing to save, or another action runs; unlike busy, keeps the label. */
  disabled?: boolean;
}) {
  const { t } = useTranslation("settings");
  return (
    <div
      className="sticky bottom-0 z-10 -mx-1 flex flex-wrap items-center justify-end gap-3 border-t border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)]/95 px-1 py-3 backdrop-blur"
      data-settings-save-bar
    >
      {hint ? (
        <span className="mr-auto text-[13px] text-[var(--color-text-secondary)]">{hint}</span>
      ) : null}
      {actions}
      <button
        type="button"
        onClick={onSave}
        disabled={busy || disabled}
        className={primaryButtonClass(busy)}
      >
        {busy ? t("common:saving") : label}
      </button>
    </div>
  );
}

/** A boxed note inside a page: a migration warning, a failure, or guidance that needs to stand out. */
export function SettingsNotice({
  tone = "info",
  action,
  children,
}: {
  tone?: "info" | "warning" | "error";
  /** A button that resolves the note, kept beside the text (below it when narrow). */
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div
      role={tone === "error" ? "alert" : undefined}
      data-settings-notice={tone}
      className={classNames(
        "flex min-w-0 flex-wrap items-center gap-x-4 gap-y-2 rounded-[10px] border px-3.5 py-2.5 text-[13px] leading-5",
        tone === "error"
          ? "border-rose-500/20 bg-rose-500/10 text-rose-700 dark:text-rose-300"
          : tone === "warning"
            ? "border-amber-500/30 bg-amber-500/10 text-amber-800 dark:text-amber-200"
            : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]",
      )}
    >
      <div className="min-w-0 flex-1 basis-60">{children}</div>
      {action ? (
        <div className="flex max-w-full shrink-0 flex-wrap gap-2 [&_button]:h-8 [&_button]:min-h-8 [&_button]:px-3 [&_button]:py-0 [&_button]:text-[13px]">
          {action}
        </div>
      ) : null}
    </div>
  );
}
