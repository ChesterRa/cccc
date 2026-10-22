import type { ReactNode } from "react";

type T = (key: string, options?: Record<string, unknown>) => string;

/** One labelled setting with its action on the right. */
export function DotsSettingRow({
  label,
  hint,
  danger = false,
  children,
}: {
  label: string;
  hint?: ReactNode;
  danger?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--glass-border-subtle)] px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className={`font-medium ${danger ? "text-rose-700 dark:text-rose-300" : ""}`}>
          {label}
        </div>
        {hint ? (
          <div className="mt-0.5 text-xs leading-relaxed text-[var(--color-text-muted)]">
            {hint}
          </div>
        ) : null}
      </div>
      <div className="flex shrink-0 flex-wrap gap-2">{children}</div>
    </div>
  );
}

/**
 * A numbered setup step. The active one is highlighted.
 */
export function DotsSetupStep({
  number,
  title,
  hint,
  active = false,
  children,
}: {
  number: number;
  title: string;
  hint: string;
  active?: boolean;
  children?: ReactNode;
}) {
  return (
    <li
      data-setup-step={number}
      className={`flex gap-3 border-t border-[var(--glass-border-subtle)] px-4 py-4 first:border-t-0 ${active ? "bg-[var(--glass-tab-bg)]/40" : ""}`}
    >
      <span
        className={`inline-flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${
          active
            ? "bg-[var(--color-text-primary)] text-[var(--color-bg-primary)]"
            : "border border-[var(--glass-border-subtle)] text-[var(--color-text-muted)]"
        }`}
        aria-hidden="true"
      >
        {number}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div>
          <div className="font-semibold">{title}</div>
          <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-text-muted)]">{hint}</p>
        </div>
        {children}
      </div>
    </li>
  );
}

export function DotsSetupTips({ t, children }: { t: T; children?: ReactNode }) {
  return (
    <section
      className="flex flex-col rounded-xl border border-amber-500/30 bg-amber-500/5 px-4 py-3"
      aria-label={t("dotsActor.setup.tipsTitle")}
    >
      <h3 className="text-xs font-semibold text-amber-800 dark:text-amber-200">
        {t("dotsActor.setup.tipsTitle")}
      </h3>
      <ul className="mt-2 list-disc space-y-1 pl-4 text-xs leading-relaxed text-amber-900 dark:text-amber-100">
        <li>{t("dotsActor.setup.tipRefresh")}</li>
        <li>{t("dotsActor.setup.tipPermissions")}</li>
        <li>{t("dotsActor.setup.tipRetry")}</li>
      </ul>
      {children ? <div className="pt-4">{children}</div> : null}
    </section>
  );
}
