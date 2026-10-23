import type { ReactNode } from "react";
import { AlertIcon, CheckIcon, ClockIcon, RefreshIcon } from "../Icons";
import { DELIVERY_STEPS, deliveryProgress, type StepState } from "./nativeActorStatus";

type T = (key: string, options?: Record<string, unknown>) => string;

export const nativeActorCard =
  "rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]";

export function NativeActorSummary({
  cells,
  compact = false,
}: {
  cells: Array<[string, string]>;
  compact?: boolean;
}) {
  return (
    <dl
      className={
        compact
          ? "flex shrink-0 flex-wrap gap-x-4 gap-y-2 border-b border-[var(--glass-border-subtle)] pb-2 text-xs"
          : `${nativeActorCard} grid shrink-0 grid-cols-1 overflow-hidden @xl/nativeActor:grid-cols-3`
      }
    >
      {cells.map(([label, value], index) => (
        <div
          key={label}
          className={
            compact
              ? "min-w-0"
              : `min-w-0 px-4 py-3 ${index ? "border-t border-[var(--glass-border-subtle)] @xl/nativeActor:border-t-0 @xl/nativeActor:border-l" : ""}`
          }
        >
          <dt className="text-xs text-[var(--color-text-muted)]">{label}</dt>
          <dd className="mt-1 font-semibold [overflow-wrap:anywhere]">{value}</dd>
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

export function NativeActorDelivery({
  phase,
  provider,
  t,
  compact = false,
  children,
}: {
  phase: string;
  provider: "dots" | "grok";
  t: T;
  compact?: boolean;
  children?: ReactNode;
}) {
  const progress = deliveryProgress(phase);
  if (!progress) return children ? <>{children}</> : null;
  const messages = `${provider}Actor`;
  return (
    <section
      className={compact ? "shrink-0" : `${nativeActorCard} shrink-0`}
      aria-label={t("nativeActor.deliveryTitle")}
    >
      {!compact && (
        <div className="border-b border-[var(--glass-border-subtle)] px-4 py-3">
          <h3 className="font-semibold">{t("nativeActor.deliveryTitle")}</h3>
          <p className="mt-0.5 text-xs text-[var(--color-text-muted)]">
            {t(`${messages}.deliveryHint`)}
          </p>
        </div>
      )}
      <ol className={compact ? "" : "px-4"}>
        {DELIVERY_STEPS.map((step, index) => {
          if (compact && index !== progress.finished) return null;
          const stepState: StepState =
            index < progress.finished
              ? "done"
              : index === progress.finished
                ? progress.next
                : "todo";
          const sending = stepState === "current" && phase === "delivering";
          const label =
            stepState === "error"
              ? t(`nativeActor.${phase}`)
              : sending
                ? t("nativeActor.steps.sending")
                : step === "waiting"
                  ? t(`${messages}.steps.waiting`)
                  : t(`nativeActor.steps.${step}`);
          const hint =
            stepState === "current" ? t(`${messages}.steps.${sending ? "sending" : step}Hint`) : "";
          return (
            <li
              key={step}
              data-step={step}
              data-state={stepState}
              aria-current={stepState === "current" || stepState === "error" ? "step" : undefined}
              className={
                compact
                  ? "flex min-w-0 items-start gap-2 py-1"
                  : "flex min-w-0 items-start gap-3 border-b border-[var(--glass-border-subtle)] py-3 last:border-b-0"
              }
            >
              <span
                className={`inline-flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${stepDot[stepState]}`}
                aria-hidden="true"
              >
                {stepState === "done" ? (
                  <CheckIcon className="size-3.5" />
                ) : stepState === "error" ? (
                  <AlertIcon className="size-3.5" />
                ) : sending ? (
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
                {hint && !compact && (
                  <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-text-muted)]">
                    {hint}
                  </p>
                )}
              </div>
            </li>
          );
        })}
      </ol>
      {children && (
        <div
          className={
            compact
              ? "mt-2 border-t border-[var(--glass-border-subtle)] pt-2"
              : "border-t border-[var(--glass-border-subtle)] px-4 py-3"
          }
        >
          {children}
        </div>
      )}
    </section>
  );
}
