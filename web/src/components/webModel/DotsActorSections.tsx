import type { ReactNode } from "react";
import type { DotsActorState } from "../../services/api";
import { formatMessageTimestamp } from "../../utils/time";
import { AlertIcon, CheckIcon, ClockIcon, RefreshIcon } from "../Icons";
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
    [
      t("dotsActor.status.pending"),
      state ? t("dotsActor.pending", { count: state.pending_count }) : t("dotsActor.unavailable"),
    ],
    [t("dotsActor.status.lastTask"), lastTask],
  ];
  return (
    <dl className={`${dotsCard} grid shrink-0 grid-cols-1 overflow-hidden @xl/dots:grid-cols-3`}>
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
  const historical = progress?.finished === DELIVERY_STEPS.length;
  const title = t(historical ? "dotsActor.delivery.lastCompleted" : "dotsActor.delivery.title");
  const description = historical
    ? state.last_execution?.completed_at
      ? formatMessageTimestamp(state.last_execution.completed_at)
      : ""
    : t("dotsActor.delivery.hint");
  return (
    <section className={`${dotsCard} flex flex-col`} aria-label={title}>
      <div className="border-b border-[var(--glass-border-subtle)] px-4 py-3">
        <h3 className="font-semibold">{title}</h3>
        {description ? (
          <p className="mt-0.5 text-xs text-[var(--color-text-muted)]">{description}</p>
        ) : null}
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
            const sending = stepState === "current" && state.phase === "delivering";
            const spinning = stepState === "current" && (sending || state.phase === "working");
            const label = t(
              stepState === "error"
                ? `dotsActor.phases.${state.phase}`
                : sending
                  ? "dotsActor.steps.notifySending"
                  : `dotsActor.steps.${step}`,
            );
            const hint =
              stepState === "current"
                ? t(sending ? "dotsActor.steps.notifySendingHint" : `dotsActor.steps.${step}Hint`)
                : historical && step === "completed"
                  ? t("dotsActor.steps.completedHint")
                  : "";
            return (
              <li
                key={step}
                data-step={step}
                data-state={stepState}
                aria-current={stepState === "current" || stepState === "error" ? "step" : undefined}
                className="flex items-start gap-3 border-b border-[var(--glass-border-subtle)] py-3 last:border-b-0"
              >
                <span
                  className={`inline-flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${stepDot[stepState]}`}
                  aria-hidden="true"
                >
                  {stepState === "done" ? (
                    <CheckIcon className="size-3.5" />
                  ) : stepState === "error" ? (
                    <AlertIcon className="size-3.5" />
                  ) : spinning ? (
                    <RefreshIcon className="size-3.5 animate-spin motion-reduce:animate-none" />
                  ) : stepState === "current" ? (
                    <ClockIcon className="size-3.5" />
                  ) : (
                    index + 1
                  )}
                </span>
                <div className="min-w-0">
                  <div
                    className={`font-medium ${stepState === "todo" ? "text-[var(--color-text-muted)]" : ""}`}
                  >
                    {label}
                  </div>
                  {hint ? (
                    <div className="mt-0.5 text-xs leading-relaxed text-[var(--color-text-muted)]">
                      {hint}
                    </div>
                  ) : null}
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
 * A numbered setup step. The active one is highlighted and fills spare height.
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
      className={`flex gap-3 border-t border-[var(--glass-border-subtle)] px-4 py-4 first:border-t-0 ${active ? "flex-1 bg-[var(--glass-tab-bg)]/40" : ""}`}
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
      {children ? <div className="mt-auto pt-4">{children}</div> : null}
    </section>
  );
}
