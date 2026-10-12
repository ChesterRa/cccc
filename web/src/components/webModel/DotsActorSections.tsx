import type { ReactNode } from "react";
import type { DotsActorState } from "../../services/api";
import { DELIVERY_STEPS, deliveryProgress, type StepState } from "./dotsActorStatus";

type T = (key: string, options?: Record<string, unknown>) => string;

export const dotsCard =
  "rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]";

/** The three facts a Dots Actor's owner checks first. */
export function DotsStatusSummary({
  state,
  t,
  subscription,
  lastTask,
}: {
  state: DotsActorState | null;
  t: T;
  subscription: string;
  lastTask: string;
}) {
  const cells: Array<[string, string]> = [
    [t("dotsActor.status.subscription"), subscription],
    [t("dotsActor.status.pending"), t("dotsActor.pending", { count: state?.pending_count ?? 0 })],
    [t("dotsActor.status.lastTask"), lastTask],
  ];
  return (
    <dl className={`${dotsCard} grid grid-cols-1 overflow-hidden @xl/dots:grid-cols-3`}>
      {cells.map(([label, value], index) => (
        <div
          key={label}
          className={`px-4 py-3 ${index ? "border-t border-[var(--glass-border-subtle)] @xl/dots:border-t-0 @xl/dots:border-l" : ""}`}
        >
          <dt className="text-xs text-[var(--color-text-muted)]">{label}</dt>
          <dd className="mt-1 font-semibold">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

const stepDot: Record<StepState, string> = {
  done: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
  current: "bg-amber-500/15 text-amber-700 dark:text-amber-300",
  error: "bg-rose-500/15 text-rose-700 dark:text-rose-300",
  todo: "border border-[var(--glass-border-subtle)] text-[var(--color-text-muted)]",
};

export function DotsDeliverySteps({ state, t }: { state: DotsActorState; t: T }) {
  const progress = deliveryProgress(state);
  return (
    <section className={`${dotsCard} flex flex-col`} aria-label={t("dotsActor.delivery.title")}>
      <div className="border-b border-[var(--glass-border-subtle)] px-4 py-3">
        <h3 className="font-semibold">{t("dotsActor.delivery.title")}</h3>
        <p className="mt-0.5 text-xs text-[var(--color-text-muted)]">
          {t("dotsActor.delivery.hint")}
        </p>
      </div>
      {progress ? (
        <ol className="flex-1 px-4">
          {DELIVERY_STEPS.map((step, index) => {
            const stepState: StepState =
              index < progress.finished
                ? "done"
                : index === progress.finished
                  ? progress.next
                  : "todo";
            return (
              <li
                key={step}
                data-step={step}
                data-state={stepState}
                className="flex items-start gap-3 border-b border-[var(--glass-border-subtle)] py-3 last:border-b-0"
              >
                <span
                  className={`inline-flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${stepDot[stepState]}`}
                  aria-hidden="true"
                >
                  {index + 1}
                </span>
                <div className="min-w-0">
                  <div className="font-medium">{t(`dotsActor.steps.${step}`)}</div>
                  <div className="mt-0.5 text-xs leading-relaxed text-[var(--color-text-muted)]">
                    {t(`dotsActor.steps.${step}Hint`)}
                  </div>
                </div>
              </li>
            );
          })}
        </ol>
      ) : (
        <p className="flex-1 px-4 py-6 text-xs text-[var(--color-text-muted)]">
          {t("dotsActor.delivery.empty")}
        </p>
      )}
    </section>
  );
}

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
 * A numbered setup step. The active one is highlighted and fills spare height;
 * a done one shows a check and drops its controls, since nothing is left to do.
 */
export function DotsSetupStep({
  number,
  title,
  hint,
  active = false,
  done = false,
  children,
}: {
  number: number;
  title: string;
  hint: string;
  active?: boolean;
  done?: boolean;
  children?: ReactNode;
}) {
  return (
    <li
      data-setup-step={number}
      data-done={done || undefined}
      className={`flex gap-3 border-t border-[var(--glass-border-subtle)] px-4 py-4 first:border-t-0 ${active ? "flex-1 bg-[var(--glass-tab-bg)]/40" : ""}`}
    >
      <span
        className={`inline-flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${
          done
            ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
            : active
              ? "bg-[var(--color-text-primary)] text-[var(--color-bg-primary)]"
              : "border border-[var(--glass-border-subtle)] text-[var(--color-text-muted)]"
        }`}
        aria-hidden="true"
      >
        {done ? (
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="m5 12 5 5 9-10" />
          </svg>
        ) : (
          number
        )}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div>
          <div className="font-semibold">{title}</div>
          <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-text-muted)]">{hint}</p>
        </div>
        {done ? null : children}
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
      {children ? <div className="mt-auto pt-4">{children}</div> : null}
    </section>
  );
}
